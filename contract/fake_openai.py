"""대조 테스트용 가짜 OpenAI : 이미지 편집(`/v1/images/edits`)·비전 채팅(`/v1/chat/completions`)

받은 요청은 `GET /__log`, 사례 설정은 `POST /__script`, 초기화는 `POST /__reset`(contract/fakelib.py).
답은 사례가 `{"images": [...], "chat": [...]}`로 순서대로 정해 준다. 다 쓰면 500.
  images 항목: {"status": 200, "file_b64": "<답으로 줄 PNG>"} 또는 {"status": 500, "headers": {...}, "delay": 초}
  chat 항목:   {"status": 200, "content": "<답 글>"} 또는 {"status": 500, ...}

사용: python contract/fake_openai.py [포트=18090]
"""

import json
import sys
import time
from http.server import BaseHTTPRequestHandler
from pathlib import Path
from urllib.parse import urlsplit

sys.path.insert(0, str(Path(__file__).resolve().parent))

import fakelib  # noqa: E402

RECORDER = fakelib.Recorder()


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args: object) -> None:
        pass

    def do_GET(self) -> None:
        self.dispatch()

    def do_POST(self) -> None:
        self.dispatch()

    def dispatch(self) -> None:
        body = fakelib.read_body(self)
        if fakelib.serve_control(self, RECORDER, body):
            return
        RECORDER.log.append(fakelib.record_of(self, body))
        path = urlsplit(self.path).path
        kind = "images" if path.endswith("/images/edits") else "chat" if path.endswith("/chat/completions") else None
        queue = RECORDER.script.get(kind, []) if kind else []
        step = queue.pop(0) if queue else {"status": 500, "body": {"error": {"message": "스크립트 없음"}}}
        if step.get("delay"):
            time.sleep(step["delay"])
        status = step.get("status", 200)
        if status == 200 and kind == "images":
            payload = {"created": 1, "data": [{"b64_json": step["file_b64"]}]}
        elif status == 200 and kind == "chat":
            payload = {"id": "x", "object": "chat.completion", "choices": [{"index": 0, "finish_reason": "stop", "message": {"role": "assistant", "content": step.get("content")}}]}
        else:
            payload = step.get("body", {"error": {"message": "오류"}})
        data = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        for name, value in step.get("headers", {}).items():
            self.send_header(name, value)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


if __name__ == "__main__":
    fakelib.run(Handler, int(sys.argv[1]) if len(sys.argv) > 1 else 18090)
