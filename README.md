# nyangmeong-character

냥멍케어 캐릭터·아이템 생성 서버(러스트). `nyangmeong-ai`(Python)의 `app/character/`를 2번 인스턴스로 옮기는 중이다. 배경·결정·서버 정보는 `docs/design/migration.md`, 진행은 `milestones/`·`handoff/`.

## 처음 한 번

맥은 터미널에서 `bash setup.sh`, Windows는 `setup.bat`(cmd·PowerShell·더블클릭). 여러 번 실행해도 된다.

- 공통(백엔드·프론트와 같음): Git 확인, Claude Code·Codex 감지(둘 다 없으면 Claude Code 설치), 이 저장소의 git 이름·메일, 커밋 훅(`.githooks`), 이니셜 브랜치
- 러스트: C++ 빌드 도구(Windows), rustup·stable·rustfmt·clippy, `.env.local` 키(디스코드에서 받음, 목록은 `.env.local.example`), 대조 테스트용 옆 폴더 `nyangmeong-ai`와 `.venv`(폴더 이름이 `ai`·`nyangmeong-ai`가 아니면 `AI_REPO=<경로>`)
- 확인: `cargo build`, 빈 포트로 띄워 `/health`

`[직접]` 줄이 남으면 종료 코드 3이다. 그 일을 하고 다시 실행한다.

## 실행·검증

```sh
cargo run                 # http://localhost:8100/health → {"status":"ok"}
sh scripts/check.sh       # fmt · clippy · test · Python↔러스트 대조 테스트
```

## 작업 흐름 (AI 도구)

`/start` 기획(Opus) → `승인` → `/impl` 구현(Sonnet)과 별도 에이전트 검증(Opus, 최대 3회) → 사람 확인 → `/end`. 규칙은 `AGENTS.md`.

## 배포

`main`에 PR이 머지되면(또는 Actions에서 `Deploy Character` 수동 실행) 러너가 빌드해 2번 인스턴스에 올리고 systemd `nyangmeong-character`를 재시작한다(`.github/workflows/main.yml`, `deploy/`).
