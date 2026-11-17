# LJS 핸드오프

## 지금 하는 것

M5 굽기 큐와 회원 경로 — 끝 (검증 2회차 판정 `확인 못 함 있음`: 2번 확인만 연결 남음, 사람 확인 2026-11-17). 다음은 M6(`/start M6`)
원본 커밋: 59ba84f (원본 저장소 develop. 로컬 작업 폴더는 LJS `b8e8d98`이라 `git show 59ba84f:<파일>`로 읽는다)

기준은 `app/character/service.py`·`router.py`·`repository.py`(59ba84f). 원본 테스트: `test_bake` 38·`test_create` 15·`test_create_without_generator` 3·`test_preview` 11·`test_equipped_image` 12·`test_regenerate` 14·`test_repository`(59ba84f에서 늘어난 사례). 서비스 수준 사례는 러스트 테스트로, 경로 수준 사례는 대조로 옮긴다.

1. Java 내부 API 추가분 — `src/repository.rs` ← `repository.py:regen_start·fetch_original_photo·report_success(original)·_photo_type`, `CharacterFailure.INPUT_ERROR`. `report_success`는 첫 생성 때 `photo` 파트(원래 파일 이름, 확장자로 `image/png`·`image/webp`·그 밖 `image/jpeg`)를 `image`·`sheet` 뒤에 붙인다. `original-photo`는 파일 15초
   - 명령: `tests/repository.rs`에 원본 `test_repository.py` e9c3edd→59ba84f 추가 사례(기대값 원본 그대로), 대조에서 가짜 Java가 받은 multipart 파트 순서·이름·Content-Type이 같음
2. 굽기 큐 — 새 `src/queue.rs` ← `service.py:BakeTask·BakeQueue(add·put_first·run·drain·_work·_notify)·get_bake_queue·_bake_one·_sheet_file_missing·_mark_bake_failed·_draw`, `BAKE_CONCURRENCY=13`·`BAKE_ATTEMPTS=2`·`BAKE_BATCH`. 상태(대기 deque·굽는 키 집합·일꾼 수·대기자 `oneshot` 목록)는 `std::sync::Mutex` 하나에 두고 `await` 동안 잡지 않는다(Python은 한 스레드라 "확인 후 꺼내기"가 끊기지 않음 → 러스트는 잠금 안에서 한 번에). 큐는 프로세스에 하나(`AppState`), 재시작하면 사라짐(원본과 같음). 큰 계산은 M4의 `heavy`(2개)를 그대로 거친다 → OpenAI 대기 13·계산 2가 따로 제한됨
   - 차이: 끊긴 요청의 대기자는 Python `future.done()` 확인 대신 `oneshot` 보내기 실패를 무시한다(결과 같음)
   - 명령: 새 `tests/bake_queue.rs`에 원본 `test_bake.py` 큐 사례(중복 넣기 무시·굽는 중 `put_first` 무시·`put_first`가 앞으로·동시 13 상한·같은 키 대기자가 결과 공유·한 작업 예외가 일꾼을 안 멈춤·시트 없을 때 조회·`FILE_NOT_FOUND` 404만 실패 표시·저장 실패는 다시 안 굽기·2회 시도) — 가짜 생성기·저장소는 `tests/service_admin.rs` 방식
3. 서비스 — `src/service.rs` ← `service.py:create_job·generate_and_report·_tasks·preview·save_equipped_image·_chosen_items·_render(시트 인자)·ItemBakeResult·bake_item_batch·_report_failure·regenerate_and_report`. 로그 문구 원본과 같게(M4처럼 traceback 대신 오류 한 줄)
   - 명령: 새 `tests/service_member.rs`에 원본 `test_create`·`test_create_without_generator`·`test_preview`·`test_equipped_image`·`test_bake`(묶음: `last` 계산·하나도 못 줄이면 502·중복 번호 제거·`BAKE_BATCH` 자르기)·`test_regenerate` 서비스 사례
4. 경로 — `src/character.rs`의 `/ai/characters`(202)·`/{n}/preview`(PNG 또는 409 `ITEM_BAKING`)·`/{n}/equipped-image`·`/ai/items/{n}/bake` 본문, 501 제거, `/ai/admin/characters/jobs/{n}/regenerate` 경로 추가(`require_admin`). `photo.read(MAX_IMAGE_BYTES + 1)`, 응답 모델 `CharacterCreateResponse`·`ItemBakeResponse`(`lastYn`) 키 순서 원본 그대로. 응답 뒤 작업(`generate_and_report`·`queue.drain`)은 `tokio::spawn`
   - 차이: Starlette `BackgroundTasks`는 응답 본문을 다 보낸 뒤 시작한다. 러스트는 응답을 돌려주기 직전에 띄워 Java 호출이 202보다 먼저 갈 수 있다. 응답·Java가 받는 요청은 같다(React는 상태를 Java에서 조회). `migration.md` "알려진 차이"에 적는다
   - 명령: 대조에서 새 경로 사례(성공·생성기 없음 502·업로드 오류·Java 4xx 전달·규격 밖 실패 보고·OpenAI 실패 보고·미리보기 409 뒤 굽기·굽기 실패 아이템 502·한 부위 두 아이템 400·전체 굽기 묶음 `lastYn`)의 상태 코드·본문·PNG 화소와 가짜 Java·OpenAI가 받은 요청이 같음
5. 대조 준비 — `contract/fake_java.py`: 작업 생성·`success`·`failure`·`regen-start`·`original-photo`·`bake-items`·`bake-targets`·시트 저장·굽기 실패·`equipped-image`에 사례별 답(`/__script`의 경로 패턴별 단계), `contract/compare.py`: 응답 뒤 작업이 끝날 때까지 기록을 기다림(사례마다 "마지막 요청" 경로가 기록될 때까지, 제한 30초). 전체 굽기는 13개가 동시에 돌아 요청 순서가 매번 달라 키(`pixelCharSn`·`itemSn`)로 정렬해 비교
6. 2번 확인 — 화면: 사람이 2번에서 `curl -H 'X-Requested-With: XMLHttpRequest' -H 'Cookie: access_token=<회원>' -F petSn=<n> -F photo=@<사진> http://127.0.0.1:8100/ai/characters` → 202, `journalctl -u nyangmeong-character`에 "전체 굽기 시간" 줄, 그동안 `vmstat 1`의 `si`·`so` 0, `ps -o rss -C nyangmeong-character` 최대값. 시크릿 `ENV_FILE`(`BACKEND_BASE_URL`·`OPENAI_API_KEY`)이 들어간 뒤에만 가능 → 안 되어 있으면 "다음에 할 것"에 남긴다
7. 문서 — `migration.md` "M5에서 옮긴 것"·알려진 차이(4의 시작 시점), `contract/README.md` 범위·사례 수, `milestones/LJS.md` M5 기능 줄(경로 5개)

질문:
1. 관리자 무료 재생성(59ba84f, PR #84 `/ai/admin/characters/jobs/{n}/regenerate`)을 M5에 넣는가 — (가) 넣는다(추천: 1의 저장소 추가분과 `generate_and_report`를 그대로 같이 쓰고, 빼면 M6 전환 때 Python을 지우는 순간 이 경로가 사라짐 [나중에 깨짐]) (나) M5는 원래 4개만, 재생성은 M5 뒤 별도 마일스톤
결정: (가) 재생성 경로를 M5에 넣는다
경로: 바로 수정이 아니라 큰 변경(러스트 파일 4개 이상·큐는 기존 테스트 없음). `.sap/config`가 없어 한 세션(`/impl` 검증 루프)으로 한다
새 의존성: 없음(`tokio::sync::oneshot`은 이미 켠 `sync` 기능, 상태 잠금은 `std::sync::Mutex`)

### 추가 요청

| 추가-1 | `contract/compare.py` | 명령: `sh scripts/check.sh` 대조 119/119(시트 파일 두 개를 동시에 받는 사례 4개를 순서 없이 비교) | 요청 2026-11-16 22:40 "잠금 풀고 unordered 붙이는 것 승인" |


## 다음에 할 것

- 연결 남음: M5 2번 확인 (시크릿 `ENV_FILE`에 `BACKEND_BASE_URL`·새 `OPENAI_API_KEY`를 넣고 다시 배포한 뒤, 2번에 캐릭터 생성 1회 → "전체 굽기 시간" 로그·`vmstat` `si`/`so` 0·`ps -o rss` 최대값, LJS)
- OpenAI 키 교체: 2026-11-17 확인 중 키가 대화 기록에 노출됐다. 대시보드에서 폐기·새로 발급하고 `.env.local`·`ENV_FILE`을 바꾼다(LJS). 크레딧은 충전함(소진 시 OpenAI 429 `insufficient_quota`, 로그에는 `RateLimitError`만 남음)
- 결정 대기: 크레딧 부족 알림 — (가) `src/clients.rs`에서 429 본문 `code`를 읽어 운영 로그에만 남김(추천, 응답·Java 보고 그대로) (나) 실패 사유를 새 값으로 Java에 보고(Python과 결과가 달라져 M6 뒤). 회원 화면에는 노출하지 않는다
- 2번 서버 최대 메모리: Actions `build`의 `image_bench` 산출물을 2번에 올려 `chmod +x image_bench && /usr/bin/time -v ./image_bench <시트 폴더>`의 `Maximum resident set size`를 `migration.md` M3 측정 표에 적는다(RAM 1GB 안에 여유). 서버에서 cargo build 안 함
- 쓰고 난 실제 시트 폴더와 `target/character-gold`는 실제 반려동물 그림이라 지운다
