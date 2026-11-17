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

## M3 이미지 계산 (stand_front_cut·place·bake_source·validate·render·cut_item) — 끝
- 기능: `engine.stand_front_cut`(`_load`·`_fill_holes`·`_defringe`·`_crop`)·`stand_front`, `canvas.place`·`find_eyes`, `bake.bake_source`·`validate`·`layer_base`·`worn_sprite`·`item_layer`, `compose.render`, `engine.cut_item`·`normalize_photo`·`normalize_item_image`
- 원본: `app/character/engine.py`·`canvas.py`·`bake.py`·`compose.py`
- 완료 조건: 명령: `cargo test`에서 Python 결과 PNG와 픽셀 단위 같음. 실행 시간·최대 메모리 비교 숫자
- 결과: 합성 시트·아이템·사진·scipy 정답과 이 PC의 실제 시트 비교가 모두 같음. 측정 표는 `docs/design/migration.md` "M3에서 옮긴 것"
- 연결 남음: 2번 서버 최대 메모리(`Maximum resident set size`) 측정 (Actions `build`의 `image_bench` 산출물, LJS)

## M4 외부 연결 (프롬프트·OpenAI·Java 내부 API·관리자 경로 3개) — 끝
- 기능: 프롬프트·눈 판독, OpenAI 클라이언트(rustls 추가), Java 내부 API 클라이언트, 관리자 경로 3개(bake-hints·pixel-image·trial-preview) 핸들러 본문, 대조용 가짜 Java·OpenAI
- 원본: `app/character/engine.py`(프롬프트)·`clients.py`·`repository.py`·`service.py`·`router.py`
- 완료 조건: 명령: `sh scripts/check.sh`(핸들러까지 가는 대조 추가)

## M5 굽기 큐와 회원 경로 (굽기 큐·회원 경로 4개·2번 확인) — 끝
- 기능: 굽기 큐(OpenAI 대기 13·큰 계산 동시 수 2 따로 제한, 미리보기 우선, 재시도), 경로 5개 `/ai/characters`·preview·equipped-image·`/ai/items/{n}/bake`·`/ai/admin/characters/jobs/{n}/regenerate`(59ba84f 무료 재생성)
- 원본: `app/character/service.py`·`router.py`·`repository.py`(59ba84f 추가분)
- 기다리는 것: 시크릿 `ENV_FILE`(`BACKEND_BASE_URL`·`OPENAI_API_KEY`, LJS)
- 완료 조건: 명령: `sh scripts/check.sh`(큐·회원 경로 대조 추가). 화면: 2번에 직접 요청해 캐릭터 1마리 생성 → 전체 굽기 끝, 2번 swap·메모리 숫자
- 결과: 대조 119/119, 로컬 실제 Java·OpenAI로 생성·미리보기 사람 확인(2026-11-17)
- 2번 확인(2026-11-17): 캐릭터 623 생성 → 전체 굽기 101.3초(36개 중 33개 성공), 굽기 뒤 RSS 57MB·available 510MB·재시작 없음. 결과는 `migration.md` "M6 전환 절차" 아래

## M6 전환 (nginx 분기, Python 코드는 남김) — 끝
- 기능: 1번 nginx `location /ai/characters`·`/ai/admin/characters/`·`/ai/items/` → 2번 8100(사람이 1번에서 직접), Python `app/character/`는 남김(2026-11-17 결정)
- 기다리는 것: 백엔드·프론트 팀원 공지
- 완료 조건: 화면: 사이트에서 캐릭터 생성·아이템 등록·전체 굽기 정상, 1번 `ops/m0.sh character`에서 스왑 없음

- 결과: 2026-11-17 12:02 KST nginx 전환, 사이트 생성·굽기·미리보기 2번에서 동작 확인
- 연결 남음: 굽기 중 최대 메모리 `systemctl show nyangmeong-character -p MemoryPeak`·OOM 없음(2번, LJS)
- 연결 남음: 아이템 49·51·53 그림 참고 굽기 `BackendError`(백엔드 `bake-image`·아이템 그림 확인, 백엔드에서 /start)
- 연결 남음: 관리자 아이템 등록·전체 굽기 사이트 확인, 1번 `ops/m0.sh character` 스왑 없음(LJS)
- 연결 남음: 팀원 공지(LJS)
## M7 로그 파일 (LOG_DIR 날짜별 파일·30일 보관) — 대기
- 기능: `core/logs.py`의 `DailyFileHandler`(`LOG_DIR/<접두사>YYYY-MM-DD.log` 이어 쓰기, 파일 형식 `시각.밀리초 [pid] 레벨 로거 - 메시지`, 파일 0660·폴더 2770)·`remove_old_files`(`KEEP_DAYS = 30`, 이름이 날짜가 아니면 둠)·`_open_folder`·`configure(log_dir)`(`LOG_DIR`이 비면 콘솔만, 파일에는 `app.*` INFO 이상과 서버 기동·오류), `config.py` `log_dir`. 2번 systemd에 로그 폴더(`deploy/nyangmeong-character.service`, `ProtectHome=true` 유지)
- 원본: `nyangmeong-ai` `app/core/logs.py`·`app/core/config.py`, 백엔드 `logback-spring.xml`(날짜별·30일, 맞춘 기준)
- 기다리는 것: 시크릿 `ENV_FILE`에 `LOG_DIR` 추가(LJS), 파일 접두사(`ai_`를 그대로 쓸지 `character_`로 할지는 기획 때 질문)
- 완료 조건: 명령: 원본 `tests/core/test_logs.py` 사례를 러스트 테스트로(날짜 바뀜·오래된 파일 지움·형식), `sh scripts/check.sh` / 화면: 2번에서 `ls -l <LOG_DIR>`에 오늘 날짜 파일, 요청 한 번 뒤 `tail` 에 `app.request` 줄. 2번 시계는 UTC라 파일 날짜 기준(원본 `date.today`는 서버 지역 시각)을 확인
