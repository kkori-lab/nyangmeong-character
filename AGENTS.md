# 냥멍케어 캐릭터 서버 컨벤션

이 저장소에서 코드를 쓰는 사람과 AI 도구가 따르는 규칙이다. 이 파일에는 항상 필요한 규칙만 두고, 나머지는 `docs/`에 있다("나머지 규칙이 있는 곳").

- Codex는 이 파일을 자동으로 읽는다. Claude Code는 `CLAUDE.md`의 `@AGENTS.md` 한 줄로 읽는다. 그 줄을 지우면 규칙이 통째로 빠진다.
- 이 파일은 AI 도구가 매번 통째로 읽으므로 짧게 유지한다.

## 이 저장소

`nyangmeong-ai`(Python) `app/character/`를 2번 인스턴스의 러스트 서버로 옮기는 저장소다. 배경·결정·서버 정보·지뢰는 `docs/design/migration.md`.

## 코드를 쓰기 전에 꼭 알 것

어기면 되돌리기 어렵거나 오류 없이 조용히 잘못되는 것만 모았다.

1. **비밀값을 커밋하지 않는다.** 이 저장소는 공개다. 키·비밀번호·토큰은 git에 올리지 않는 `.env.local`과 GitHub 시크릿(`ENV_FILE`)에만 둔다. 반려동물 사진·캐릭터 시트 원본(정답 파일 포함)도 올리지 않는다. (`docs/convention/config.md`)
2. **Python 코드를 그대로 옮긴다.** 원본은 `nyangmeong-ai` develop이다. 옮길 함수는 먼저 원본을 읽고 같은 입력에 같은 출력(상태 코드·본문 바이트·픽셀)을 낸다. 다르게 할 수밖에 없으면 `docs/design/migration.md` "알려진 차이"에 적는다. 원본에 없는 기능을 더하지 않는다.
3. **계약을 혼자 바꾸지 않는다.** `ApiResponse` 모양, `X-Requested-With` 검사, Java 인증 오류(본문·`Set-Cookie`) 그대로 전달, 캐릭터 규격(512×512, 얼굴 영역, 시트 1024)은 `nyangmeong-ai`·백엔드·프론트와 같아야 한다. 어긋나면 오류 없이 조용히 깨진다.
4. **저장·인증·소유 검사는 Java가 한다.** DB에 붙지 않는다. Java 내부 API는 브라우저 쿠키를 그대로 넘겨 부른다(서버 간 키 없음).
5. **2번 서버는 RAM 1GB다.** 서버에서 `cargo build`를 돌리지 않는다(빌드는 Actions 러너). 메모리 큰 계산은 동시 수를 제한한다.
6. **테스트에서 외부 AI API(OpenAI)를 부르지 않는다.** Java는 가짜로 대신한다(`contract/fake_java.py`). 키가 없는 CI에서도 통과해야 한다.
7. **테스트를 끄거나 지워서 통과시키지 않는다.** 대조 테스트 사례(`contract/compare.py`)도 같다. (`docs/convention/test.md`)

## 기술 스택

Rust stable(edition 2024), axum 0.8, tokio, reqwest 0.13, serde_json(`preserve_order`). Docker는 쓰지 않는다.

- 의존성은 `cargo add`·`cargo remove`로만 바꾸고 `Cargo.toml`·`Cargo.lock`을 같이 커밋한다.
- 줄바꿈은 LF(`.gitattributes`), 포맷은 `cargo fmt`.

## 모듈 구조

```
src/
├─ main.rs        서버 시작(PORT 기본 8100)
├─ lib.rs         모듈 공개(통합 테스트·examples용)
├─ app.rs         라우터 조립, 404·405·끝 / 307, 요청 로그        ← app/main.py, Starlette
├─ config.rs      설정(환경변수·.env.local)                        ← core/config.py
├─ response.rs    ApiResponse                                      ← core/response.py
├─ error.rs       오류 응답, Java 오류 그대로 전달                  ← core/errors.py
├─ backend.rs     Java 내부 API 호출                                ← core/backend.py
├─ auth.rs        X-Requested-With, 회원·관리자 확인               ← core/auth.py
├─ image.rs       업로드 검증                                       ← core/image.py
├─ form.rs        폼·JSON 본문 읽기                                 ← Starlette request.form()·json()
├─ pyval.rs       pydantic·Python 값 변환 규칙
├─ pyjson.rs      Python json.loads·dumps 모양
├─ logs.rs        로그 형식                                         ← core/logs.py
├─ pil.rs         Pillow 동작(열기·NEAREST·붙이기·thumbnail·PNG)    ← PIL 13.0.0
├─ ndimage.rs     scipy.ndimage(label·침식·팽창·채우기·번짐)        ← scipy 1.18.1
├─ engine.rs      시트 분할·cut_item·사진 정규화                    ← character/engine.py
├─ canvas.rs      눈 찾기·512 배치                                  ← character/canvas.py
├─ bake.rs        착용 컷 정렬·아이템 레이어·validate               ← character/bake.py
├─ compose.rs     착장 합성                                         ← character/compose.py
├─ clients.rs     OpenAI 호출·큰 계산 동시 수 제한                  ← character/clients.py
├─ repository.rs  Java 캐릭터 내부 API                              ← character/repository.py
├─ service.rs     회원·관리자 경로 흐름                             ← character/service.py
├─ queue.rs       굽기 큐(BakeTask·BakeQueue)                       ← character/service.py
└─ character.rs   /ai/characters·/ai/items 경로                     ← character/router.py
prompts/          OpenAI 프롬프트·화풍 그림(원본 app/character/prompts/ 그대로, 바이너리에 넣음)
contract/         Python↔러스트 대조 테스트(가짜 Java·OpenAI), 정답 만들기(gold_images.py·gold_prompts.py)·측정(image_bench.py)
tests/            통합 테스트(정답 파일 비교 등), fixtures/image/ 합성 정답(실제 그림 아님)
examples/         측정 프로그램(image_bench.rs)
deploy/           systemd 설치(Actions가 씀)
ops/              서버에서 손으로 돌리는 스크립트(측정)
```

- 새 모듈은 원본 Python 파일과 짝이 되게 만들고, 파일 첫 줄 주석에 원본 경로를 적는다.

## 검증 명령

`sh scripts/check.sh` 하나다: `cargo fmt --check`, `cargo clippy --all-targets -D warnings`, `cargo test`, 대조 테스트(`scripts/contract.sh`). 대조 테스트는 옆 폴더의 `nyangmeong-ai`(`../ai`·`../nyangmeong-ai` 또는 `AI_REPO`)와 그 `.venv`가 있어야 돈다. CI(`build.yml`)는 대조를 뺀 cargo 검사만 돈다. 실제 시트 정답 비교는 `CHARACTER_SHEETS_DIR`(bare.png·worn_<부위코드>.png 폴더)가 있는 PC의 `check.sh`에서 돈다(`scripts/gold.sh`가 정답을 만든다).

## 나머지 규칙이 있는 곳

아래 파일은 자동으로 읽히지 않는다. 해당하는 일을 하기 전에 그 파일만 연다.

| 이럴 때 | 읽는 파일 |
| --- | --- |
| 이전 배경·서버·배포·지뢰를 알아야 할 때 | `docs/design/migration.md` |
| 테스트를 쓰거나 실패한 테스트를 고칠 때 | `docs/convention/test.md` |
| 설정·비밀값·환경변수를 다룰 때, 핸드오프를 쓸 때 | `docs/convention/config.md` |
| 브랜치·커밋·PR을 만들 때 | `docs/convention/git.md` |
| 마일스톤을 만들거나 고칠 때 | `docs/convention/milestone.md` |

## AI 도구 작업 방식

사용량 한도가 있는 플랜을 쓰는 팀원이 있어 토큰을 아낀다.

- 이 파일은 이미 읽혀 있으므로 다시 읽지 않는다. `docs/`는 위 표에서 해당하는 파일만 연다.
- 파일은 필요한 부분만 읽는다. 원본 Python도 옮길 함수만 검색해서 그 주변만 읽는다. `target/`, `.venv/`, `__pycache__/`는 읽지 않는다.
- 검증 명령은 `sh scripts/check.sh`다. 실패한 부분만 본다. `/end`가 이 명령의 종료 코드가 0이 아니면 마일스톤을 끝내지 않는다.
- `화면:` 완료 조건은 서버를 띄워(`cargo run`) curl·Postman으로 요청을 보내 확인한다. 2번 서버에서 확인할 것은 명령과 기대 결과를 사람에게 넘긴다(이 PC에서 서버 SSH를 하지 않는다).
- 기능 구현은 기획(`/start`, Opus) → 사람 승인(`승인` 또는 `/impl`) → 구현과 검증 루프(`/impl`: 구현은 Sonnet, 검증은 대화를 보지 않는 별도 에이전트 Opus, `실패`면 고쳐서 최대 3회, 그래도 실패면 사람에게 넘긴다) → 사람 확인 → `/end` 순서다. 첫 검증 실패 뒤에는 기준 파일(기획·검증 명령·의존성·테스트·대조 사례)을 고칠 수 없다(`scripts/verify-loop.sh` 훅). 루프를 버리는 것(`clear`)은 사람만 한다. 기획이 승인되기 전에는 코드를 쓰지 않는다. 승인된 뒤 사람이 기획에 없는 것을 요청하면 다시 기획하지 않고, 고치기 전에 `handoff/<이름>.md`의 `### 추가 요청`에 먼저 적는다(형식은 `.claude/skills/impl/SKILL.md` 6번).
- 몇 개 파일로 끝나는 일에는 서브에이전트를 쓰지 않는다.
- 작업 하나가 끝나면 `/end`로 핸드오프를 남기고 `/clear`한 뒤, 다음 작업은 `/start`로 시작한다.
- 개인 지시는 `CLAUDE.local.md`, 개인 설정은 `.claude/settings.local.json`에 둔다(git 제외). 저장소의 `.claude/skills/`는 공용이라 PR로만 바꾼다.
