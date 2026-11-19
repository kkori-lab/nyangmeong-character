# LJS 핸드오프

## 지금 하는 것

M12 관리자 무료 재생성 실제 흐름 확인 — 끝 (검증 1회차 `확인 못 함 있음`(사람 확인만 남음), 사람 확인 2026-11-19). `재생성 시작 실패`·`재생성 원본 사진 조회 실패` 로그 끝에 이유(`service::cause`, 예: `Java 409`), 테스트 `tests/service_regen_logs.rs`. 로컬 Java(팀 개발 DB)·러스트 서버로 ①~⑤ 모두 통과. 다음은 "다음에 할 것" 맨 위

## 다음에 할 것

- ai 세션 답장: FN-MYP-010-23 관리자 무료 재생성 확인 결과(2026-11-19, 러스트 서버) — ① 관리자 202 통과 ② 재생성 작업 822 실패 줄 없이 생성·success·굽기 통과 ③ 작업 822 완료·캐릭터 625 새 이미지 통과 ④ 같은 번호(820) 재호출 202·`재생성 시작 실패: charCrtJobSn=820: Java 409` 한 줄 통과 ⑤ 회원 403 통과. 참고: 시드 캐릭터 600은 원본 파일이 Java 저장 폴더에 없어 original-photo 404(작업 820 INPUT_ERROR). 시험 데이터(문의 235·237·238, 작업 810·820·821·822, 캐릭터 621·600·625) 정리 여부도 같이 (LJS)
- 연결 남음: M9 2번 사유 로그 — 다음 실제 크레딧 소진 때 2번 `/var/log/nyangmeong-character/character_<날짜>.log`에 `OpenAI 429 사유: insufficient_quota` 줄이 `… 실패: RateLimitError` 바로 앞에 나오는지 (LJS)
- 연결 남음: M8 맥 확인 — 맥 터미널에서 `bash setup.sh` → rustup이 없으면 sh.rustup.rs로 설치·`~/.zshrc`에 `~/.cargo/bin` PATH, 끝에 `[완료] 서버 기동`, 두 번째 실행 종료 코드 0. Windows 더블클릭 시 끝에서 창이 멈추는지도 (LJS)
- 연결 남음: M8 백엔드·프론트 `setup.sh`·`setup.bat` 머리 주석 "고칠 때 둘 다" → "셋 다(캐릭터 포함)" (백엔드·프론트 저장소에서 /start)
- 연결 남음: M7 2번 로그 폴더 — 배포 뒤 `ls -ld /var/log/nyangmeong-character; ls -l /var/log/nyangmeong-character`(폴더 `drwxrws---`, 파일 `-rw-rw---- nmchar`), `curl -s http://127.0.0.1:8100/health; tail -n 3 /var/log/nyangmeong-character/character_$(TZ=Asia/Seoul date +%F).log`에 `INFO app.request - GET /health -> 200`, 줄 시각이 한국 시각 (LJS)
- 연결 남음: M6 굽기 중 최대 메모리 — 2번에서 `systemctl show nyangmeong-character -p MemoryPeak`·`journalctl -k | grep -i oom` 결과를 `migration.md` "M6 전환 절차" 결과 줄에 (LJS)
- 연결 남음: M6 아이템 49·51·53 그림 참고 굽기 실패(`BackendError`, Java `GET /internal/ai/items/{n}/bake-image` 또는 아이템 그림 없음 추정) — 백엔드에서 /start
- 연결 남음: M6 관리자 아이템 등록·전체 굽기 사이트 확인, 1번 `ops/m0.sh character` 스왑 없음 (LJS)
- 연결 남음: M6 공지(사람, 전환은 됨): 백엔드·프론트 팀원에게 "캐릭터·아이템 경로 8개(`/ai/characters`·`/ai/characters/{n}/preview`·`/ai/characters/{n}/equipped-image`·`/ai/items/bake-hints`·`/ai/items/pixel-image`·`/ai/items/trial-preview`·`/ai/items/{n}/bake`·`/ai/admin/characters/jobs/{n}/regenerate`)가 2026-11-17 12:02부터 2번 러스트 서버로 갑니다. 요청·응답 계약(ApiResponse·오류 코드·Java 내부 API 호출)은 그대로입니다. 문제가 보이면 알려 주세요. nginx 블록만 지우면 바로 Python으로 돌아갑니다"
- OpenAI 키 교체: 2026-11-17 확인 중 키가 대화 기록에 노출됐다. 대시보드에서 폐기·새로 발급하고 `.env.local`·`ENV_FILE`을 바꾼다(LJS). 크레딧은 충전함(소진 시 OpenAI 429 `insufficient_quota`, 로그에는 `RateLimitError`만 남음)
- 2번 서버 최대 메모리: Actions `build`의 `image_bench` 산출물을 2번에 올려 `chmod +x image_bench && /usr/bin/time -v ./image_bench <시트 폴더>`의 `Maximum resident set size`를 `migration.md` M3 측정 표에 적는다(RAM 1GB 안에 여유). 서버에서 cargo build 안 함
- 쓰고 난 실제 시트 폴더와 `target/character-gold`는 실제 반려동물 그림이라 지운다
