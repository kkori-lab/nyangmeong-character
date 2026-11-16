# LJS 마일스톤

기준: `nyangmeong-ai` develop `app/character/`·`app/core/` (2026-11-16). 배경·결정은 `docs/design/migration.md`.

## M0 기준 측정 (1번 캐릭터 생성·아이템 등록) — 끝
- 기능: `ops/m0.sh character` (선택: `item-image`·`item-bake`)
- 완료 조건: 1번에서 캐릭터 1마리 생성 동안 vmstat·CPU 초·RSS·py-spy 숫자
- 결과: `docs/design/migration.md` "왜"

## M1 네트워크 (1번↔2번, 2번→OpenAI, `/health`) — 끝
- 기능: 보안 목록·1번 firewalld·2번 iptables, Actions 첫 배포
- 완료 조건: 1번→2번 `/health` 200, 외부→2번 8100 시간 초과, 2번→1번 `/internal/auth/check` 401, 2번→OpenAI 응답

## M2 공통 계약 (ApiResponse·XRW·Java 인증·업로드 검증) — 끝
- 기능: `src/response.rs`·`error.rs`·`backend.rs`·`auth.rs`·`image.rs`·`form.rs`·`pyval.rs`·`character.rs`·`app.rs`
- 원본: `app/core/response.py`·`errors.py`·`backend.py`·`auth.py`·`image.py`, `app/character/router.py`
- 완료 조건: 명령: `sh scripts/check.sh`(대조 47/47 같음)
- 연결 남음: 경로 7개 핸들러 본문 (M4, LJS)

## M3 이미지 계산 (stand_front_cut·place·bake_source·validate·render·cut_item) — 대기
- 기능: `engine.stand_front_cut`(`_load`·`_fill_holes`·`_defringe`·`_crop`)·`stand_front`, `canvas.place`·`find_eyes`, `bake.bake_source`·`validate`·`layer_base`·`worn_sprite`·`item_layer`, `compose.render`, `engine.cut_item`·`normalize_photo`·`normalize_item_image`
- 원본: `app/character/engine.py`·`canvas.py`·`bake.py`·`compose.py`
- 기다리는 것: 정답 파일(M0 측정용 캐릭터의 맨몸 시트·착용 시트, Java에서 받기. 커밋하지 않음)
- 완료 조건: 명령: `cargo test`에서 Python 결과 PNG와 픽셀 단위 같음. 실행 시간·최대 메모리 비교 숫자

## M4 외부 연결과 큐 (OpenAI·Java 내부 API·굽기 큐·경로 7개) — 대기
- 기능: OpenAI 클라이언트(rustls 추가), Java 내부 API 클라이언트, 굽기 큐(OpenAI 대기·큰 계산 동시 수 따로 제한, 미리보기 우선, 재시도), 경로 7개 핸들러 본문
- 원본: `app/character/clients.py`·`repository.py`·`service.py`·`router.py`
- 기다리는 것: 시크릿 `ENV_FILE`(`BACKEND_BASE_URL`·`OPENAI_API_KEY`, LJS)
- 완료 조건: 명령: `sh scripts/check.sh`(핸들러까지 가는 대조 추가). 화면: 2번에 직접 요청해 캐릭터 1마리 생성 → 전체 굽기 끝, 2번 swap·메모리 숫자

## M5 전환 (nginx 분기, Python app/character/ 제거) — 대기
- 기능: 1번 nginx `location /ai/characters`·`/ai/items/` → 2번 8100, `nyangmeong-ai`에서 `app/character/` 제거(그 저장소 PR)
- 기다리는 것: 백엔드·프론트 팀원 공지
- 완료 조건: 화면: 사이트에서 캐릭터 생성·아이템 등록·전체 굽기 정상, 1번 `ops/m0.sh character`에서 스왑 없음
