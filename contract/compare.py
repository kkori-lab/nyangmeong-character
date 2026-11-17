"""M2 대조 테스트 : 같은 요청을 Python(nyangmeong-ai)과 러스트에 보내 응답을 비교한다

CASES : 핸들러 본문 전(본문 읽기 → X-Requested-With → 인증 → 값 검증)에서 끝나는 요청
SCENARIOS : 핸들러까지 가는 요청. 사례마다 가짜 Java·가짜 OpenAI의 답을 정해 주고, 두 서버가 그 가짜들에 보낸 요청도 비교한다
  응답 뒤 작업(생성·굽기)이 있는 사례는 `wait`의 요청이 가짜 Java에 기록되고 기록이 멈출 때까지 기다린 뒤 비교한다.
  굽기 여러 개가 동시에 도는 사례(`unordered`)는 요청 순서가 매번 달라 기록을 정렬해 비교한다
비교: 상태 코드, Content-Type·Set-Cookie·Allow·Location, 본문(timestamp는 모양만 확인, PNG는 크기·화소 해시)

사용: python contract/compare.py [Python 주소] [러스트 주소]
"""

import base64
import hashlib
import http.client
import io
import json
import re
import sys
import time
from pathlib import Path
from urllib.parse import urlsplit

XRW = {"X-Requested-With": "XMLHttpRequest"}
JSON = {"Content-Type": "application/json"}
PNG = b"\x89PNG\r\n\x1a\n" + b"0" * 32
TS = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d{6})?$")


def cookie(token: str) -> dict[str, str]:
    return {"Cookie": f"access_token={token}"}


def multipart(parts: list[tuple[str, str | None, bytes]], boundary: str = "BnD") -> tuple[dict, bytes]:
    body = b""
    for name, filename, data in parts:
        disposition = f'form-data; name="{name}"'
        if filename is not None:
            disposition += f'; filename="{filename}"'
        body += f"--{boundary}\r\nContent-Disposition: {disposition}\r\n".encode()
        if filename is not None:
            body += b"Content-Type: image/png\r\n"
        body += b"\r\n" + data + b"\r\n"
    body += f"--{boundary}--\r\n".encode()
    return {"Content-Type": f"multipart/form-data; boundary={boundary}"}, body


def case(name, method, path, headers=None, body=b""):
    return {"name": name, "method": method, "path": path, "headers": headers or {}, "body": body}


def form_case(name, path, token, parts, xrw=True):
    headers, body = multipart(parts)
    if xrw:
        headers |= XRW
    if token:
        headers |= cookie(token)
    return case(name, "POST", path, headers, body)


def json_case(name, path, token, body, xrw=True, content_type=JSON):
    headers = dict(content_type)
    if xrw:
        headers |= XRW
    if token:
        headers |= cookie(token)
    data = body if isinstance(body, bytes) else json.dumps(body).encode()
    return case(name, "POST", path, headers, data)


PHOTO = ("photo", "a.png", PNG)
IMAGE = ("image", "a.png", PNG)

CASES = [
    # 라우터 : 404·405·끝 / 리다이렉트·health
    case("health", "GET", "/health"),
    case("405 GET", "GET", "/ai/characters"),
    case("405 HEAD", "HEAD", "/ai/characters"),
    case("405 퍼센트 경로", "GET", "/ai/items/bake%2Dhints"),
    case("404", "POST", "/ai/nothing", XRW),
    case("404 items 루트", "POST", "/ai/items", XRW),
    case("307 끝 슬래시", "POST", "/ai/characters/", XRW | {"Host": "example.test"}),
    case("307 쿼리 유지", "POST", "/ai/items/bake-hints/?a=1", XRW | {"Host": "h"}),
    # X-Requested-With
    case("403 XRW 없음", "POST", "/ai/characters"),
    case("403 XRW 소문자 값", "POST", "/ai/characters", {"X-Requested-With": "xmlhttprequest"}),
    case("403 관리자 경로 XRW 없음", "POST", "/ai/items/1/bake"),
    # 인증
    case("401 쿠키 없음", "POST", "/ai/characters", XRW),
    case("401 쿠키 틀림", "POST", "/ai/items/1/bake", XRW | cookie("nope")),
    case("403 정지 회원", "POST", "/ai/characters", XRW | cookie("banned")),
    case("401 html 본문", "POST", "/ai/characters", XRW | cookie("html")),
    case("502 Java 500", "POST", "/ai/characters", XRW | cookie("boom")),
    case("502 응답 모양 틀림", "POST", "/ai/characters", XRW | cookie("weird")),
    case("403 회원이 관리자 경로", "POST", "/ai/items/1/bake", XRW | cookie("member")),
    case("403 회원이 bake-hints", "POST", "/ai/items/bake-hints", XRW | cookie("member")),
    # 본문 읽기 오류는 XRW·인증보다 먼저
    case("400 boundary 없음(XRW 없음)", "POST", "/ai/characters", {"Content-Type": "multipart/form-data"}),
    case(
        "400 multipart 깨짐",
        "POST",
        "/ai/characters",
        {"Content-Type": "multipart/form-data; boundary=BnD"},
        b"--BnD\r\nContent-Disposition: form-data\r\n\r\nx\r\n--BnD--\r\n",
    ),
    json_case("400 JSON 문법(XRW 없음)", "/ai/characters/1/preview", None, b"{", xrw=False),
    json_case("400 JSON UTF-8 깨짐", "/ai/characters/1/preview", None, b'{"a":"\xff"}', xrw=False),
    json_case("403 JSON 정상인데 XRW 없음", "/ai/characters/1/preview", None, {"itemSns": []}, xrw=False),
    # 값 검증 (인증 뒤)
    case("400 생성 본문 없음", "POST", "/ai/characters", XRW | cookie("member")),
    form_case("400 petSn 문자", "/ai/characters", "member", [PHOTO, ("petSn", None, b"abc")]),
    form_case("400 petSn 1.5", "/ai/characters", "member", [PHOTO, ("petSn", None, b"1.5")]),
    form_case("400 photo 없음", "/ai/characters", "member", [("petSn", None, b"1")]),
    form_case("400 photo가 글자", "/ai/characters", "member", [("photo", None, b"x"), ("petSn", None, b"1")]),
    form_case("400 petSn 빈 값", "/ai/characters", "member", [PHOTO, ("petSn", None, b"")]),
    case(
        "400 urlencoded",
        "POST",
        "/ai/characters",
        XRW | cookie("member") | {"Content-Type": "application/x-www-form-urlencoded"},
        b"petSn=1",
    ),
    json_case("400 preview 경로 문자", "/ai/characters/abc/preview", "member", {"itemSns": [1]}),
    json_case("400 preview 경로 1.5", "/ai/characters/1.5/preview", "member", {"itemSns": [1]}),
    json_case("400 preview 배열 본문", "/ai/characters/1/preview", "member", [1]),
    json_case("400 preview itemSns 문자", "/ai/characters/1/preview", "member", {"itemSns": "1"}),
    json_case("400 preview itemSns 1.5", "/ai/characters/1/preview", "member", {"itemSns": [1.5]}),
    json_case("400 preview 본문 없음", "/ai/characters/1/preview", "member", b""),
    json_case(
        "400 preview text/plain",
        "/ai/characters/1/preview",
        "member",
        {"itemSns": [1]},
        content_type={"Content-Type": "text/plain"},
    ),
    json_case("400 equipped 경로 문자", "/ai/characters/x/equipped-image", "member", {"itemSns": []}),
    form_case(
        "400 bake-hints itemNm 51자",
        "/ai/items/bake-hints",
        "admin",
        [IMAGE, ("regnCd", None, b"HEAD"), ("itemNm", None, ("가" * 51).encode())],
    ),
    form_case("400 bake-hints regnCd 없음", "/ai/items/bake-hints", "admin", [IMAGE, ("itemNm", None, b"a")]),
    form_case("400 pixel-image image 없음", "/ai/items/pixel-image", "admin", [("regnCd", None, b"H"), ("itemNm", None, b"a")]),
    json_case("400 trial bakeHint 빈 값", "/ai/items/trial-preview", "admin", {"pixelCharSn": 1, "regnCd": "H", "bakeHint": ""}),
    json_case("400 trial regnCd 숫자", "/ai/items/trial-preview", "admin", {"pixelCharSn": 1, "regnCd": 1, "bakeHint": "a"}),
    json_case("400 trial bakeHint 301자", "/ai/items/trial-preview", "admin", {"pixelCharSn": 1, "regnCd": "H", "bakeHint": "가" * 301}),
    case("400 bake 경로 문자", "POST", "/ai/items/abc/bake", XRW | cookie("admin")),
    case("400 bake 경로 1.5 (mbrSn 실수 관리자)", "POST", "/ai/items/1.5/bake", XRW | cookie("floaty")),
]

# ---------- 핸들러까지 가는 요청 ----------

FIX = Path(__file__).resolve().parent.parent / "tests" / "fixtures" / "image"


def b64(path: Path) -> str:
    return base64.b64encode(path.read_bytes()).decode()


def fixture(name: str) -> bytes:
    return (FIX / name).read_bytes()


HAT = fixture("items/hat.png")
SHEETS = {"petSeCd": "PETC0001", "sheetFileSn": 50, "scale": 0.5, "originX": 10.5, "originY": 20, "items": []}
JAVA_OK = {"sheets": {"status": 200, "data": SHEETS}, "files": {"50": b64(FIX / "sheets/plain/bare.png")}}


def worn(regn: str) -> dict:
    return {"status": 200, "file_b64": b64(FIX / f"sheets/plain/worn_{regn}.png")}


def drawn(data: bytes) -> dict:
    return {"status": 200, "file_b64": base64.b64encode(data).decode()}


def scenario(name, request, java=None, openai=None, wait=None, unordered=False):
    """wait: (경로 정규식, 수) 또는 경로 정규식 : 가짜 Java가 그 요청을 그만큼 받을 때까지 기다림"""
    if isinstance(wait, str):
        wait = (wait, 1)
    return {"name": name, "request": request, "java": java or {}, "openai": openai or {}, "wait": wait, "unordered": unordered}


def item_form(path, parts=None, token="admin", filename="hat.png", regn=b"MYPC0001", name="빨간 모자".encode()):
    parts = parts or [("image", filename, HAT), ("regnCd", None, regn), ("itemNm", None, name)]
    return form_case("-", path, token, parts)


def chat(content, status=200, **extra):
    return {"status": status, "content": content} | extra


ERR500 = {"status": 500}
HINT = "/ai/items/bake-hints"
PIXEL = "/ai/items/pixel-image"
TRIAL = "/ai/items/trial-preview"


def trial_request(regn="MYPC0002", hint="round glasses", sn=77, token="admin"):
    return json_case("-", TRIAL, token, {"pixelCharSn": sn, "regnCd": regn, "bakeHint": hint})


def bad_form(path, data, regn=b"MYPC0001"):
    return item_form(path, parts=[("image", "a.png", data), ("regnCd", None, regn), ("itemNm", None, b"x")])


SCENARIOS = [
    # bake-hints : OpenAI 비전 -> 첫 줄 정리
    scenario("bake-hints 성공", item_form(HINT), openai={"chat": [chat("- A red cap.\nextra line")]}),
    scenario("bake-hints 성공 다른 부위·이름", item_form(HINT, regn=b"MYPC0005", name='가방 "큰"'.encode()), openai={"chat": [chat('"a blue bag"')]}),
    scenario("bake-hints 500 두 번", item_form(HINT), openai={"chat": [ERR500, ERR500]}),
    scenario("bake-hints 500 뒤 성공(재시도)", item_form(HINT), openai={"chat": [ERR500, chat("a cap")]}),
    scenario("bake-hints 429 retry-after-ms 뒤 성공", item_form(HINT), openai={"chat": [{"status": 429, "headers": {"retry-after-ms": "20"}}, chat("a cap")]}),
    scenario("bake-hints 400은 재시도 없음", item_form(HINT), openai={"chat": [{"status": 400}, chat("a cap")]}),
    scenario("bake-hints x-should-retry false", item_form(HINT), openai={"chat": [{"status": 500, "headers": {"x-should-retry": "false"}}, chat("a cap")]}),
    scenario("bake-hints x-should-retry true", item_form(HINT), openai={"chat": [{"status": 400, "headers": {"x-should-retry": "true"}}, chat("a cap")]}),
    scenario("bake-hints 빈 답", item_form(HINT), openai={"chat": [chat("")]}),
    scenario("bake-hints content null", item_form(HINT), openai={"chat": [chat(None)]}),
    scenario("bake-hints 151자", item_form(HINT), openai={"chat": [chat("a" * 151)]}),
    scenario("bake-hints 150자", item_form(HINT), openai={"chat": [chat("a" * 150)]}),
    scenario("bake-hints 신발 부위", item_form(HINT, regn=b"MYPC0006")),
    scenario("bake-hints 확장자 틀림", item_form(HINT, filename="hat.gif")),
    scenario("bake-hints 그림 아님", bad_form(HINT, b"not an image at all")),
    scenario("bake-hints 서명만 PNG(열 수 없음)", bad_form(HINT, PNG), openai={"chat": [chat("a cap")]}),
    # pixel-image : OpenAI 이미지 편집 -> cut_item
    scenario("pixel-image 성공", item_form(PIXEL), openai={"images": [drawn(HAT)]}),
    scenario("pixel-image 500 뒤 성공(재시도)", item_form(PIXEL, regn=b"MYPC0003"), openai={"images": [ERR500, drawn(HAT)]}),
    scenario("pixel-image 500 두 번", item_form(PIXEL), openai={"images": [ERR500, ERR500]}),
    scenario("pixel-image 빈 아이템 그림", item_form(PIXEL), openai={"images": [drawn(fixture("items/empty.png"))]}),
    scenario("pixel-image 그림 아닌 답", item_form(PIXEL), openai={"images": [drawn(b"hello")]}),
    scenario("pixel-image b64 아닌 글자", item_form(PIXEL), openai={"images": [{"status": 200, "file_b64": "!!!"}]}),
    scenario("pixel-image 알파 그림", item_form(PIXEL), openai={"images": [drawn(fixture("items/alpha.png"))]}),
    scenario("pixel-image 신발 부위", item_form(PIXEL, regn=b"MYPC0006")),
    # trial-preview : Java 시트 + OpenAI 굽기 + 합성
    scenario("trial-preview 성공(안경)", trial_request(), java=JAVA_OK, openai={"images": [worn("MYPC0002")]}),
    scenario("trial-preview 성공(머리)", trial_request("MYPC0001", "a hat on a dog"), java=JAVA_OK, openai={"images": [worn("MYPC0001")]}),
    scenario(
        "trial-preview 성공(몸통·고양이 시트)",
        trial_request("MYPC0004", "a red dog coat"),
        java={"sheets": {"status": 200, "data": SHEETS | {"petSeCd": "PETC0002", "scale": "0.25", "originY": "100"}}, "files": JAVA_OK["files"]},
        openai={"images": [worn("MYPC0004")]},
    ),
    scenario("trial-preview 구운 시트가 맨몸과 같음", trial_request(), java=JAVA_OK, openai={"images": [{"status": 200, "file_b64": b64(FIX / "sheets/plain/bare.png")}]}),
    scenario("trial-preview OpenAI 500 두 번", trial_request(), java=JAVA_OK, openai={"images": [ERR500, ERR500]}),
    scenario("trial-preview 500 뒤 성공(재시도)", trial_request(), java=JAVA_OK, openai={"images": [ERR500, worn("MYPC0002")]}),
    scenario("trial-preview 캐릭터 없음 404", trial_request()),
    scenario("trial-preview 시트 파일 없음 404", trial_request(), java=JAVA_OK | {"file_status": {"50": 404}}),
    scenario("trial-preview Java 500", trial_request(), java={"sheets": {"status": 500}}),
    scenario("trial-preview 응답 모양 틀림", trial_request(), java={"sheets": {"status": 200, "data": {"petSeCd": "PETC9999"}}}),
    scenario("trial-preview 빈 시트 파일", trial_request(), java=JAVA_OK | {"files": {"50": ""}}),
    scenario("trial-preview 신발 부위", trial_request("MYPC0006")),
    scenario("trial-preview 관리자 아님", trial_request(token="member")),
]

JOB = {"charCrtJobSn": 31, "petSeCd": "PETC0001", "vrtyNm": "말티즈", "colrNm": None, "chrcCn": "귀 접힘"}
JOBS = "/internal/ai/characters/jobs"
EYES = chat('{"shape": "almond", "color": "brown"}')
BARE = {"status": 200, "file_b64": b64(FIX / "sheets/plain/bare.png")}
PHOTO_PNG = fixture("photos/rgb_1300x1000.png")
PHOTO_WEBP = fixture("photos/webp_lossy_700x500.webp")
CREATE = "/ai/characters"


def route(method, path, **answer):
    return {"method": method, "path": path} | answer


def bake_items(*rows):
    data = [{"itemSn": sn, "regnCd": regn, "bakeHint": f"item {sn}", "bakeImgRefYn": False} for sn, regn in rows]
    return route("GET", "/internal/ai/characters/bake-items", data=data)


def job_route(**job):
    return route("POST", JOBS, data=JOB | job)


def create_form(token="member", filename="dog.png", photo=PHOTO_PNG, pet_sn=b"3"):
    return form_case("-", CREATE, token, [("photo", filename, photo), ("petSn", None, pet_sn)])


def regenerate(sn="31", token="admin"):
    return case("-", "POST", f"/ai/admin/characters/jobs/{sn}/regenerate", XRW | cookie(token))


def preview(item_sns, kind="preview", sn=1, token="member"):
    return json_case("-", f"/ai/characters/{sn}/{kind}", token, {"itemSns": item_sns})


def bake(item_sn="9", token="admin"):
    return case("-", "POST", f"/ai/items/{item_sn}/bake", XRW | cookie(token))


def baked_sheets(items):
    """맨몸 50번, 몸통 아이템 5번이 51번 파일로 구워진 캐릭터"""
    return {
        "sheets": {"status": 200, "data": SHEETS | {"items": items}},
        "files": {"50": b64(FIX / "sheets/plain/bare.png"), "51": b64(FIX / "sheets/plain/worn_MYPC0004.png")},
    }


SHOP = bake_items((5, "MYPC0004"), (6, "MYPC0004"), (7, "MYPC0002"))
BAKED_5 = [{"itemSn": 5, "atchFileSn": 51, "bakeFailed": False}]
ITEM_SHEET = r"/internal/ai/characters/\d+/item-sheets/\d+"
WORN_4 = worn("MYPC0004")

SCENARIOS += [
    # /ai/characters : 202 뒤 생성·보고·굽기
    scenario(
        "characters 성공(굽기 대상 없음)",
        create_form(),
        java={"routes": [job_route(), bake_items()]},
        openai={"chat": [EYES], "images": [BARE]},
        wait=r".*/bake-items",
    ),
    scenario(
        "characters 성공 뒤 아이템 2개 굽기",
        create_form(),
        java={"routes": [job_route(), bake_items((5, "MYPC0004"), (6, "MYPC0004"))]},
        openai={"chat": [EYES], "images": [BARE, WORN_4, WORN_4]},
        wait=(ITEM_SHEET, 2),
        unordered=True,
    ),
    scenario(
        "characters 고양이·webp 한글 파일 이름",
        create_form(filename="고양이 사진.webp", photo=PHOTO_WEBP, pet_sn=b"4"),
        java={"routes": [job_route(petSeCd="PETC0002", vrtyNm=None, chrcCn='"꼬리" {길다}'), bake_items()]},
        openai={"chat": [EYES], "images": [BARE]},
        wait=r".*/bake-items",
    ),
    scenario(
        "characters 눈 못 찾음 -> 규격 실패",
        create_form(),
        java={"routes": [job_route()]},
        openai={"chat": [EYES], "images": [{"status": 200, "file_b64": b64(FIX / "sheets/noeyes/bare.png")}]},
        wait=r".*/failure",
    ),
    scenario(
        "characters OpenAI 500 두 번 -> 생성 실패",
        create_form(),
        java={"routes": [job_route()]},
        openai={"chat": [EYES], "images": [ERR500, ERR500]},
        wait=r".*/failure",
    ),
    scenario(
        "characters 눈 판별 실패는 기본값으로 생성",
        create_form(),
        java={"routes": [job_route(), bake_items()]},
        openai={"chat": [{"status": 400}], "images": [BARE]},
        wait=r".*/bake-items",
    ),
    scenario(
        "characters 성공 보고 Java 500 -> 멈춤",
        create_form(),
        java={"routes": [job_route(), route("POST", r".*/success", status=500)]},
        openai={"chat": [EYES], "images": [BARE]},
        wait=r".*/success",
    ),
    scenario("characters Java 409 그대로", create_form(), java={"routes": [route("POST", JOBS, status=409, code="CHARACTER_ALREADY_EXISTS")]}),
    scenario("characters 작업 응답 모양 틀림 502", create_form(), java={"routes": [route("POST", JOBS, data={"charCrtJobSn": "x"})]}),
    scenario("characters PDF 400", create_form(filename="p.pdf", photo=b"%PDF-1.7"), java={"routes": [job_route()]}),
    scenario("characters 확장자 틀림 400", create_form(filename="p.jpg"), java={"routes": [job_route()]}),
    # 관리자 무료 재생성
    scenario(
        "regenerate 성공",
        regenerate(),
        java={"routes": [route("POST", r".*/regen-start", data=JOB), route("GET", r".*/original-photo", file_b64=base64.b64encode(PHOTO_PNG).decode()), bake_items()]},
        openai={"chat": [EYES], "images": [BARE]},
        wait=r".*/bake-items",
    ),
    scenario("regenerate 시작 409", regenerate(), java={"routes": [route("POST", r".*/regen-start", status=409, code="ALREADY_PROCESSED")]}, wait=r".*/regen-start"),
    scenario(
        "regenerate 원본 사진 없음 -> 입력 실패",
        regenerate(),
        java={"routes": [route("POST", r".*/regen-start", data=JOB), route("GET", r".*/original-photo", status=404, code="FILE_NOT_FOUND")]},
        wait=r".*/failure",
    ),
    scenario("regenerate 회원은 403", regenerate(token="member")),
    scenario("regenerate 경로 문자 400", regenerate(sn="abc")),
    # 미리보기·대표 이미지
    scenario("preview 맨몸", preview([]), java=baked_sheets([]) | {"routes": [SHOP]}),
    scenario("preview 구운 아이템", preview([5]), java=baked_sheets(BAKED_5) | {"routes": [SHOP]}, unordered=True),
    scenario(
        "preview 안 구운 아이템 409 뒤 굽기",
        preview([7]),
        java=baked_sheets([]) | {"routes": [SHOP]},
        openai={"images": [worn("MYPC0002")]},
        wait=ITEM_SHEET,
    ),
    scenario("preview 굽기 실패 아이템 502", preview([7]), java=baked_sheets([{"itemSn": 7, "atchFileSn": None, "bakeFailed": True}]) | {"routes": [SHOP]}),
    scenario("preview 같은 부위 두 개 400", preview([5, 6]), java=baked_sheets([]) | {"routes": [SHOP]}),
    scenario("preview 굽기 대상 아님 400", preview([999]), java=baked_sheets([]) | {"routes": [SHOP]}),
    scenario("preview 캐릭터 없음 404", preview([]), java={"routes": [SHOP]}),
    scenario(
        "preview 구운 시트 깨짐 502",
        preview([5]),
        java=baked_sheets(BAKED_5) | {"routes": [SHOP], "files": {"50": b64(FIX / "sheets/plain/bare.png"), "51": base64.b64encode(b"not image").decode()}},
        unordered=True,
    ),
    scenario("equipped 저장", preview([5], "equipped-image"), java=baked_sheets(BAKED_5) | {"routes": [SHOP]}, unordered=True),
    scenario("equipped 맨몸 저장", preview([], "equipped-image"), java=baked_sheets([]) | {"routes": [SHOP]}),
    scenario(
        "equipped Java 409 그대로",
        preview([5], "equipped-image"),
        java=baked_sheets(BAKED_5) | {"routes": [SHOP, route("PUT", r".*/equipped-image", status=409, code="EQUIPPED_ITEMS_CHANGED")]},
        unordered=True,
    ),
    scenario(
        "equipped 안 구운 아이템 409 뒤 굽기",
        preview([7], "equipped-image"),
        java=baked_sheets([]) | {"routes": [SHOP]},
        openai={"images": [worn("MYPC0002")]},
        wait=ITEM_SHEET,
    ),
    # 전체 굽기 한 묶음 (관리자)
    scenario(
        "bake 묶음 2개(중복 번호 제거)",
        bake(),
        java=baked_sheets([]) | {"routes": [bake_items((9, "MYPC0004")), route("GET", r".*/bake-targets", data={"pixelCharSns": [1, 2, 1], "last": True})]},
        openai={"images": [WORN_4, WORN_4]},
        unordered=True,
    ),
    scenario(
        "bake 남은 후보가 있으면 lastYn false",
        bake(),
        java=baked_sheets([]) | {"routes": [bake_items((9, "MYPC0004")), route("GET", r".*/bake-targets", data={"pixelCharSns": list(range(1, 15)), "last": True})]},
        openai={"images": [WORN_4] * 13},
        unordered=True,
    ),
    scenario(
        "bake 굽기 실패는 실패 표시",
        bake(),
        java=baked_sheets([]) | {"routes": [bake_items((9, "MYPC0004")), route("GET", r".*/bake-targets", data={"pixelCharSns": [1], "last": True})]},
        openai={"images": [ERR500] * 4},
    ),
    scenario(
        "bake 시트 파일 없음은 실패 표시",
        bake(),
        java=baked_sheets([]) | {"file_status": {"50": 404}, "routes": [bake_items((9, "MYPC0004")), route("GET", r".*/bake-targets", data={"pixelCharSns": [1, 2], "last": False})]},
        unordered=True,
    ),
    scenario(
        "bake 하나도 못 줄이면 502",
        bake(),
        java={"sheets": {"status": 500}, "routes": [bake_items((9, "MYPC0004")), route("GET", r".*/bake-targets", data={"pixelCharSns": [1], "last": True})]},
    ),
    scenario("bake 대상 아닌 아이템 400", bake(), java={"routes": [bake_items((8, "MYPC0004"))]}),
    scenario("bake 대상 없음", bake(), java={"routes": [bake_items((9, "MYPC0004")), route("GET", r".*/bake-targets", data={"pixelCharSns": [], "last": True})]}),
]


def control(base: str, method: str, path: str, body=None):
    url = urlsplit(base)
    conn = http.client.HTTPConnection(url.hostname, url.port, timeout=10)
    data = json.dumps(body).encode() if body is not None else None
    conn.request(method, path, data, {"Content-Type": "application/json"} if data else {})
    res = conn.getresponse()
    out = res.read()
    conn.close()
    return json.loads(out)


WAIT_SEC = 30
STABLE_SEC = 0.5


def settle(fakes: dict, wait) -> None:
    """응답 뒤 작업 : 기다릴 요청이 기록되고, 그 뒤 기록이 STABLE_SEC 동안 늘지 않을 때까지"""
    deadline = time.monotonic() + WAIT_SEC
    if wait:
        pattern, count = wait
        while time.monotonic() < deadline:
            log = control(fakes["java"], "GET", "/__log")
            if sum(1 for r in log if re.fullmatch(pattern, r["path"])) >= count:
                break
            time.sleep(0.1)
    sizes = None
    while time.monotonic() < deadline:
        now = tuple(len(control(fake, "GET", "/__log")) for fake in (fakes["java"], fakes["openai"]))
        if now == sizes:
            return
        sizes = now
        time.sleep(STABLE_SEC)


def run_scenario(base: str, fakes: dict, s: dict) -> dict:
    for fake in fakes.values():
        control(fake, "POST", "/__reset")
    control(fakes["java"], "POST", "/__script", s["java"])
    control(fakes["openai"], "POST", "/__script", s["openai"])
    res = send(base, s["request"])
    if s["wait"]:
        settle(fakes, s["wait"])
    logs = {key: control(fakes[key], "GET", "/__log") for key in ("java", "openai")}
    if s["unordered"]:
        logs = {key: sorted(log, key=lambda r: json.dumps(r, sort_keys=True, ensure_ascii=False)) for key, log in logs.items()}
    return {"response": res} | logs


COMPARED_HEADERS = ("content-type", "allow", "location")


def send(base: str, c: dict) -> dict:
    url = urlsplit(base)
    conn = http.client.HTTPConnection(url.hostname, url.port, timeout=60)
    headers = {"Content-Length": str(len(c["body"]))} | c["headers"]
    conn.putrequest(c["method"], c["path"], skip_host="Host" in headers, skip_accept_encoding=True)
    for key, value in headers.items():
        conn.putheader(key, value)
    conn.endheaders(c["body"] or None)
    res = conn.getresponse()
    body = res.read()
    conn.close()
    return {
        "status": res.status,
        "headers": {k: res.getheader(k) for k in COMPARED_HEADERS},
        "set-cookie": res.headers.get_all("set-cookie") or [],
        "body": normalize(body),
    }


def normalize(body: bytes):
    if body.startswith(b"\x89PNG\r\n\x1a\n"):
        from PIL import Image

        image = Image.open(io.BytesIO(body))
        return {"png": [image.width, image.height, hashlib.sha1(image.convert("RGBA").tobytes()).hexdigest()]}
    try:
        data = json.loads(body)
    except ValueError:
        return body
    if isinstance(data, dict) and "timestamp" in data:
        if not TS.match(str(data["timestamp"])) and not str(data["timestamp"]).startswith("2026"):
            data["timestamp"] = f"모양 틀림:{data['timestamp']}"
        else:
            data["timestamp"] = "<ts>"
    # 원문 그대로도 비교 (공백·순서까지)
    return {"json": data, "raw": re.sub(rb'"timestamp": ?"[^"]*"', b'"timestamp":"<ts>"', body)}


def first_diff(a, b, path="") -> str | None:
    """두 값이 처음 달라지는 자리"""
    if type(a) is not type(b):
        return f"{path}: {a!r} != {b!r}"
    if isinstance(a, dict):
        for key in sorted(set(a) | set(b)):
            if key not in a or key not in b:
                return f"{path}.{key}: {a.get(key)!r} != {b.get(key)!r}"
            found = first_diff(a[key], b[key], f"{path}.{key}")
            if found:
                return found
        return None
    if isinstance(a, list) and a and all(isinstance(x, str) for x in a + b):
        return None if a == b else f"{path}: {a!r} != {b!r}"
    if isinstance(a, list):
        if len(a) != len(b):
            brief = lambda log: [f"{x.get('method')} {x.get('path')}" if isinstance(x, dict) else x for x in log]
            return f"{path}: 길이 {len(a)} != {len(b)} : {brief(a)} != {brief(b)}"
        for at, (x, y) in enumerate(zip(a, b)):
            found = first_diff(x, y, f"{path}[{at}]")
            if found:
                return found
        return None
    return None if a == b else f"{path}: {a!r} != {b!r}"


def report(a: dict, b: dict, keys) -> None:
    for key in keys:
        if a[key] != b[key]:
            print(f"   {key}")
            print(f"     Python: {a[key]!r}")
            print(f"     러스트: {b[key]!r}")


def main(python: str, rust: str, verbose: bool = False, fakes: dict | None = None) -> int:
    """fakes: {"python": {"java": 주소, "openai": 주소}, "rust": {...}} 가 있으면 핸들러까지 가는 사례도 비교한다"""
    failed = 0
    total = len(CASES)
    for c in CASES:
        a, b = send(python, c), send(rust, c)
        if a == b:
            if verbose:
                print(f"같음  {a['status']}  {c['name']}")
            continue
        failed += 1
        print(f"다름  {c['name']}")
        report(a, b, ("status", "headers", "set-cookie", "body"))
    for s in SCENARIOS if fakes else []:
        total += 1
        a = run_scenario(python, fakes["python"], s)
        b = run_scenario(rust, fakes["rust"], s)
        if a == b:
            if verbose:
                print(f"같음  {a['response']['status']}  {s['name']}")
            continue
        failed += 1
        print(f"다름  {s['name']}")
        if a["response"] != b["response"]:
            print("   응답")
            report(a["response"], b["response"], ("status", "headers", "set-cookie", "body"))
        for key, label in (("java", "가짜 Java가 받은 요청"), ("openai", "가짜 OpenAI가 받은 요청")):
            if a[key] != b[key]:
                print(f"   {label}: {first_diff(a[key], b[key])}")
    print(f"{total - failed}/{total} 같음")
    return 1 if failed else 0


if __name__ == "__main__":
    # 서버를 직접 띄웠을 때 : python contract/compare.py [Python 주소] [러스트 주소]
    python = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:18000"
    rust = sys.argv[2] if len(sys.argv) > 2 else "http://127.0.0.1:18100"
    sys.exit(main(python, rust, verbose=True))
