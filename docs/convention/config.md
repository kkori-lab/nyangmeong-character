# 설정과 비밀값

설정 파일, 비밀값, 환경변수를 다룰 때와 핸드오프를 쓸 때 연다.

**이 저장소는 공개다. 한 번 커밋된 값은 지워도 이력에 남는다.**

- 키 이름은 `nyangmeong-ai`의 `Settings`와 같다(`BACKEND_BASE_URL`, `OPENAI_API_KEY`, `CHARACTER_IMAGE_MODEL`, `CHARACTER_VISION_MODEL`, `CHARACTER_TIMEOUT_SEC`). 코드는 `src/config.rs`의 `Settings`로 읽는다. 키를 추가하면 `Settings`와 `.env.local.example`을 같은 PR에서 고친다.
- 개발 PC: `.env.local`(git 제외)에 둔다. 환경변수가 `.env.local`보다 우선한다. 키 목록은 `.env.local.example`(값은 비움)에 있다. `/start`가 빠진 키를 알려 준다.
- 2번 서버: 시크릿 `ENV_FILE`에 `KEY=값` 줄로 넣는다. 배포(`main.yml`)가 매번 `/etc/nyangmeong-character.env`(root 600)로 덮어쓰고 systemd가 읽는다. 바꾼 뒤에는 다시 배포해야 반영된다.
- 서버 IP·포트 외 접속 정보, 키, 토큰, 개인 PC 경로는 파일·커밋·로그에 쓰지 않는다. 사설 IP(`10.0.0.x`)와 포트는 적어도 된다. 공인 IP는 적지 않는다(시크릿 `OCI_HOST`).
- 로그에 비밀값과 토큰을 남기지 않는다. Java로 넘기는 사용자 `Cookie` 헤더도 로그·오류 응답에 남기지 않는다(새면 그 사용자로 Java API를 부를 수 있다). 경로의 숫자는 `{n}`으로 가린다(`backend::masked`).
- 정답 파일(캐릭터 시트·사진)은 저장소 밖 폴더에 두고 경로만 환경변수로 준다.
- 작업 기록은 `handoff/<이름>.md`에 사람마다 한 파일로 두고 커밋한다. 다른 PC에서도 이어서 작업하기 위해서다. 60줄을 넘기지 않고, 오래 남길 배경·결정은 `docs/design/migration.md`로 옮긴다.
