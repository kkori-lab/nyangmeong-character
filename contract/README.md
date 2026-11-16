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

- 핸들러 본문 전에서 끝나는 요청만 보낸다: 라우터(404·405·끝 `/` 307), 본문 읽기 오류, `X-Requested-With`, Java 인증(401·403 그대로 전달, 502), 값 검증(400).
- 핸들러까지 가는 요청은 M4에서 추가한다. 지금 러스트 핸들러는 501 `NOT_IMPLEMENTED`라 비교하면 다르게 나온다(비교가 차이를 잡는지 확인할 때 쓴다).
- 2026-11-16: 47/47 같음 (FastAPI 0.161.1, Starlette 1.9.0, pydantic 2.14.1)
