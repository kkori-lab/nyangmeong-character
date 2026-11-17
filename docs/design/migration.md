# 캐릭터 서버 이전 설계

`nyangmeong-ai`(Python) `app/character/`를 2번 인스턴스의 러스트 서버로 옮기는 배경·결정·서버 정보·지뢰를 모은다. 진행 기록은 `handoff/<이름>.md`, 단계는 `milestones/<이름>.md`에 있다.

공개 저장소다. 서버 IP·포트 외 접속 정보·키·개인 PC 경로를 적지 않는다.

## 왜

- 1번 인스턴스(OCI Micro, Java + Python + nginx + MySQL, RAM 1GB)에서 캐릭터 생성 뒤 전체 굽기 동안 사이트가 느려진다.
- 측정(M0 `character`, 2026-11-16, 캐릭터 1마리·아이템 33개): 굽기 524.5초 동안 `us 4% sy 9% id 23% wa 23% st 41%`, 95% 이상 바쁜 초 393/658, free 최소 10MB, swap 최대 2.5GB, swap in/out 약 1.9GB/1.6GB. 원인은 계산량이 아니라 메모리 부족 → 스왑(`wa`)과 버스트 한도 초과(`st`)
- 원인(코드 확인): 생성 직후 모든 아이템을 굽기 큐에 넣고 `BAKE_CONCURRENCY = 13`개를 동시에 돌린다. 굽기마다 `engine.stand_front_cut`이 1024 시트를 `_defringe`에서 float64 배열(1024×1024×3 ≈ 25MB, 중간 배열 포함 100MB 안팎)로 처리한다. py-spy 상위: `stand_front_cut` 27%, `_defringe`·`_load`·Pillow 변환. OpenAI·HTTP·JSON 5%
- 측정 중 `굽기 결과 보고 실패`(Java 저장 실패) 여러 건: OpenAI 비용은 나갔는데 결과가 사라짐. 스왑 중 Java 시트 저장 타임아웃(15초)으로 추정

## 결정

- 2026-11-14: 캐릭터를 2번 인스턴스(OCI Micro, 무료)로 분리하고, 서버는 러스트로 새로 짠다(학원 수업과 연계한 학습 목적).
- 2026-11-16: 캐릭터·아이템 생성 경로를 통째로 러스트로 옮긴다. OpenAI 호출, Java 내부 API 호출, 인증 확인, 굽기 큐까지 러스트가 맡는다. 이전이 끝나면 Python `app/character/`를 뺀다. 그때까지 Python은 그대로 둔다.
- 2026-11-16: Python 코드를 그대로 옮긴다. 원본은 `nyangmeong-ai` develop. 다르게 할 수밖에 없는 것은 아래 "알려진 차이"에 적는다.
- 2026-11-16: 이전 중 1번 임시 대책(`BAKE_CONCURRENCY` 낮추기·계산만 세마포어)은 하지 않는다. `BAKE_CONCURRENCY = 13` 유지.
- 2026-11-16: 배포 위치는 AI(`/home/<계정>/ai-backend`)와 다르게 `/opt/nyangmeong-character` — 실행 파일만 두고 `ProtectHome=true`를 쓰려고(나중에 맞출 수 있음).
- 2026-11-17: 전환은 nginx만 넘긴다. Python `app/character/`는 지우지 않고 남겨 둔다(문제 시 nginx 블록만 지우면 바로 Python으로 돌아감). 위 2026-11-16의 "이전이 끝나면 뺀다"는 보류.
- 2026-11-17: nginx는 사람이 1번에서 직접 고친다(저장소에 nginx 설정 파일을 두지 않음). 절차는 아래 "M6 전환 절차".

## 옮길 범위 (`app/character/router.py`, 계약은 `nyangmeong-ai` `docs/api/character.md`·`internal.md`)

굽기 큐를 같이 쓰므로 아래 경로는 한 번에 전환한다(나눠 옮기면 같은 굽기를 두 번 하고 OpenAI 비용도 두 번).

| 경로 | 권한 | 하는 일 |
|---|---|---|
| `POST /ai/characters` | 회원 | Java 작업 생성 → 202 응답 → 백그라운드 생성·보고 → 전 아이템 굽기 |
| `POST /ai/characters/{pixelCharSn}/preview` | 회원 | 착용 합성 PNG, 안 구웠으면 큐 앞에 넣고 409 `ITEM_BAKING` |
| `POST /ai/characters/{pixelCharSn}/equipped-image` | 회원 | 착용 합성 → Java 대표 이미지 저장 |
| `POST /ai/items/bake-hints` | 관리자 | 아이템 그림 → 굽기 지시 초안(OpenAI 비전) |
| `POST /ai/items/pixel-image` | 관리자 | 실사 → 픽셀 아이템 그림(OpenAI 이미지 + `cut_item`) |
| `POST /ai/items/trial-preview` | 관리자 | 시험 굽기 1회 + 합성 |
| `POST /ai/items/{itemSn}/bake` | 관리자 | 안 구운 캐릭터 한 묶음 굽기, 관리자 화면이 `lastYn`까지 반복 |
| `POST /ai/admin/characters/jobs/{charCrtJobSn}/regenerate` | 관리자 | 무료 재생성(develop 59ba84f에 추가): 202 → Java `regen-start`·`original-photo` → 생성·보고·굽기 |

러스트가 갖출 것(Python 쪽 파일):

- 공통 계약(`core/response.py`·`errors.py`·`auth.py`·`backend.py`·`image.py`) — M2에서 옮김
- Java 내부 API 클라이언트(`character/repository.py`): 작업 생성·성공/실패 보고·굽기 대상·시트 조회/저장·대표 이미지. 기본 타임아웃 3초, 시트 15초. 응답 뒤 백그라운드 호출도 요청의 쿠키를 쓴다
- OpenAI 클라이언트(`character/clients.py`, `core/config.py`): 이미지 `gpt-image-2`, 비전 `gpt-5-nano`, 타임아웃 180초
- 이미지 계산(`engine.py`·`bake.py`·`compose.py`·`canvas.py`): float64 대신 u8·정수로 메모리를 줄이되 결과는 Python 정답과 픽셀 단위로 같게
- 굽기 큐(`character/service.py`): 동시 실행 제한, 회원 미리보기 우선(`put_first`), 재시도 `BAKE_ATTEMPTS = 2`, 저장 실패는 다시 굽지 않음. OpenAI 대기 동시 수와 메모리 큰 계산 동시 수를 따로 제한한다(2번도 RAM 1GB)

## M2에서 옮긴 것 (2026-11-16)

기준: `nyangmeong-ai` develop 코드와 FastAPI 0.161.1·Starlette 1.9.0·pydantic 2.14.1 동작(가상환경 소스·직접 실행으로 확인).

| 러스트 | Python 원본 | 내용 |
|---|---|---|
| `src/response.rs` | `core/response.py` | ApiResponse 키 순서, None 칸 제외, `timestamp`는 `datetime.now().isoformat()` 모양(KST 고정 +9, 마이크로초 0이면 소수부 없음) |
| `src/error.rs` | `core/errors.py` + FastAPI 기본 | AppError, Java 4xx 그대로(`path`만 요청 경로로, `json.dumps` 모양 `", "`·`": "`로 다시 씀, text/*에 charset 붙임), 검증 400, `{"detail"}` 404·405·본문 파싱 400, 500 |
| `src/backend.rs` | `core/backend.py` | Java 호출: 4xx 전달, 그 외 502, 리다이렉트 안 따라감, 쿠키 저장 안 함, 연결·읽기 제한 시간 3초, 실패 로그(경로 숫자 `{n}`) |
| `src/auth.rs` | `core/auth.py` | `X-Requested-With`, `/internal/auth/check`(200만 통과, 401·403 전달, 그 밖 502), `int(mbrSn)` 내장 `int()` 규칙, 관리자 `USRC0005` |
| `src/image.rs` | `core/image.py` | 10MB, 확장자 + 시그니처 |
| `src/form.rs`·`src/pyval.rs` | Starlette `request.form()`·`json()`, pydantic lax | 폼 마지막 값, 빈 폼 값 = 없음, multipart 오류 문구, JSON 형식 판정, pydantic `int`(`"1.0"`·`"1_0"`·bool 허용, `"1."`·`"1e3"` 거절) |
| `src/character.rs`·`src/app.rs` | `character/router.py`, Starlette 라우터 | 경로 7개 + `/health`(`{"status":"ok"}`), 순서: 본문 읽기 → XRW → 인증 → 값 검증 → 핸들러(지금 501), 404·405(`Allow`)·끝 `/` 307 |

## M3에서 옮긴 것 (2026-11-16)

기준: `nyangmeong-ai` e9c3edd, 원본 venv(Pillow 13.0.0·numpy 2.5.4·scipy 1.18.1)가 만든 정답.

- `engine`·`canvas`·`bake`·`compose` 이미지 계산 → `src/pil.rs`·`ndimage.rs`·`engine.rs`·`canvas.rs`·`bake.rs`·`compose.rs`. 화소 단위로 같다: 합성 시트 10·아이템 그림 6·사진 정규화 20·scipy 마스크 8, 실제 시트(맨몸 1 + 착용 5)와 임의 크기 사진 31장
- Pillow 13 소스(`Resample.c`·`Reduce.c`)에 맞춘 것: `thumbnail`은 세로가 가로보다 두 배 넘게 줄면 세로 먼저, `reduce`는 f32 승수와 `>> 24`, 필터 계수는 `double` 나눗셈
- 합성 정답 `tests/fixtures/image/`(실제 그림 아님, 다시 만들기 `sh scripts/gold.sh synth`). 실제 시트 비교는 `CHARACTER_SHEETS_DIR`(정답 `target/character-gold`, 커밋 안 함). 정답 float는 repr 문자열(serde_json 기본 실수 읽기가 마지막 한 비트를 틀림)

측정(실제 시트 1024×1024, 개발 PC, 반복 3회 중 가장 빠른 시간, 힙은 할당량 최고치). 2번 서버 최대 RSS는 아직 안 잼:

| 작업 | 러스트 시간(ms) | 러스트 힙(MB) | Python 시간(ms) | Python 힙(MB) |
| --- | --- | --- | --- | --- |
| stand_front_cut(맨몸) | 51.9 | 18.0 | 275.3 | 95.4 |
| place | 4.2 | 1.4 | 87.5 | 5.1 |
| validate(MYPC0002~0005) | 113~119 | 22.5 | 653~770 | 95.8 |
| render(착용 5개 전부) | 459.0 | 23.9 | 2351.7 | 97.6 |

## M4 [A]에서 옮긴 것 (2026-11-16)

기준: `nyangmeong-ai` develop(대조 테스트는 `contract/run.py`가 `AI_REF`, 기본 `origin/develop`에서 꺼낸 코드로 돈다). 굽기 큐·회원 경로 4개는 M5.

| 러스트 | Python 원본 | 내용 |
|---|---|---|
| `src/engine.rs`·`prompts/` | `engine.py` 프롬프트 쪽, `prompts/` | 프롬프트·눈 판독·굽기 지시 정리. 원본 venv가 만든 정답(`tests/fixtures/prompts.json`, `sh scripts/gold.sh prompts`)과 문자열이 같음 |
| `src/clients.rs` | `clients.py` | OpenAI 호출(reqwest + rustls). SDK처럼 180초·재시도 1회·`image[]`·필드 순서. 큰 계산은 `heavy`(동시 2) |
| `src/repository.rs` | `repository.py` | Java 내부 API 10개. 응답 검사는 pydantic lax(`float`·`bool` 규칙을 `pyval.rs`에 추가) |
| `src/service.rs` | `service.py` 일부 | `describe_item`·`draw_item`·`trial_preview`(`bake_once`·`render`) |
| `src/character.rs` | `router.py` | 관리자 경로 3개(`bake-hints`·`pixel-image`·`trial-preview`) 핸들러 |

대조 테스트(`contract/`)는 Python·러스트가 가짜 Java·가짜 OpenAI에 보낸 요청(경로·필드·프롬프트·그림 화소)까지 비교한다.

## M5에서 옮긴 것 (2026-11-16)

기준: `nyangmeong-ai` develop 59ba84f. 경로 8개가 모두 러스트에 있다(501 없음).

| 러스트 | Python 원본 | 내용 |
|---|---|---|
| `src/queue.rs` | `service.py` `BakeQueue`·`_bake_one`·`_draw` | 굽기 큐. 동시 13·시도 2·`put_first`·같은 키 대기자 공유·저장 실패는 다시 안 굽기·시트 파일 없음(`FILE_NOT_FOUND` 404)만 실패 표시. 상태는 잠금 하나, `.await` 동안 안 잡음. 일꾼은 `tokio::spawn`(요청이 끊겨도 굽기는 이어짐) |
| `src/service.rs` | `service.py` 나머지 | `create_job`·`generate_and_report`·`regenerate_and_report`·`preview`·`save_equipped_image`·`bake_item_batch`, `rules.py` `matches_spec` |
| `src/repository.rs` | `repository.py` 59ba84f 추가분 | `regen_start`·`fetch_original_photo`, `report_success`의 `photo` 파트(확장자로 Content-Type, 파일 이름은 httpx HTML5 치환과 같게), `CHARACTER_INPUT_ERROR` |
| `src/character.rs`·`app.rs` | `router.py` | 회원 경로 3개·전체 굽기·무료 재생성 경로. 큐는 `AppState`에 하나 |

테스트: `tests/bake_queue.rs`(원본 `test_bake.py` 큐 사례 + 동시 상한·대기자 공유), `tests/service_member.rs`(원본 create·preview·equipped·regenerate 서비스 사례), `tests/repository.rs` 추가 7개. 대조 119/119 같음(새 사례 34개, 응답 뒤 작업은 가짜 Java 기록이 멈출 때까지 기다린 뒤 비교).

## M6 전환 절차 (nginx, 1번에서 사람이)

전제: M5 2번 확인(생성 → 전체 굽기 로그·스왑 없음)이 끝난 뒤. 2026-11-17 시도에서 1번 Java(`main`)의 `GET /internal/ai/characters/bake-items` 응답에 `bakeImgRefYn`이 없어 굽기 대상 조회가 실패했다(원본 Python도 같은 필드를 필수로 읽음). 백엔드 `develop`→`main` 배포 뒤 다시 확인한다.

1. 전: `sudo nginx -T | grep -n location`으로 기존 `/ai`가 접두사(`location /ai`)인지 확인한다(정규식 `~`이면 새 블록이 안 먹음). 1번 Python 로그에 진행 중인 굽기가 없는지 확인한다. Python 로그의 전체 굽기 묶음 시간을 보고 `/ai/items/`의 `proxy_read_timeout`을 정한다
2. 블록: `location /ai/characters`(끝 `/` 없이)·`location /ai/admin/characters/`·`location /ai/items/` → `proxy_pass http://10.0.0.170:8100;`. `proxy_set_header`·`client_max_body_size`·timeout은 기존 `/ai` 블록에서 복사한다
3. 적용: `sudo nginx -t && sudo systemctl reload nginx`
4. 기대: `curl -s -w '\n%{http_code}\n' -X POST https://pawever.cloud/ai/characters/1/preview` → 403 `FORBIDDEN`이고 2번 `journalctl -u nyangmeong-character`에 `POST /ai/characters/{n}/preview -> 403` 한 줄(러스트가 받음). 다른 `/ai` 경로(예: `/ai/health`)는 1번 Python 그대로 200
5. 사이트 확인: 캐릭터 생성·아이템 등록(관리자)·전체 굽기 정상, 2번 로그에 "전체 굽기 시간", 1번 `ops/m0.sh character`에서 스왑 없음
6. 되돌리기: 추가한 블록 3개를 지우고 `sudo nginx -t && sudo systemctl reload nginx`(Python 코드가 남아 있어 바로 돌아감)

2026-11-17 결과: 12:02 KST 전환(2번 로그에 `preview -> 403` 확인). 백엔드 `main` 배포 뒤 사이트에서 새 캐릭터(623) 생성 → 전체 굽기 101.3초(M0 Python 524.5초), 36개 중 33개 성공·개당 28~36초 1회 시도. 실패 3개(itemSn 49·51·53, 그림 참고 굽기)는 1초 안에 `BackendError`(OpenAI 전 Java 호출, 원인 확인 남음). 굽기 뒤 RSS 57MB, `free` available 510MB / 952MB, PID 그대로(재시작·OOM 없음). 2번은 스왑이 없어 메모리가 모자라면 느려지지 않고 OOM으로 죽는다 → 굽기 중 최대값은 `systemctl show nyangmeong-character -p MemoryPeak`로 잰다. 2번 시계는 UTC.

## 알려진 차이

실사용에서 거의 안 나는 것만 둔다. 새로 생기면 여기에 적는다.

- OpenAI: Python SDK의 `User-Agent`·`x-stainless-*` 헤더는 없다. 재시도 대기는 같은 범위(0.375~0.5초)의 시계 기반 난수다. `OPENAI_BASE_URL`은 러스트가 `.env.local`에서도 읽는다(SDK는 환경변수만)
- 제한 시간 재는 방식: httpx는 연결·읽기·쓰기를 한 번마다 180초씩 잰다. reqwest `read_timeout`은 요청을 보낼 때 한 번 맞춘 타이머라 업로드와 응답 머리 대기를 합쳐 잰다(Java 호출은 3초·시트 15초). 멈춘 상대에서는 둘 다 시간 초과가 나고 OpenAI는 SDK처럼 한 번 재시도한다(`tests/timeouts.rs`). 큰 업로드가 느리게 오래 걸려도 합계가 한도 안이면 같다
- 눈 판독 답의 깊은 중첩: Python은 C 재귀 한도까지 받는다(3.13 이 PC 2998단계, 한도는 플랫폼마다 다름). serde_json은 128단계에서 거절해 기본 눈값이 된다(깊이를 맞추면 스택 넘침 위험). `1e400`·4300자리 정수 같은 숫자는 Python과 같게 읽는다(`pyjson::loads_str_lenient`, 정답 표 `parse_eyes`)
- `retry-after`가 HTTP 날짜 모양이면 SDK는 남은 초를 따르지만 여기서는 무시하고 기본 대기를 쓴다(OpenAI는 초 숫자로 보냄)
- 눈 판독 답의 짝 없는 서로게이트 이스케이프(`\ud800` 꼴)는 Python `json.loads`가 받지만 여기서는 거절해 기본 눈값이 된다. `NaN`·`Infinity` 상수는 받는다(값은 안 쓰므로 `null`로 읽음)
- OpenAI 그림 base64는 알파벳 밖 글자를 버리고 읽는다(`b64decode` 비엄격과 같음). 패딩이 모자란 글자는 Python과 달리 오류(둘 다 502라 응답은 같음)
- `report_success`의 `originY`: `place`가 정수 100을 돌려주는 경우(`min(.., 100)`)를 `Layout.origin_y == 100.0`으로 판별해 `"100"`을 보낸다. 우연히 정확히 100.0인 실수는 Python이 `"100.0"`을 보내지만 여기서는 `"100"`이다(확률 0에 가까움)
- 큰 이미지 계산은 동시에 2개까지(`HEAVY_CONCURRENCY`). Python은 `asyncio.to_thread` 기본 풀이다. 결과는 같다
- 응답 뒤 작업(생성·보고·굽기)의 시작 시점: Starlette `BackgroundTasks`는 응답 본문을 다 보낸 뒤 시작하고, 여기는 응답을 돌려주기 직전에 `tokio::spawn`한다. Java 작업 보고가 202 응답보다 먼저 갈 수 있다. 응답과 Java가 받는 요청은 같다(React는 상태를 Java에서 조회)
- 굽기 큐 일꾼 수: 여기는 잠금 안에서 일꾼 수를 먼저 올려 동시에 굽는 수가 늘 13(`BAKE_CONCURRENCY`) 이하다. Python은 `_work`가 처음 돌 때 수를 올려, 한 틈에 `drain`이 두 번 불리면 상한을 넘는다(검증 에이전트 실측: 작업 20개에 `gather(drain(), drain())` → 동시 20). 굽는 결과·Java 보고는 같고 OpenAI 동시 호출 수만 다르다
- 굽기 중 패닉: Python은 `_bake_one`의 예외를 `except Exception`으로 받고 같은 일꾼이 다음 작업을 꺼낸다. 여기는 패닉한 일꾼이 끝난다(굽는 중 표시 정리·대기자 알림·일꾼 수 줄이기는 Drop이 함). 남은 작업은 다른 일꾼이나 다음 `drain`이 꺼낸다. 패닉은 코드 결함일 때만 나고(이미지 계산의 패닉은 `heavy`가 오류로 바꿈) 일반 실패는 Python처럼 결과(`None`·실패 표시)로 돌아온다
- 요청이 끊겼을 때(시험 안 함, hyper·uvicorn 동작에서 추정): axum은 연결이 끊기면 핸들러 future를 버린다. 전체 굽기는 일꾼이 따로 돌아 굽기·Java 보고는 끝까지 가고 응답만 안 간다(uvicorn은 핸들러를 끝까지 돈다). 미리보기·생성 경로는 응답 전에 끊기면 그 뒤 일(작업 생성 등)을 안 한다

- 정수는 i64까지(Python은 무제한 → Java가 4xx)
- `json.loads`의 UTF-16·NaN·Infinity 미지원
- multipart 비정상 본문 세부(multer와 python-multipart)
- 로그: 원본처럼 `LOG_DIR` 날짜별 파일(30일 보관)에 `app.*`를 쓰고 콘솔(journald)에도 남긴다. 파일 이름 접두사는 `ai_` 대신 `character_`(1번 Python 파일과 구별). 2번은 systemd `TZ=Asia/Seoul`로 파일 날짜·시각이 한국 시각. 파일에 남는 기동·오류 줄은 uvicorn 메시지 대신 `INFO app.main - listening on <주소>`·`ERROR app.main - panic: <내용>`. 폴더를 못 열 때 경고 끝의 오류 문구는 러스트 `io::Error` 문자열
- 307 Location의 scheme은 `X-Forwarded-Proto`(없으면 http)
- 이미지 PNG의 바이트(압축)는 Pillow(`optimize=True`)와 다르고 디코드한 화소는 같다. 색 형식(RGB·RGBA)은 같다
- `normalize_photo`·`normalize_item_image`: PNG(8비트·팔레트·회색·RGBA·EXIF 방향)는 Pillow와 화소까지 같다. JPEG·손실 WebP는 디코더(Pillow는 libjpeg-turbo·libwebp, 여기는 zune-jpeg·image-webp)가 달라 화소가 조금 다르다(크기·방향은 같음). 손실 없는 WebP는 같다. 사진 방향은 EXIF 청크만 읽는다(Pillow는 없으면 PNG `Raw profile type exif` 텍스트 청크와 XMP `tiff:Orientation`도 본다 → 거기에만 방향이 적힌 사진은 누운 채로 남는다). 결과는 OpenAI 입력으로만 쓴다
- 16비트 PNG는 Pillow와 같다(RGB·RGBA·회색+알파는 위 바이트, 회색은 255로 자름). CMYK JPEG·애니메이션 등 드문 형식은 비교하지 않았다

## 서버·네트워크·배포

- 1번 인스턴스: CentOS Stream 8(2024-05 지원 종료), 방화벽은 `firewalld`(`public` zone). Java는 `0.0.0.0:8080`. Python은 uvicorn 워커 자식 프로세스에서 일한다
- 2번 인스턴스: 홈 리전(오사카), `VM.Standard.E2.1.Micro`, Ubuntu 24.04 Minimal(x86_64, glibc 2.39), 부트 볼륨 47GB(Balanced, 백업 없음), 1번과 같은 VCN·서브넷, 임시 공인 IP(SSH 전용). `nano`·`iptables-persistent` 설치. 스왑은 M4 측정 후 결정. systemd `nyangmeong-character`(계정 `nmchar`, `/opt/nyangmeong-character`)
- 네트워크: 사설 IP 1번 `10.0.0.41`, 2번 `10.0.0.170`. 보안 목록 수신 규칙 `10.0.0.41/32 → TCP 8100`, `10.0.0.170/32 → TCP 8080`. 1번 firewalld rich rule(`10.0.0.170/32` → 8080). 2번 iptables INPUT(`-s 10.0.0.41 --dport 8100 ACCEPT`, REJECT 앞) 저장됨
  - 서버 간 포트는 OCI 보안 목록(서브넷 → 보안 → 수신 규칙, 소스 `/32`) + 서버 방화벽(1번 `firewall-cmd` rich rule, 2번 `iptables -I INPUT <REJECT 줄 번호>` 후 `netfilter-persistent save`) 두 겹 다 연다. 시간 초과면 보안 목록(패킷이 안 옴, `iptables -L -n -v` 카운터 0), 즉시 거절이면 서버 방화벽. 임시 서버는 `python3 -m http.server 8100 --directory /tmp/empty`(홈 폴더 노출 방지)
- 2번 SSH 키: 맥 키, 학원 PC 키(PuTTYgen, MobaXterm), Actions 배포 키. 새 키는 이미 들어간 기기로 접속해 `~/.ssh/authorized_keys`에 공개키 한 줄을 `>>`로 덧붙인다(PuTTYgen 위쪽 칸 전체 — 여러 줄로 접혀 보여도 한 줄. 일부만 복사하면 `Server refused our key`)
- 배포(GitHub Actions, `nyangmeong-ai`와 같은 구성): `build.yml`(develop PR → fmt·clippy·test·build, 대조 테스트는 빠짐), `main.yml`(main PR 머지 또는 수동 실행 → 러너 `ubuntu-24.04`에서 빌드 → 2번에 scp → `deploy/install.sh` 설치·재시작·`/health` 확인), `pr-notify.yml`(AI와 같은 파일)
  - 시크릿(이름은 AI와 같고 값은 2번 기준): `OCI_HOST`·`OCI_USER`(ubuntu)·`OCI_PORT`(22)·`KSMARTDEV`·`ENV_FILE`(`/etc/nyangmeong-character.env`로 덮어씀)·`DISCORD_WEBHOOK_URL`
  - `KSMARTDEV`는 Go SSH 라이브러리라 `-----BEGIN ... PRIVATE KEY-----` 형식만 읽는다. PuTTYgen 키는 **Conversions → Export OpenSSH key**로 내보낸 파일 전체. `.ppk`·`---- BEGIN SSH2 PUBLIC KEY ----`를 넣으면 `ssh.ParsePrivateKey: ssh: no key found`
- 크로스 컴파일(확인용, 배포는 Actions): 맥 `brew install zig`, 학원 PC winget `zig.zig`. 둘 다 `cargo install --locked cargo-zigbuild`, `rustup target add x86_64-unknown-linux-gnu` 후 `cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.39`
- 측정 스크립트 `ops/m0.sh <이름>`(1번에서 실행, 결과 `~/m0-<이름>-<시각>/summary.txt`): vmstat, Python(워커 자손 포함)·Java CPU 초, 프로세스별 RSS 최대, py-spy 비중, 굽기 로그. 이름은 `character`·`item-image`·`item-bake`

## 지뢰

- 2번 서비스는 `BACKEND_BASE_URL`이 비면 `/ai/**`를 전부 502로 답한다(Python과 같음). 경로를 쓰기 전에 시크릿 `ENV_FILE`에 `BACKEND_BASE_URL=http://10.0.0.41:8080`을 넣고 다시 배포한다
- 대조 테스트(`contract/`)는 FastAPI·Starlette·pydantic 버전에 묶여 있다. `nyangmeong-ai` 의존성이 바뀌면 다시 돌린다
- 굽기 큐는 프로세스 메모리에 있다. 8개 경로(M5에서 무료 재생성 추가)는 nginx에서 한 번에 넘긴다. 전환 전 Python에서 돌던 굽기가 끝났는지 확인하고 넘긴다
- OpenAI 키는 2번에만 둔다. 저장소·로그에 남기지 않는다. systemd `EnvironmentFile`(권한 600)로 넣는다
- 2번 8100에는 인증 없이 들어오는 길이 없어야 한다: nginx(1번 사설 IP)만 닿게 보안 목록 + iptables 둘 다. 인증 확인은 러스트가 매 요청 Java에 묻는다(캐시 없음)
- 2번도 RAM 1GB다. 큰 계산 동시 수를 제한하지 않으면 1번과 같은 스래싱이 2번에서 난다. M4에서 RSS·swap을 잰다
- nginx(1번): 기존 `/ai`가 접두사 규칙인지 먼저 확인(`sudo nginx -T | grep -n location`). 정규식(`~`)이면 새 블록이 안 먹는다. `location /ai/characters`(끝 `/` 없이)·`location /ai/admin/characters/`·`location /ai/items/` 3개. `proxy_set_header`·timeout·`client_max_body_size`는 기존 `/ai` 블록에서 그대로 복사. `pixel-image`는 30~40초, `/ai/items/{itemSn}/bake`는 한 묶음을 다 굽고 답하므로 더 길다 — Python 로그로 묶음 시간을 확인해 timeout을 정한다
- 계약(`ApiResponse`, 오류 코드 `FORBIDDEN`·`ITEM_BAKING`·`INVALID_REQUEST`·502, 캐릭터 규격 512×512·얼굴 영역, 시트 1024)은 `nyangmeong-ai`·백엔드·프론트와 같아야 한다. 어긋나면 오류 없이 조용히 깨진다. 백엔드·프론트 팀원에게 캐릭터·아이템 경로가 러스트로 간다고 알린다
- Java 내부 API는 `/internal/**` POST에도 `X-Requested-With`를 요구한다. 응답 뒤 백그라운드 호출은 요청 때 복사한 쿠키를 쓰므로, 쿠키가 만료되면 Java가 401(보고 실패로 남음, 작업은 Java 타임아웃으로 정리)
- `scipy.ndimage.label`·`binary_erosion`·`binary_propagation`의 이웃 정의(4/8방향)·반올림, float64→정수 변환이 다르면 테두리가 달라진다. M3는 정답 비교 테스트부터 만든다. 정답 파일은 M0 측정용 캐릭터의 시트를 받아 쓴다(저장소에 커밋하지 않음)
- Actions 러너는 인터넷에서 2번 22번으로 들어온다. 22번 소스를 개인 IP로 좁히면 배포가 끊긴다. 러너를 `ubuntu-latest`로 바꾸면 glibc가 2번(2.39)보다 높아져 실행 파일이 안 돌 수 있다
- 1번은 CentOS Stream 8 지원 종료라 `dnf install`이 저장소 오류로 실패할 수 있다
- 1번 인스턴스는 지우지 않는다. 임시 공인 IP는 인스턴스 종료 때만 바뀐다(중지·시작은 유지)
- 서버에서 `cargo build`를 돌리면 1GB 메모리로 OOM 날 수 있다. 빌드는 러너·PC에서 한다
