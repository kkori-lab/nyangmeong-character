"""M2 대조 테스트 : 같은 요청을 Python(nyangmeong-ai)과 러스트에 보내 응답을 비교한다

핸들러 본문 전(본문 읽기 → X-Requested-With → 인증 → 값 검증)에서 끝나는 요청만 보낸다.
비교: 상태 코드, Content-Type·Set-Cookie·Allow·Location, 본문(timestamp는 모양만 확인)

사용: python contract/compare.py [Python 주소] [러스트 주소]
"""

import http.client
import json
import re
import sys
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

COMPARED_HEADERS = ("content-type", "allow", "location")


def send(base: str, c: dict) -> dict:
    url = urlsplit(base)
    conn = http.client.HTTPConnection(url.hostname, url.port, timeout=10)
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


def main() -> int:
    python = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:18000"
    rust = sys.argv[2] if len(sys.argv) > 2 else "http://127.0.0.1:18100"
    failed = 0
    for c in CASES:
        a, b = send(python, c), send(rust, c)
        if a == b:
            print(f"같음  {a['status']}  {c['name']}")
            continue
        failed += 1
        print(f"다름  {c['name']}")
        for key in ("status", "headers", "set-cookie", "body"):
            if a[key] != b[key]:
                print(f"   {key}\n     Python: {a[key]!r}\n     러스트: {b[key]!r}")
    print(f"\n{len(CASES) - failed}/{len(CASES)} 같음")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
