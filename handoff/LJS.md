# LJS 핸드오프

## 지금 하는 것

M4 외부 연결 [A] — 끝 (검증 통과 2회차, 2026-11-16). 다음은 M5(`/start M5`)
원본 커밋: 59ba84f (e9c3edd 뒤 바뀐 것은 `repository.py`·`router.py`·`service.py`의 관리자 무료 재생성뿐이고 회원·재생성 경로에서만 쓴다 → M5. `engine.py`·`clients.py`는 그대로)

검증 루프 2회차까지 쓰던 기획을 다시 세운다. 1~5는 e302162에서 구현됐고 기준은 그대로다. 6·7이 2회차에서 지적된 원본과의 차이를 다룬다. [B](굽기 큐·회원 경로 4개·2번 확인·M4 문서)는 milestones M5로 옮겼다(`_bake_once`는 trial-preview가 써서 [A]의 `service.rs`에 있다).

1. 프롬프트 — `src/engine.rs`, `prompts/`(원본 `app/character/prompts/` 6개, `include_str!`·`include_bytes!`) ← `engine.py:species_of·eye_question·parse_eyes·_pick·_eye_text·_registered_text·build_prompt·build_wear_prompt·item_hint_question·parse_bake_hint·item_pixel_prompt`, `BakeItem`·`BAKE_REGIONS`·`BAKE_HINT_MAX`. Python `str.format`(`{{`), `json.dumps(ensure_ascii=False)`, `str.strip`·`splitlines`(유니코드 공백·줄 구분 문자), `len`(코드 포인트)을 따른다. `\{.*\}`(DOTALL)는 정규식 없이 첫 `{`~마지막 `}`
   - 명령: 원본 venv로 만든 정답(`contract/gold_prompts.py` → `tests/fixtures/prompts.json`)과 문자열이 같음: 종 2 × 눈 모양 4 × 색 8(없음 포함) × 등록 정보(없음·중괄호·따옴표·이모지·줄바꿈), 부위 6(미등록 포함) × 그림 참고 여부, `parse_eyes`·`parse_bake_hint` 입력 표(빈 답·코드 펜스·JSON 아님·` `·150자 경계)
2. OpenAI 클라이언트 — `src/clients.rs` ← `clients.py:OpenAICharacterClient`(`render`·`_detect_eyes`·`_draw_sheet`·`_bake`·`_describe_item`·`_draw_item`)·`get_character_client`. reqwest로 `POST /images/edits`(multipart `model`·`prompt`·`image[]` 순서대로·`size`·`quality`·`n`)와 `POST /chat/completions`. SDK와 같게: 제한 시간 180초, `max_retries=1`(연결 오류·시간 초과·408·409·429·5xx만 한 번, `retry-after` 또는 0.5초 지터 대기), 눈 판별 실패는 기본값, 그 밖 실패는 `CharacterApiError`. 키가 비면 생성기 없음
   - 차이: 주소 `OPENAI_BASE_URL`(기본 `https://api.openai.com/v1`). Python SDK도 같은 환경변수를 읽어 대조에서 둘 다 가짜로 돌린다. `Settings`·`.env.local.example`에 있음
   - 명령: 대조에서 가짜 OpenAI가 받은 요청(경로·필드·프롬프트·그림 화소·data URI 화소)이 Python과 같음, 500 → 성공(재시도 1회)·400(재시도 없음)·시간 초과 사례
3. Java 내부 API — `src/repository.rs` ← `repository.py:JavaCharacterJobRepository` e9c3edd의 10개 메서드·`_fetch_sheet`와 응답 모델 6개(59ba84f의 `regen_start`·`fetch_original_photo`·`report_success`의 `original`·`INPUT_ERROR`는 M5). 쿠키·`X-Requested-With` 전달, 파일 15초, 쿼리 `itemSns` 반복(빈 목록이면 안 붙임)·`page=0`, multipart 파일 이름·`image/png`, 폼 `itemSns` 반복, 배치값은 Python `str()` 모양(`pyjson::float_repr`, `place`가 정수 100을 돌려줄 때 `"100"`), `list_bake_items` 중복 번호(첫 자리·마지막 값)·지시와 그림이 다 없는 행 제외, 모양이 틀리면 502. pydantic lax `float`·`bool`·`Literal`·`min_length`는 `src/pyval.rs`
   - 명령: 원본 `tests/character/test_repository.py` 18개 사례를 옮긴 `tests/repository.rs`(기대값 원본 그대로), 대조에서 가짜 Java가 받은 요청이 같음
4. 관리자 경로 3개 — `src/character.rs`의 `/ai/items/bake-hints`·`/ai/items/pixel-image`·`/ai/items/trial-preview` ← `router.py` 핸들러, `service.py:describe_item·draw_item·trial_preview·_bake_once·_check_region·_item_image_error·_preview_error`, `rules.py:png_size·matches_spec`. 나머지 4개 경로는 501 그대로(M5). 로그 문구는 원본과 같게(`logger.exception` traceback 대신 오류 한 줄)
   - 명령: 대조에서 관리자 3개 경로 사례(성공·OpenAI 실패 502·아이템 못 찾음·부위 확인·업로드 오류)의 상태 코드·본문·PNG 화소가 Python과 같음, 원본 `test_item_pixel` 등의 경로 동작 사례
5. 대조 준비 — `contract/fake_java.py` 캐릭터 내부 API(메모리 상태, 받은 요청 기록·조회·초기화), `contract/fake_openai.py`(합성 시트 `tests/fixtures/image/`, 사례별 실패·답 지정, 기록). `run.py`가 Python용·러스트용 가짜를 따로 띄워 기록 비교. 실제 OpenAI는 안 부름
6. 눈 판독 JSON 숫자 — `src/pyjson.rs:loads_str_lenient`(`parse_eyes`만 씀) ← Python 3.13 `json.loads` 숫자 처리. 원본 venv로 확인함: `1e400` → `inf`로 읽고 나머지 칸을 씀(러스트는 지금 serde가 거절해 기본 눈값), 4301자리 정수 → `int` 자릿수 제한 `ValueError`로 기본 눈값(러스트는 지금 받음), 4300자리 정수·긴 소수는 받음. 문자열 밖 숫자 토큰을 Python 문법(`-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][-+]?[0-9]+)?`)으로 읽어 f64가 넘치면 `null`로 바꾸고(값은 안 씀), 소수·지수 없는 정수가 4300자리를 넘으면 `None`
   - 명령: `contract/gold_prompts.py` `parse_eyes` 입력 표에 `1e400`·`-1e400`·`1e-400`·`1E+400`·4300/4301자리 정수(음수 포함)·4301자리 소수·문자열 안 `1e400`·`01` 추가 → 정답 다시 만들고 같음
   - 차이(유지): 깊은 중첩. Python은 C 재귀 한도까지 받음(이 PC venv에서 2998, 한도는 플랫폼마다 다름), serde는 128. 깊이를 맞추면 스택 넘침 위험이 있어 맞추지 않는다. `migration.md` 95줄에서 `1e400`을 빼고 측정값을 적는다
7. 제한 시간 기록 바로잡기 — reqwest 0.13.5 `read_timeout`은 요청을 보낼 때 한 번 맞춘 타이머다(`PendingRequest::poll`, 응답 머리가 올 때까지, 업로드 시간 포함). 그래서 업로드가 멈추면 시간 초과가 나고, SDK처럼 재시도한 뒤 실패한다. `migration.md` 94줄("OpenAI 쓰기 제한 시간 없음… 계속 기다린다")과 105줄("쓰기 제한 시간 없음")은 틀린 기록이다. 테스트로 확인한 뒤 실제 차이로 고친다: "업로드와 응답 대기를 합쳐 180초(Java는 3·15초)를 넘으면 시간 초과. httpx는 읽기·쓰기 한 번마다 잰다"
   - 명령: 새 통합 테스트(`tests/timeouts.rs`). 받기를 멈추는 TCP 서버에 큰 본문을 보내고 제한을 1초로 두면 `OpenAiClient`는 시간 초과로 재시도 1회 뒤 `SdkError`, 연결 2번, 몇 초 안에 끝남. `backend.rs`도 같은 방식으로 1회 확인한다(재시도 없음). Python 쪽 기대(httpx `WriteTimeout` → `APITimeoutError` → 재시도)는 SDK 소스로 확인해 테스트 주석에 적는다

질문: 7의 처리 — (가) 동작은 그대로 두고 기록만 바로잡는다(추천: 업로드는 몇 MB라 합계 차이가 날 일이 거의 없음) (나) httpx처럼 읽기·쓰기 한 번마다 재도록 맞춘다(본문 스트림 감시와 머리 대기 타이머 재설정, `clients.rs`·`backend.rs`에 수십 줄)
결정: HEAVY_CONCURRENCY=2. M4는 [A]만, [B]는 M5. 7은 (가) 동작은 그대로 두고 기록만 바로잡는다

새 의존성: 없음(이미 넣은 `reqwest` `rustls`·`base64` 0.22 그대로)

## 다음에 할 것

- M5 시작 전 확인: develop이 59ba84f로 움직였다(PR #84 `SCR-MYP-010 관리자 무료 재생성`): 8번째 경로 `/ai/admin/characters/jobs/{n}/regenerate`, `report_success`의 `photo` 파트, 실패 사유 `CHARACTER_INPUT_ERROR`, Java `regen-start`·`original-photo`. 7개 경로 계획에 없던 것이라 M5 `/start`에서 옮길 범위에 넣을지 정한다(`/start`는 새 원본 커밋으로 다시 읽는다)

- 2번 서버 최대 메모리: Actions `build`의 `image_bench` 산출물을 2번에 올려 `chmod +x image_bench && /usr/bin/time -v ./image_bench <시트 폴더>`의 `Maximum resident set size`를 `migration.md` M3 측정 표에 적는다(RAM 1GB 안에 여유). 서버에서 cargo build 안 함
- M4 2번 확인 전에 시크릿 `ENV_FILE`에 `BACKEND_BASE_URL`·`OPENAI_API_KEY`를 넣고 다시 배포한다
- 쓰고 난 실제 시트 폴더와 `target/character-gold`는 실제 반려동물 그림이라 지운다
