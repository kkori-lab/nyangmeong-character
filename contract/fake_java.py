"""대조 테스트용 가짜 Java : /internal/auth/check만 쿠키 값으로 답한다

캐릭터 내부 API(`/internal/ai/**`)는 받은 요청을 기록하고(`GET /__log`, `POST /__reset`), 사례가 정한 답을 준다(`POST /__script`):
  {"sheets": {"status": 200, "data": {...}}, "files": {"<파일 번호>": "<PNG base64>"}, "file_status": {"<번호>": 404}}
쓰기 요청(POST·PUT)은 기록만 하고 200 {"data": {"pixelCharSn": 77}}로 답한다.
사용: python contract/fake_java.py [포트=18080]
쿠키 access_token 값 → 응답
  member  200 일반 회원        admin  200 관리자(USRC0005)
  banned  403 + Set-Cookie     html   401 text/html(charset 없음)
  boom    500                  weird  200 모양 틀림(mbrSn 문자)
  floaty  200 mbrSn 7.9        그 밖·없음 401 + Set-Cookie 2개
"""

import base64
import json
import re
import sys
from http.server import BaseHTTPRequestHandler
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import fakelib  # noqa: E402

RECORDER = fakelib.Recorder()
JSON_TYPE = "application/json;charset=UTF-8"

SET_COOKIES = [
    "access_token=; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:10 GMT; Path=/; HttpOnly",
    "refresh_token=; Max-Age=0; Path=/; HttpOnly",
]


def java_error(status: int, message: str, code: str, path: str) -> bytes:
    body = {
        "timestamp": "2026-11-16T11:16:36.207723553",
        "status": "error",
        "httpCode": status,
        "message": message,
        "path": path,
        "code": code,
    }
    return json.dumps(body, ensure_ascii=False, separators=(",", ":")).encode()


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args: object) -> None:
        pass

    def send(self, status: int, body: bytes, content_type: str | None, cookies=()) -> None:
        self.send_response(status)
        if content_type:
            self.send_header("Content-Type", content_type)
        for cookie in cookies:
            self.send_header("Set-Cookie", cookie)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self) -> None:
        self.write_request()

    def do_PUT(self) -> None:
        self.write_request()

    def write_request(self) -> None:
        body = fakelib.read_body(self)
        if fakelib.serve_control(self, RECORDER, body):
            return
        RECORDER.log.append(fakelib.record_of(self, body))
        envelope = {"timestamp": "t", "status": "success", "httpCode": 200, "message": "ok", "data": {"pixelCharSn": 77}}
        self.send(200, json.dumps(envelope).encode(), JSON_TYPE)

    def do_GET(self) -> None:
        if fakelib.serve_control(self, RECORDER, b""):
            return
        path = self.path.split("?")[0]
        # Python 앱이 캐릭터와 상관없이 주기로 부르는 도구 목록 : 기록에서 뺀다
        if not path.startswith("/internal/ai/tools"):
            RECORDER.log.append(fakelib.record_of(self, b""))
        if path.startswith("/internal/ai/"):
            self.serve_character(path)
            return
        if self.path != "/internal/auth/check":
            self.send(404, java_error(404, "없음", "NOT_FOUND", self.path), "application/json")
            return
        cookie = self.headers.get("Cookie") or ""
        token = cookie.removeprefix("access_token=")
        json_type = "application/json;charset=UTF-8"
        if token in ("member", "admin", "weird", "floaty"):
            data = {
                "member": {"mbrSn": 1, "mbrAuthrtCd": "USRC0001"},
                "admin": {"mbrSn": 2, "mbrAuthrtCd": "USRC0005"},
                "weird": {"mbrSn": "x", "mbrAuthrtCd": "USRC0001"},
                "floaty": {"mbrSn": 7.9, "mbrAuthrtCd": "USRC0005"},
            }[token]
            body = {"timestamp": "t", "status": "success", "httpCode": 200, "message": "ok"}
            body["data"] = data
            self.send(200, json.dumps(body).encode(), json_type)
        elif token == "banned":
            body = java_error(403, "정지된 회원입니다.", "MEMBER_SUSPENDED", self.path)
            self.send(403, body, json_type, SET_COOKIES[:1])
        elif token == "html":
            self.send(401, b"<html>login</html>", "text/html")
        elif token == "boom":
            self.send(500, b"boom", "text/plain")
        else:
            body = java_error(401, "로그인이 필요합니다.", "UNAUTHORIZED", self.path)
            self.send(401, body, json_type, SET_COOKIES)


    def serve_character(self, path: str) -> None:
        script = RECORDER.script
        found = re.fullmatch(r"/internal/ai/characters/\d+/sheets", path)
        if found:
            step = script.get("sheets", {"status": 404, "code": "PIXEL_CHARACTER_NOT_FOUND"})
            if step.get("status", 200) == 200:
                envelope = {"timestamp": "t", "status": "success", "httpCode": 200, "message": "ok", "data": step["data"]}
                self.send(200, json.dumps(envelope).encode(), JSON_TYPE)
            else:
                body = java_error(step["status"], "오류", step.get("code", "ERROR"), path)
                self.send(step["status"], body, JSON_TYPE)
            return
        found = re.fullmatch(r"/internal/ai/characters/\d+/sheet-files/(\d+)", path)
        if found:
            number = found.group(1)
            status = script.get("file_status", {}).get(number, 200)
            data = base64.b64decode(script.get("files", {}).get(number, ""))
            if status == 200:
                self.send(200, data, "image/png")
            else:
                self.send(status, java_error(status, "파일 없음", "FILE_NOT_FOUND", path), JSON_TYPE)
            return
        self.send(404, java_error(404, "없음", "NOT_FOUND", path), JSON_TYPE)


if __name__ == "__main__":
    fakelib.run(Handler, int(sys.argv[1]) if len(sys.argv) > 1 else 18080)
