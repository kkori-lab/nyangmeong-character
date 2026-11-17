# nyangmeong-character

냥멍케어 캐릭터·아이템 생성 서버(러스트). `nyangmeong-ai`(Python)의 `app/character/`를 2번 인스턴스로 옮기는 중이다. 배경·결정·서버 정보는 `docs/design/migration.md`, 진행은 `milestones/`·`handoff/`.

## 처음 한 번

```sh
# 러스트 (Windows는 Visual Studio C++ Build Tools도 필요)
rustup default stable
# 커밋 메시지에서 AI 도구 공동 작성자 줄을 지우는 훅
git config core.hooksPath .githooks
# 대조 테스트용 원본 : 옆 폴더에 nyangmeong-ai를 클론하고 그 저장소에서 uv sync
# (폴더 이름이 ai·nyangmeong-ai가 아니면 AI_REPO=<경로>)
# 설정
cp .env.local.example .env.local
```

## 실행·검증

```sh
cargo run                 # http://localhost:8100/health → {"status":"ok"}
sh scripts/check.sh       # fmt · clippy · test · Python↔러스트 대조 테스트
```

## 작업 흐름 (AI 도구)

`/start` 기획(Opus) → `승인` → `/impl` 구현(Sonnet)과 별도 에이전트 검증(Opus, 최대 3회) → 사람 확인 → `/end`. 규칙은 `AGENTS.md`.

## 배포

`main`에 PR이 머지되면(또는 Actions에서 `Deploy Character` 수동 실행) 러너가 빌드해 2번 인스턴스에 올리고 systemd `nyangmeong-character`를 재시작한다(`.github/workflows/main.yml`, `deploy/`).
