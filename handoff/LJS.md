# LJS 핸드오프

## 지금 하는 것

ai 세션 답장 (FN-MYP-010-23 확인 결과) — 끝 (2026-11-19). 코드 변경 없음. ai 세션(`ai-4c`)에 확인 결과를 보냈고 그쪽 핸드오프의 FN-MYP-010-23 줄이 닫힌 것을 사람이 확인함. 시험 데이터 정리는 (가) 백엔드에 넘김(ai 핸드오프 "로컬 DB 정리 (LJS 백엔드)"에도 들어감). 다음은 "다음에 할 것" 맨 위

## 다음에 할 것

- 연결 남음: M12 로컬 시험 데이터 정리 — 문의 235·237·238, 작업 810·820·821·822, 캐릭터 621·600·625 (백엔드에서 /start)
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
