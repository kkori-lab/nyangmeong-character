# LJS 핸드오프

## 지금 하는 것

M7 로그 파일 (LOG_DIR 날짜별 파일·30일 보관) — 끝 (검증 2회차 판정 `확인 못 함 있음`: 2번 확인만 남음, 사람 확인 2026-11-17). `app.*` 로그를 `LOG_DIR/character_YYYY-MM-DD.log`에도 씀, 2번은 unit이 `/var/log/nyangmeong-character`·`TZ=Asia/Seoul`. 다음은 M8(`/start M8`)

## 다음에 할 것

- 연결 남음: M7 2번 로그 폴더 — 배포 뒤 `ls -ld /var/log/nyangmeong-character; ls -l /var/log/nyangmeong-character`(폴더 `drwxrws---`, 파일 `-rw-rw---- nmchar`), `curl -s http://127.0.0.1:8100/health; tail -n 3 /var/log/nyangmeong-character/character_$(TZ=Asia/Seoul date +%F).log`에 `INFO app.request - GET /health -> 200`, 줄 시각이 한국 시각 (LJS)
- 다음 작업: M8 개발 환경 설치(`/start M8`) — 백엔드·프론트 `setup.sh` 공통 절 + 러스트 절, DMS MCP 제외 (2026-11-17 요청)
- 연결 남음: M6 굽기 중 최대 메모리 — 2번에서 `systemctl show nyangmeong-character -p MemoryPeak`·`journalctl -k | grep -i oom` 결과를 `migration.md` "M6 전환 절차" 결과 줄에 (LJS)
- 연결 남음: M6 아이템 49·51·53 그림 참고 굽기 실패(`BackendError`, Java `GET /internal/ai/items/{n}/bake-image` 또는 아이템 그림 없음 추정) — 백엔드에서 /start
- 연결 남음: M6 관리자 아이템 등록·전체 굽기 사이트 확인, 1번 `ops/m0.sh character` 스왑 없음 (LJS)
- 연결 남음: M6 공지(사람, 전환은 됨): 백엔드·프론트 팀원에게 "캐릭터·아이템 경로 8개(`/ai/characters`·`/ai/characters/{n}/preview`·`/ai/characters/{n}/equipped-image`·`/ai/items/bake-hints`·`/ai/items/pixel-image`·`/ai/items/trial-preview`·`/ai/items/{n}/bake`·`/ai/admin/characters/jobs/{n}/regenerate`)가 2026-11-17 12:02부터 2번 러스트 서버로 갑니다. 요청·응답 계약(ApiResponse·오류 코드·Java 내부 API 호출)은 그대로입니다. 문제가 보이면 알려 주세요. nginx 블록만 지우면 바로 Python으로 돌아갑니다"
- 로그: Java 응답 모양이 틀리거나 Java가 4xx로 답하면 `굽기 대상 조회 실패` 한 줄만 남고 이유가 없다(Python은 traceback에 `AppError`만). 이유 한 줄을 붙일지는 Python과 같이 정한다
- OpenAI 키 교체: 2026-11-17 확인 중 키가 대화 기록에 노출됐다. 대시보드에서 폐기·새로 발급하고 `.env.local`·`ENV_FILE`을 바꾼다(LJS). 크레딧은 충전함(소진 시 OpenAI 429 `insufficient_quota`, 로그에는 `RateLimitError`만 남음)
- 결정 대기: 크레딧 부족 알림 — (가) `src/clients.rs`에서 429 본문 `code`를 읽어 운영 로그에만 남김(추천, 응답·Java 보고 그대로) (나) 실패 사유를 새 값으로 Java에 보고(Python과 결과가 달라져 M6 뒤). 회원 화면에는 노출하지 않는다
- 2번 서버 최대 메모리: Actions `build`의 `image_bench` 산출물을 2번에 올려 `chmod +x image_bench && /usr/bin/time -v ./image_bench <시트 폴더>`의 `Maximum resident set size`를 `migration.md` M3 측정 표에 적는다(RAM 1GB 안에 여유). 서버에서 cargo build 안 함
- 쓰고 난 실제 시트 폴더와 `target/character-gold`는 실제 반려동물 그림이라 지운다
