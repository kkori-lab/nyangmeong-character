# 대조 테스트 (Python ↔ 러스트)

같은 요청을 `nyangmeong-ai`(Python)와 이 저장소(러스트)에 보내 상태 코드·`Content-Type`·`Set-Cookie`·`Allow`·`Location`·본문(공백·키 순서까지, `timestamp`는 모양만)을 비교한다. Java는 `fake_java.py`가 대신한다.

## 돌리는 법 (터미널 3개 + 1개)

```sh
# 1. 가짜 Java
python -I contract/fake_java.py 18080

# 2. Python (nyangmeong-ai 폴더에서, .env.local 값을 덮어씀)
BACKEND_BASE_URL=http://127.0.0.1:18080 OPENAI_API_KEY= LOG_DIR= \
  .venv/Scripts/python.exe -m uvicorn app.main:app --port 18000 --log-level warning

# 3. 러스트 (이 폴더에서)
cargo build && BACKEND_BASE_URL=http://127.0.0.1:18080 PORT=18100 ./target/debug/nyangmeong-character

# 4. 비교 (다르면 종료 코드 1)
PYTHONIOENCODING=utf-8 python -I contract/compare.py
```

## 범위

- `CASES`: 핸들러 본문 전에서 끝나는 요청: 라우터(404·405·끝 `/` 307), 본문 읽기 오류, `X-Requested-With`, Java 인증(401·403 그대로 전달, 502), 값 검증(400).
- `SCENARIOS`: 핸들러까지 가는 요청(경로 8개). 사례마다 가짜 Java(`routes`로 경로별 답)·가짜 OpenAI의 답을 정하고, 두 서버가 가짜들에 보낸 요청까지 비교한다. 응답 뒤 작업이 있으면 `wait`의 요청이 기록되고 기록이 멈출 때까지 기다린다. 동시에 굽는 사례는 기록을 정렬해 비교한다(`unordered`).
- 가짜 서버의 연결 대기열은 100(기본 5면 전체 굽기의 동시 연결 13개가 넘쳐 거절된다).
- 2026-11-16: 119/119 같음 (원본 59ba84f, FastAPI 0.161.1, Starlette 1.9.0, pydantic 2.14.1)
