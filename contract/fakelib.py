"""가짜 Java·가짜 OpenAI 공통 : 받은 요청을 기록하고(`/__log`), 사례마다 답을 정해 준다(`/__script`)

Python 서버와 러스트 서버가 각자 자기 가짜에 요청을 보내므로, 기록을 서로 비교하면 두 서버가 밖으로 보낸 요청이 같은지 알 수 있다.
기록은 비교하기 좋은 모양으로 줄인다: JSON은 파싱, multipart는 (이름, 파일 이름, 형식, 내용 요약) 목록, PNG는 크기·화소 해시(PNG 바이트는 두 구현이 다르다).
"""

import base64
import hashlib
import io
import json
import re
from email.parser import BytesParser
from email.policy import default as default_policy
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qsl, urlsplit

from PIL import Image

PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"
RECORDED_HEADERS = ("cookie", "x-requested-with", "authorization")


def png_summary(data: bytes) -> dict | None:
    """PNG면 크기·모드·화소 해시, 아니면 None"""
    if not data.startswith(PNG_SIGNATURE):
        return None
    image = Image.open(io.BytesIO(data))
    rgba = image.convert("RGBA")
    return {"png": [image.width, image.height, hashlib.sha1(rgba.tobytes()).hexdigest()]}


def summarize(data: bytes):
    found = png_summary(data)
    if found is not None:
        return found
    try:
        return {"text": data.decode("utf-8")}
    except UnicodeDecodeError:
        return {"bytes": [len(data), hashlib.sha1(data).hexdigest()]}


def data_uris_to_pixels(value):
    """JSON 안의 `data:image/png;base64,...` 주소를 그림 요약으로 바꾼다"""
    if isinstance(value, dict):
        return {key: data_uris_to_pixels(item) for key, item in value.items()}
    if isinstance(value, list):
        return [data_uris_to_pixels(item) for item in value]
    if isinstance(value, str):
        match = re.fullmatch(r"data:image/png;base64,(.*)", value, re.S)
        if match:
            return summarize(base64.b64decode(match.group(1)))
    return value


def parse_multipart(content_type: str, body: bytes) -> list[dict]:
    message = BytesParser(policy=default_policy).parsebytes(b"Content-Type: " + content_type.encode() + b"\r\n\r\n" + body)
    parts = []
    for part in message.iter_parts():
        parts.append(
            {
                "name": part.get_param("name", header="content-disposition"),
                "filename": part.get_filename(),
                "type": part.get_content_type() if part.get_filename() else None,
                "content": summarize(part.get_payload(decode=True) or b""),
            }
        )
    return parts


def record_of(handler: BaseHTTPRequestHandler, body: bytes) -> dict:
    url = urlsplit(handler.path)
    content_type = handler.headers.get("Content-Type") or ""
    record: dict = {
        "method": handler.command,
        "path": url.path,
        "query": sorted(parse_qsl(url.query, keep_blank_values=True)),
        "headers": {name: handler.headers.get(name) for name in RECORDED_HEADERS if handler.headers.get(name)},
    }
    if content_type.startswith("multipart/form-data"):
        record["multipart"] = sorted(
            parse_multipart(content_type, body), key=lambda p: (p["name"] or "", json.dumps(p["content"], sort_keys=True))
        )
        # 같은 이름 그림의 순서가 뜻을 가진다(`image[]` 첫째 = 화풍 기준) : 정렬 전 순서도 남긴다
        record["multipart_order"] = [p["name"] for p in parse_multipart(content_type, body)]
    elif content_type.startswith("application/json"):
        record["json"] = data_uris_to_pixels(json.loads(body))
    elif body:
        record["body"] = summarize(body)
    return record


class Recorder:
    """서버 한 개의 상태 : 기록·사례별 답"""

    def __init__(self) -> None:
        self.log: list[dict] = []
        self.script: dict = {}

    def reset(self) -> None:
        self.log = []
        self.script = {}


def serve_control(handler: BaseHTTPRequestHandler, recorder: Recorder, body: bytes) -> bool:
    """`/__log`·`/__reset`·`/__script` 처리. 제어 경로면 True"""
    path = urlsplit(handler.path).path
    if not path.startswith("/__"):
        return False
    if path == "/__log":
        payload = json.dumps(recorder.log, ensure_ascii=False).encode()
    elif path == "/__reset":
        recorder.reset()
        payload = b"{}"
    elif path == "/__script":
        recorder.script = json.loads(body or b"{}")
        payload = b"{}"
    else:
        payload = b"{}"
    handler.send_response(200)
    handler.send_header("Content-Type", "application/json")
    handler.send_header("Content-Length", str(len(payload)))
    handler.end_headers()
    handler.wfile.write(payload)
    return True


def read_body(handler: BaseHTTPRequestHandler) -> bytes:
    length = int(handler.headers.get("Content-Length") or 0)
    return handler.rfile.read(length) if length else b""


def run(handler_class: type, port: int) -> None:
    ThreadingHTTPServer(("127.0.0.1", port), handler_class).serve_forever()
