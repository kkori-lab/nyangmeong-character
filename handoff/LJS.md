# LJS 핸드오프

## 지금 하는 것

M4 외부 연결 [A] — 구현 중 (승인 2026-11-16 14:38, [B]는 새 M5)
원본 커밋: e9c3edd (대조 테스트는 그 뒤 develop 59ba84f까지 같음을 확인)

(M3 요약·측정 표는 `docs/design/migration.md` "M3에서 옮긴 것"으로 옮김.) 아래 [A]·[B]는 M4를 둘로 나눈 것이다(이번엔 [A]만).

1. 프롬프트 [A] — `src/engine.rs`, 새 `prompts/`(원본 `app/character/prompts/` 6개 그대로 복사, 원본 저장소도 공개, `include_str!`·`include_bytes!`) ← `engine.py:species_of·eye_question·parse_eyes·_pick·_eye_text·_registered_text·build_prompt·build_wear_prompt·item_hint_question·parse_bake_hint·item_pixel_prompt`, `BakeItem`·`BAKE_REGIONS`·`BAKE_HINT_MAX`. Python `str.format`(`{{`), `json.dumps(ensure_ascii=False)`, `str.strip`·`splitlines`(유니코드 공백·줄 구분 문자), `len`(코드 포인트)을 따른다. `\{.*\}`(DOTALL)는 정규식 없이 첫 `{`~마지막 `}`
   - 명령: 원본 venv로 만든 정답(`contract/gold_prompts.py` → `tests/fixtures/prompts.json`)과 문자열이 같음: 종 2 × 눈 모양 4 × 색 8(없음 포함) × 등록 정보(없음·중괄호·따옴표·이모지·줄바꿈), 부위 6(미등록 포함) × 그림 참고 여부, `parse_eyes`·`parse_bake_hint` 입력 표(빈 답·코드 펜스·JSON 아님·` `·150자 경계)
2. OpenAI 클라이언트 [A] — 새 `src/clients.rs` ← `clients.py:OpenAICharacterClient`(`render`·`_detect_eyes`·`_draw_sheet`·`_bake`·`_describe_item`·`_draw_item`)·`get_character_client`. reqwest로 `POST /images/edits`(multipart `model`·`prompt`·`image[]` 순서대로·`size`·`quality`·`n`)와 `POST /chat/completions`. SDK와 같게: 제한 시간 180초, `max_retries=1`(연결 오류·시간 초과·408·409·429·5xx만 한 번, `retry-after` 또는 0.5초 지터 대기), 눈 판별 실패는 기본값, 그 밖 실패는 `CharacterApiError`. 키가 비면 생성기 없음
   - 차이: 주소 `OPENAI_BASE_URL`(기본 `https://api.openai.com/v1`). Python SDK도 같은 환경변수를 읽어 대조에서 둘 다 가짜로 돌린다. `Settings`·`.env.local.example`에 추가
   - 명령: 대조에서 가짜 OpenAI가 받은 요청(경로·필드·프롬프트·그림 화소·data URI 화소)이 Python과 같음, 500 → 성공(재시도 1회)·400(재시도 없음)·시간 초과 사례
3. Java 내부 API [A] — 새 `src/repository.rs` ← `repository.py:JavaCharacterJobRepository` 10개 메서드·`_fetch_sheet`와 응답 모델 6개. 쿠키·`X-Requested-With` 전달, 파일 15초, 쿼리 `itemSns` 반복(빈 목록이면 안 붙임)·`page=0`, multipart 파일 이름·`image/png`, 폼 `itemSns` 반복, 배치값은 Python `str()` 모양(`pyjson::float_repr`, `place`가 정수 100을 돌려줄 때 `"100"`), `list_bake_items` 중복 번호(첫 자리·마지막 값)·지시와 그림이 다 없는 행 제외, 모양이 틀리면 502. pydantic lax `float`·`bool`·`Literal`·`min_length`는 원본 venv로 확인한 표로 `src/pyval.rs`에 추가
   - 명령: 원본 `tests/character/test_repository.py` 18개 사례를 `tests/repository.rs`로 옮김(기대값 원본 그대로), 대조에서 가짜 Java가 받은 요청이 같음
4. 굽기 큐 [B] — 새 `src/service.rs` ← `service.py:BakeQueue`(`add`·`put_first`·`run`·`drain`·`_work`·`_notify`)·`_bake_one`·`_sheet_file_missing`·`_mark_bake_failed`·`_draw`·`_bake_once`. `BAKE_CONCURRENCY = 13`(OpenAI 대기 일꾼), `BAKE_ATTEMPTS = 2`, 같은 (캐릭터, 아이템) 두 번 안 굽기, 미리보기 앞에 넣기, 저장 실패는 다시 안 굽기
   - 차이: 큰 계산(`normalize_*`·`stand_front`·`place`·`bake_source`·`validate`·`compose.render`·`cut_item`)은 `spawn_blocking` + 세마포어 `HEAVY_CONCURRENCY`(값 2로 정함). 원본은 `asyncio.to_thread` 기본 풀(min(32, CPU+4))이 OpenAI 대기와 큰 계산을 같이 묶는다. 큐 상태는 `Mutex` 하나로 확인·변경을 한 번에(원본은 이벤트 루프 한 스레드). 결과는 같고 동시 수만 다름
   - 명령: 원본 `test_bake.py`(38)·`test_items.py`(32)의 큐·묶음 사례를 `tests/bake_queue.rs`로 옮김(가짜 client·repo), 큰 계산 동시 실행 최대 수 테스트
5. 경로 7개 핸들러 [A: bake-hints·pixel-image·trial-preview / B: 나머지 4개] — `src/character.rs`의 501을 `router.py` 핸들러와 `service.py:create_job·generate_and_report·_tasks·preview·save_equipped_image·_chosen_items·_render·describe_item·draw_item·trial_preview·bake_item_batch·_check_region·_item_image_error·_preview_error·_report_failure`, `rules.py:png_size·matches_spec`으로 바꿈. 응답 뒤 작업(`BackgroundTasks`)은 응답을 만든 뒤 `tokio::spawn`. 로그 문구는 원본과 같게(`logger.exception` traceback 대신 오류 한 줄)
   - 명령: 대조에 핸들러까지 가는 사례 추가: 생성(202·생성기 없음 502·눈 못 찾음 SPEC_MISMATCH·OpenAI 실패 API_ERROR·Java 4xx 전달) 뒤 가짜 Java가 받은 보고·굽기 요청, 미리보기(합성 PNG·409 ITEM_BAKING·굽기 실패 502·없는 아이템·부위 중복 400), 착용 이미지(409 EQUIPPED_ITEMS_CHANGED 전달), 관리자 3개, 전체 굽기 묶음(`bakedCnt`·`failedCnt`·`lastYn`). PNG는 화소로 비교(바이트는 알려진 차이). 원본 `test_create*`·`test_preview`·`test_equipped_image`·`test_item_pixel`·`test_regenerate`의 경로 동작 사례를 대조 사례로 옮김
   - 화면: `cargo run` + 가짜 Java·가짜 OpenAI로 curl 생성 → 202 → 가짜 Java 기록에 성공 보고와 아이템 시트 저장
6. 대조 준비 [A] — `contract/fake_java.py`에 캐릭터 내부 API(메모리 상태, 받은 요청 기록·조회·초기화), 새 `contract/fake_openai.py`(합성 시트 `tests/fixtures/image/` 돌려줌, 사례별 실패·답 지정, 기록). `run.py`가 Python용·러스트용 가짜를 따로 띄워 기록을 비교, 순서가 정해지지 않는 굽기 요청은 정렬해 비교. 실제 OpenAI는 안 부름
7. 2번 확인 [B, 사람] — 화면: 시크릿 `ENV_FILE`에 `BACKEND_BASE_URL`(1번 Java 사설 주소)·`OPENAI_API_KEY` 넣고 배포 → 1번에서 사이트 로그인 쿠키로 `curl -s -X POST http://<2번 사설 IP>:8100/ai/characters -H 'X-Requested-With: XMLHttpRequest' -H 'Cookie: <브라우저 쿠키>' -F photo=@pet.png -F petSn=<번호>` → 202 `charCrtJobSn`, `journalctl -u nyangmeong-character -f`에 `전체 굽기 시간`, 그동안 2번 `vmstat 1`의 `si`·`so` 0, `ps -o rss= -C nyangmeong-character` 최대값을 `migration.md`에 적음. 사이트 경로는 아직 1번 Python(전환은 M6)
8. 문서 [B] — `docs/design/migration.md` "M4에서 옮긴 것". 결과가 달라지는 것이 생기면 "알려진 차이"

질문:
결정: 1. HEAVY_CONCURRENCY=2 2. 나눔 — 이번 M4는 [A]만(항목 1·2·3·6, 5의 관리자 3개). [B](항목 4·7, 5의 회원 4개)는 M5, 8은 M5에서 [B]와 함께. 항목 4의 큐는 M5에서 만든다(trial_preview가 쓰는 `_bake_once`는 [A]에서 service.rs에 둠)

새 의존성: `reqwest` 기능 `rustls`(OpenAI HTTPS, aws-lc-rs·플랫폼 인증서. PC·러너 빌드가 안 되면 `rustls-no-provider` + ring), `base64` 0.22(`b64_json`·data URI)

## 다음에 할 것

- M5 시작 전 확인: develop이 59ba84f로 움직였다(PR #84 `SCR-MYP-010 관리자 무료 재생성`): 8번째 경로 `/ai/admin/characters/jobs/{n}/regenerate`, `report_success`의 `photo` 파트, 실패 사유 `CHARACTER_INPUT_ERROR`, Java `regen-start`·`original-photo`. 7개 경로 계획에 없던 것이라 M5 `/start`에서 옮길 범위에 넣을지 정한다(`/start`는 새 원본 커밋으로 다시 읽는다)

- 2번 서버 최대 메모리: Actions `build`의 `image_bench` 산출물을 2번에 올려 `chmod +x image_bench && /usr/bin/time -v ./image_bench <시트 폴더>`의 `Maximum resident set size`를 `migration.md` M3 측정 표에 적는다(RAM 1GB 안에 여유). 서버에서 cargo build 안 함
- M4 2번 확인 전에 시크릿 `ENV_FILE`에 `BACKEND_BASE_URL`·`OPENAI_API_KEY`를 넣고 다시 배포한다
- 쓰고 난 실제 시트 폴더와 `target/character-gold`는 실제 반려동물 그림이라 지운다
