# 핸드오프

공개 저장소다. 서버 IP·포트 외 접속 정보·키·개인 PC 경로를 적지 않는다.

## 왜 이 저장소인가

- 1번 인스턴스(OCI Micro, Java + Python + nginx + MySQL, RAM 1GB)에서 캐릭터 생성 뒤 전체 굽기 동안 사이트가 느려진다.
- 측정(M0 `character`, 2026-11-16, 캐릭터 1마리·아이템 33개): 굽기 524.5초 동안 `us 4% sy 9% id 23% wa 23% st 41%`, 95% 이상 바쁜 초 393/658, free 최소 10MB, swap 최대 2.5GB, swap in/out 약 1.9GB/1.6GB. 원인은 계산량이 아니라 메모리 부족 → 스왑(`wa`)과 버스트 한도 초과(`st`)
- 원인(코드 확인, `nyangmeong-ai` `app/character/`): 생성 직후 모든 아이템을 굽기 큐에 넣고 `BAKE_CONCURRENCY = 13`개를 동시에 돌린다. 굽기마다 `engine.stand_front_cut`이 1024 시트를 `_defringe`에서 float64 배열(1024×1024×3 ≈ 25MB, 중간 배열 포함 100MB 안팎)로 처리한다. py-spy 상위: `stand_front_cut` 27%, `_defringe`·`_load`·Pillow 변환. OpenAI·HTTP·JSON 5%
- 측정 중 `굽기 결과 보고 실패`(Java 저장 실패) 여러 건: OpenAI 비용은 나갔는데 결과가 사라짐. 스왑 중 Java 시트 저장 타임아웃(15초)으로 추정
- 결정(2026-11-14): 캐릭터를 2번 인스턴스(OCI Micro, 무료)로 분리하고, 서버는 러스트로 새로 짠다(학원 수업과 연계한 학습 목적).
- 결정(2026-11-16): 캐릭터·아이템 생성 경로를 통째로 러스트로 옮긴다. OpenAI 호출, Java 내부 API 호출, 인증 확인, 굽기 큐까지 러스트가 맡는다. 이전이 끝나면 Python `app/character/`를 뺀다. 그때까지 Python은 그대로 둔다.

## 옮길 범위 (`nyangmeong-ai` `app/character/router.py`, 계약은 `docs/api/character.md`·`docs/api/internal.md`)

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

러스트가 새로 갖출 것(Python 쪽 해당 파일):

- 공통 계약: `ApiResponse` 모양(`core/response.py`, null 필드 제외, 성공엔 `path` 없음), `X-Requested-With: XMLHttpRequest` 검사(`core/auth.py`), 업로드 검증 10MB(`core/image.py`)
- 인증: 브라우저 Cookie로 Java `GET /internal/auth/check`, 401·403은 본문·`Set-Cookie` 그대로 전달, 관리자 = `mbrAuthrtCd == USRC0005`(`core/auth.py`, `core/backend.py`)
- Java 내부 API 클라이언트: 작업 생성·성공/실패 보고·굽기 대상·시트 조회/저장·대표 이미지(`character/repository.py`). 기본 타임아웃 3초, 시트 15초. 응답 뒤 백그라운드 호출도 요청의 쿠키를 쓴다
- OpenAI 클라이언트: 이미지 `gpt-image-2`, 비전 `gpt-5-nano`, 타임아웃 180초(`character/clients.py`, `core/config.py`)
- 이미지 계산: `engine.py`·`bake.py`·`compose.py`·`canvas.py`. float64 대신 u8·정수로 메모리를 줄이되 결과는 Python 정답과 픽셀 단위로 같게
- 굽기 큐: 동시 실행 제한, 회원 미리보기 우선(`put_first`), 재시도 `BAKE_ATTEMPTS = 2`, 저장 실패는 다시 굽지 않음(`character/service.py`). OpenAI 대기 동시 수와 메모리 큰 계산 동시 수를 따로 제한한다(2번도 RAM 1GB)

## 지금 상태

- 1번 인스턴스: CentOS Stream 8(2024-05 지원 종료), 방화벽은 `firewalld`(`public` zone, `iptables-persistent` 없음). Java는 `0.0.0.0:8080`에서 받음(2026-11-16 `ss -tlnp` 확인). Python은 uvicorn 워커 자식 프로세스에서 일한다(측정 때 확인)
- 2번 인스턴스: 홈 리전(오사카), `VM.Standard.E2.1.Micro`, Ubuntu 24.04 Minimal(x86_64), 부트 볼륨 47GB(Balanced, 백업 없음), 1번과 같은 VCN·서브넷, 임시 공인 IP(SSH 전용, 고정 IP 안 받음). `nano`·`iptables-persistent` 설치. 스왑은 M4 측정 후 결정. systemd `nyangmeong-character` 실행 중(2026-11-16 Actions 첫 배포)
- 네트워크(M1 끝, 2026-11-16): 사설 IP 1번 `10.0.0.41`, 2번 `10.0.0.170`. 보안 목록 수신 규칙 `10.0.0.41/32 → TCP 8100`, `10.0.0.170/32 → TCP 8080`. 1번 firewalld rich rule(`10.0.0.170/32` → 8080). 2번 iptables INPUT 5번 줄(`-s 10.0.0.41 --dport 8100 ACCEPT`, REJECT 앞) 저장됨. 확인: 1번→2번 `/health` 200, 외부→2번 8100 시간 초과, 2번→1번 `/internal/auth/check` 401, 2번→OpenAI 응답
- 2번 SSH 키: 맥 키, 학원 PC 키(PuTTYgen, MobaXterm 접속), Actions 배포 키. 새 키는 이미 들어간 기기로 접속해 `~/.ssh/authorized_keys`에 공개키 한 줄을 `>>`로 덧붙인다(PuTTYgen 위쪽 칸 전체 — 칸에서 여러 줄로 접혀 보여도 한 줄. 일부만 복사하면 `Server refused our key`)
- Actions 배포 키(`KSMARTDEV`): Go SSH 라이브러리라 `-----BEGIN ... PRIVATE KEY-----` 형식만 읽는다. PuTTYgen 키는 **Conversions → Export OpenSSH key**로 내보낸 파일 전체를 넣는다. `.ppk`·`---- BEGIN SSH2 PUBLIC KEY ----`(Save public key)를 넣으면 `ssh.ParsePrivateKey: ssh: no key found`
- 코드: axum 0.8 + tokio + reqwest 0.13(TLS 없음 — OpenAI 호출 전 M4에서 rustls 추가), `GET /health` → `{"status":"ok"}`(Python과 같음), 포트는 환경변수 `PORT`(기본 8100, 이 저장소에서 정한 값), `0.0.0.0` 바인드. 맥·학원 PC(Windows, rustup + VS Build Tools 2022)에서 `cargo build`·`clippy -D warnings`·`fmt --check`·`curl` 200 확인
- 크로스 컴파일(맥): `brew install zig`, `cargo install cargo-zigbuild`, `rustup target add x86_64-unknown-linux-gnu` 후 `cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.39`(서버 glibc 2.39). 배포는 Actions가 하므로 확인용
- 배포(GitHub Actions, `nyangmeong-ai`와 같은 구성): `build.yml`(develop PR → fmt·clippy·test·build), `main.yml`(main PR 머지 또는 수동 실행 → 러너 `ubuntu-24.04`에서 빌드 → 2번에 실행 파일 scp → `deploy/install.sh`가 `/opt/nyangmeong-character`에 설치, systemd `nyangmeong-character`(계정 `nmchar`) 재시작, `/health` 200 확인), `pr-notify.yml`(AI와 같은 파일). 시크릿 이름은 AI와 같고 값은 2번 기준: `OCI_HOST`·`OCI_USER`(ubuntu)·`OCI_PORT`(22)·`KSMARTDEV`(2번 배포 전용 키)·`ENV_FILE`(M4부터, `/etc/nyangmeong-character.env`로 덮어씀)·`DISCORD_WEBHOOK_URL`. 배포 위치는 AI(`/home/<계정>/ai-backend`)와 다르게 `/opt` — 실행 파일만 두고 `ProtectHome=true`를 쓰려고(2026-11-16, 나중에 맞출 수 있음)
- 학원 PC 크로스 컴파일: zig(winget `zig.zig`)·`cargo install --locked cargo-zigbuild`·`rustup target add x86_64-unknown-linux-gnu` 후 맥과 같은 명령. 배포는 Actions가 하므로 확인용
- 측정 스크립트 `m0.sh <이름>`(1번에서 실행, 결과는 `~/m0-<이름>-<시각>/summary.txt`): vmstat, Python(워커 자손 포함)·Java CPU 초, 프로세스별 RSS 최대, py-spy 비중, 굽기 로그. 저장소에는 아직 없음

## 다음에 할 것 (순서대로)

### M2 공통 계약 (끝, 2026-11-16)

원칙: Python 코드를 그대로 옮긴다. 기준은 `nyangmeong-ai` develop(= 로컬 LJS) 코드와 FastAPI 0.161.1·Starlette 1.9.0·pydantic 2.14.1 동작(가상환경 소스·직접 실행으로 확인).

| 러스트 | Python 원본 | 내용 |
|---|---|---|
| `src/response.rs` | `core/response.py` | ApiResponse 키 순서, None 칸 제외, `timestamp`는 `datetime.now().isoformat()` 모양(KST 고정 +9, 마이크로초 0이면 소수부 없음) |
| `src/error.rs` | `core/errors.py` + FastAPI 기본 | AppError, Java 4xx 그대로(`path`만 요청 경로로, `json.dumps` 모양 `", "`·`": "`로 다시 씀, text/*에 charset 붙임), 검증 400, `{"detail"}` 404·405·본문 파싱 400, 500 |
| `src/backend.rs` | `core/backend.py` | Java 호출: 4xx 전달, 그 외 502, 리다이렉트 안 따라감, 쿠키 저장 안 함, 연결·읽기 제한 시간 3초, 실패 로그(경로 숫자 `{n}`) |
| `src/auth.rs` | `core/auth.py` | `X-Requested-With`, `/internal/auth/check`(200만 통과, 401·403 전달, 그 밖 502), `int(mbrSn)` 내장 `int()` 규칙, 관리자 `USRC0005` |
| `src/image.rs` | `core/image.py` | 10MB, 확장자 + 시그니처 |
| `src/form.rs`·`src/pyval.rs` | Starlette `request.form()`·`json()`, pydantic lax | 폼 마지막 값, 빈 폼 값 = 없음, multipart 오류 문구, JSON 형식 판정, pydantic `int`(`"1.0"`·`"1_0"`·bool 허용, `"1."`·`"1e3"` 거절) |
| `src/character.rs`·`src/app.rs` | `character/router.py`, Starlette 라우터 | 경로 7개 + `/health`(`{"status":"ok"}`), 순서: 본문 읽기 → XRW → 인증 → 값 검증 → 핸들러(지금 501), 404·405(`Allow`)·끝 `/` 307 |

확인: `contract/` 대조 테스트 47/47 같음(가짜 Java, 로컬). 단위 테스트 24개. 방법은 `contract/README.md`.

알려진 차이(실사용에서 거의 안 남): 정수는 i64까지(Python은 무제한 → Java가 4xx), `json.loads`의 UTF-16·NaN·Infinity 미지원, multipart 비정상 본문 세부(multer와 python-multipart), 쓰기 제한 시간 없음(reqwest), 로그는 날짜별 파일 대신 journald, 307 Location의 scheme은 `X-Forwarded-Proto`(없으면 http).

### M3 이미지 계산 (다음)

1. 정답 파일 모으기: M0 측정용 캐릭터의 맨몸 시트·착용 시트를 Java에서 받는다(저장소에 커밋하지 않음)
2. `engine.py`·`bake.py`·`compose.py`·`canvas.py`를 읽고 함수 단위로 옮긴다. `stand_front_cut`(`_load`·`_fill_holes`·`_defringe`·`_crop`)부터
3. Python 결과와 픽셀 단위 비교 테스트 → 실행 시간·최대 메모리 비교

M1 때 쓴 방법(다시 필요할 때): 서버 간 포트는 OCI 보안 목록(서브넷 → 보안 → 수신 규칙, 소스 `/32`) + 서버 방화벽(1번 `firewall-cmd` rich rule, 2번 `iptables -I INPUT <REJECT 줄 번호>` 후 `netfilter-persistent save`) 두 겹 다 연다. 시간 초과면 보안 목록(패킷이 2번에 안 옴, `iptables -L -n -v` 카운터 0), 즉시 거절이면 서버 방화벽. 임시 서버는 `python3 -m http.server 8100 --directory /tmp/empty`(홈 폴더 노출 방지)

### 이전 중 1번 임시 대책 (2026-11-16 결정: 하지 않음, `BAKE_CONCURRENCY = 13` 유지)

이전은 오래 걸린다. 그동안 Python `BAKE_CONCURRENCY`를 낮추거나 계산(`bake_source`·`validate`)만 세마포어로 1~2개씩 돌리면 1번 스왑이 줄 것으로 본다. 바꾸면 `m0.sh character`로 다시 잰다.

## 마일스톤

| # | 내용 | 완료 조건 | 상태 |
|---|---|---|---|
| M0 | 기준 측정: 1번에서 캐릭터 생성·아이템 등록 | `character` 숫자(위 "왜") + `item-image`·`item-bake`(선택) | `character` 끝 |
| M1 | 네트워크: 1번→2번 8100, 2번→1번 8080, 2번→OpenAI, `GET /health` | 1번→2번 `/health` 200, 외부→2번 8100 시간 초과, 2번→1번 8080 응답, 2번→OpenAI 응답 | 끝 (2026-11-16, Actions 배포 후 1번→2번 `/health` 200, 외부 시간 초과) |
| M2 | 공통 계약: `ApiResponse`, `X-Requested-With`, Java 인증 확인과 오류(본문·`Set-Cookie`) 그대로 전달, 업로드 검증 | 같은 요청을 Python과 러스트에 보내 상태 코드·본문·`Set-Cookie`가 같음 | 끝 (2026-11-16, `contract/` 47/47, 핸들러 본문은 M4) |
| M3 | 이미지 계산: `stand_front_cut`(`_load`·`_defringe`·`_crop`)·`place`·`bake_source`·`validate`·`render`·`cut_item` | Python 결과 PNG(정답 파일)와 픽셀 단위로 같음, 실행 시간·최대 메모리 비교 숫자 | 대기 |
| M4 | 외부 연결과 큐: OpenAI 클라이언트, Java 내부 API 클라이언트, 굽기 큐(동시 수 둘로 제한, 미리보기 우선, 재시도), 7개 경로 | 2번에 직접 요청해 캐릭터 1마리 생성 → 전체 굽기 끝, 2번 swap·메모리 숫자 | 대기 |
| M5 | 전환: nginx 분기, Python `app/character/` 제거 | 사이트에서 캐릭터 생성·아이템 등록·전체 굽기 정상, 1번 `m0.sh character`에서 스왑 없음 | 대기 |

## 지뢰

- 2번 서비스는 `BACKEND_BASE_URL`이 비면 `/ai/**`를 전부 502로 답한다(Python과 같음). 경로를 쓰기 전에 시크릿 `ENV_FILE`에 `BACKEND_BASE_URL=http://10.0.0.41:8080`을 넣고 다시 배포한다
- 대조 테스트(`contract/`)는 FastAPI·Starlette·pydantic 버전에 묶여 있다. `nyangmeong-ai` 의존성이 바뀌면 다시 돌린다
- 굽기 큐는 프로세스 메모리에 있다. 7개 경로는 nginx에서 한 번에 넘긴다. 전환 전 Python에서 돌던 굽기가 끝났는지 확인하고 넘긴다
- OpenAI 키는 2번에만 둔다. 저장소·로그에 남기지 않는다(공개 저장소). systemd `EnvironmentFile`(권한 600)로 넣는다
- 2번 8100에는 인증 없이 들어오는 길이 없어야 한다: nginx(1번 사설 IP)만 닿게 보안 목록 + iptables 둘 다. 인증 확인은 러스트가 매 요청 Java에 묻는다(캐시 없음)
- 2번도 RAM 1GB다. 큰 계산 동시 수를 제한하지 않으면 1번과 같은 스래싱이 2번에서 난다. M4에서 RSS·swap을 잰다
- nginx(1번): 기존 `/ai`가 접두사 규칙인지 먼저 확인(`sudo nginx -T | grep -n location`). 정규식(`~`)이면 새 블록이 안 먹는다. `POST /ai/characters`는 끝 `/`가 없으므로 `location /ai/characters`(끝 `/` 없이)와 `location /ai/items/`. `proxy_set_header`·timeout·`client_max_body_size`는 기존 `/ai` 블록에서 그대로 복사(쿠키·`X-Requested-With`·`Set-Cookie` 전달). `pixel-image`는 30~40초, `/ai/items/{itemSn}/bake`는 한 묶음을 다 굽고 답하므로 더 길다 — Python 로그로 묶음 시간을 확인해 timeout을 정한다
- 계약(`ApiResponse`, 오류 코드 `FORBIDDEN`·`ITEM_BAKING`·`INVALID_REQUEST`·502, 캐릭터 규격 512×512·얼굴 영역, 시트 1024)은 `nyangmeong-ai`·백엔드·프론트와 같아야 한다. 어긋나면 오류 없이 조용히 깨진다. 백엔드·프론트 팀원에게 캐릭터·아이템 경로가 러스트로 간다고 알린다
- Java 내부 API는 `/internal/**` POST에도 `X-Requested-With`를 요구한다(Java `RequestedWithFilter`). 응답 뒤 백그라운드 호출은 요청 때 복사한 쿠키를 쓰므로, 쿠키가 만료되면 Java가 401(보고 실패로 남음, 작업은 Java 타임아웃으로 정리)
- `scipy.ndimage.label`·`binary_erosion`·`binary_propagation`의 이웃 정의(4/8방향)·반올림, float64→정수 변환이 다르면 테두리가 달라진다. M3는 정답 비교 테스트부터 만든다. 정답 파일은 M0에서 만든 측정용 캐릭터의 시트를 받아 쓴다(저장소에 커밋하지 않음)
- Actions 러너는 인터넷에서 2번 22번으로 들어온다. 22번 소스를 개인 IP로 좁히면 배포가 끊긴다. 러너를 `ubuntu-latest`로 바꾸면 glibc가 2번(2.39)보다 높아져 실행 파일이 안 돌 수 있다
- 1번은 CentOS Stream 8 지원 종료라 `dnf install`이 저장소 오류로 실패할 수 있다
- 1번 인스턴스는 지우지 않는다(임시가 아닌 고정 공인 IP여도 사설 IP·설정이 바뀐다). 임시 공인 IP는 인스턴스 종료 때만 바뀐다(중지·시작은 유지, OCI 문서)
- 서버에서 `cargo build`를 돌리면 1GB 메모리로 OOM 날 수 있다. 빌드는 맥에서 크로스 컴파일
