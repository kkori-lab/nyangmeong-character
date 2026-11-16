# 핸드오프

공개 저장소다. 서버 IP·포트 외 접속 정보·키·개인 PC 경로를 적지 않는다.

## 왜 이 저장소인가

- 1번 인스턴스(OCI Micro, Java + Python + nginx)에서 캐릭터 생성 뒤 전체 굽기가 CPU를 100%로 약 5분 잡아(사람 기억, 미측정) 사이트가 느려진다.
- 원인(코드 확인, `nyangmeong-ai` `app/character/service.py`): 생성 직후 모든 아이템을 굽기 큐에 넣고, `BAKE_CONCURRENCY = 13`개를 동시에 돌리며, 굽기마다 numpy·scipy 이미지 처리를 스레드에서 돌린다.
- 결정(2026-11-14): 캐릭터 경로를 2번 인스턴스(OCI Micro, 무료)로 분리하고, 서버는 러스트로 새로 짠다(학원 수업과 연계한 학습 목적). Python `app/character/`는 이전이 끝날 때까지 그대로 둔다.

## 지금 상태

- 2번 인스턴스: 생성 완료. 홈 리전(오사카), `VM.Standard.E2.1.Micro`, Ubuntu 24.04 Minimal(x86_64), 부트 볼륨 47GB(Balanced, 백업 없음), 1번과 같은 VCN·서브넷, 임시 공인 IP(SSH 전용, 고정 IP 안 받음). `nano`·`iptables-persistent` 설치. 스왑은 M4에서 측정 후 결정
- 2번 SSH 키: 맥에서 만든 키만 등록. 학원 PC로 2번에 들어가려면 학원 PC에서 키를 따로 만들고, 맥으로 2번에 접속해 `~/.ssh/authorized_keys`에 그 공개키 한 줄을 덧붙인다(덮어쓰지 말 것)
- 코드: `LJS` 브랜치 `cb698b1` — axum 0.8 + tokio, `GET /health` → `ok`, 포트는 환경변수 `PORT`(기본 8100), `0.0.0.0` 바인드. 로컬 `cargo build`·`clippy -D warnings`·`fmt --check`·`curl` 200 확인
- 크로스 컴파일(맥): `brew install zig`, `cargo install cargo-zigbuild`, `rustup target add x86_64-unknown-linux-gnu` 후 `cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.39`(서버 glibc 2.39). 맥에서 빌드 성공(ELF x86-64, 1.2MB). 서버 실행은 아직 안 함

## 다음에 할 것 (순서대로)

1. (학원 PC, 1번 SSH) `sudo ss -tlnp | grep java`로 Java가 받는 주소·포트 확인. `127.0.0.1`이면 2번에서 닿지 않으므로 Java `server.address` 변경을 백엔드와 정한다(되돌리기 어려운 결정)
2. (OCI 콘솔) 1번·2번 사설 IP 확인(`10.0.0.x`인지). 보안 목록에 수신 규칙 2줄: `<1번 사설 IP>/32` → TCP 8100(nginx→러스트), `<2번 사설 IP>/32` → TCP `<Java 포트>`(러스트→Java). `0.0.0.0/0` 금지 — 서버 간 호출은 키 없이 한다
3. (1번 SSH) iptables에 `-s <2번 사설 IP> --dport <Java 포트> -j ACCEPT`를 `REJECT` 줄 앞에 `-I`로 넣고 `sudo netfilter-persistent save`
4. (맥) 실행 파일을 2번에 `scp`, 2번에서 `./nyangmeong-character &` 후 `curl -i localhost:8100/health` → 200
5. (맥, 2번 SSH) iptables에 `-s <1번 사설 IP> --dport 8100 -j ACCEPT`를 같은 방식으로 넣고 저장
6. 확인: 1번에서 `curl -I http://<2번 사설 IP>:8100/health` → 200, 맥에서 `curl --max-time 5 http://<2번 공인 IP>:8100/health` → 시간 초과, 2번에서 `curl --max-time 5 http://<1번 사설 IP>:<Java 포트>/` → 응답 옴. 여기까지가 M1 끝

## 마일스톤

| # | 내용 | 완료 조건 | 상태 |
|---|---|---|---|
| M1 | 2번 서버 준비, 러스트 뼈대 `GET /health` | 1번에서 2번 사설 IP로 `/health` 200, 공인 IP로는 닿지 않음 | 진행 (뼈대·크로스 컴파일 끝, 네트워크 남음) |
| M2 | 공통 계약: `ApiResponse` 껍데기, `X-Requested-With` 검사, Java 인증 확인과 오류(본문·`Set-Cookie`) 그대로 전달 | 같은 요청을 Python과 러스트에 보내 상태 코드·본문·`Set-Cookie`가 같음 | 대기 |
| M3 | 굽기 CPU 계산(`cut_item`, `stand_front_cut` 등) | Python 결과 PNG(정답 파일)와 픽셀 단위로 같음, 실행 시간 비교 숫자 | 대기 |
| M4 | 경로 이전과 nginx 분기 | 캐릭터 1마리 생성 → 전체 굽기 `lastYn`까지, 1번 CPU 100% 안 됨 | 대기 |

## 지뢰

- 굽기 큐는 프로세스 메모리에 있다. 큐를 쓰는 경로(`/ai/characters/**`, `/ai/items/{itemSn}/bake`, `/ai/items/trial-preview`)를 Python과 러스트로 나눠 옮기면 같은 굽기를 두 번 하고 외부 API 비용도 두 번 나간다. 한 번에 옮긴다. `bake-hints`·`pixel-image`는 따로 옮겨도 된다
- nginx(1번): 기존 `/ai`가 접두사 규칙인지 먼저 확인(`sudo nginx -T | grep -n location`). 정규식(`~`)이면 새 블록이 안 먹는다. `POST /ai/characters`는 끝 `/`가 없으므로 `location /ai/characters`(끝 `/` 없이). `proxy_set_header`·timeout은 기존 `/ai` 블록에서 그대로 복사(쿠키·`X-Requested-With`·`Set-Cookie` 전달, `pixel-image` 30~40초)
- 계약(`ApiResponse`, 캐릭터 규격 512×512·얼굴 영역, 인증 오류 전달)은 `nyangmeong-ai`·백엔드와 같아야 한다. 어긋나면 오류 없이 조용히 깨진다. 백엔드·프론트 팀원에게 캐릭터 경로가 러스트로 간다고 알린다
- `scipy.ndimage.label`의 이웃 정의(4/8방향)·반올림이 러스트 구현과 다르면 테두리가 달라진다. M3는 정답 비교 테스트부터 만든다
- 1번 인스턴스는 지우지 않는다(임시가 아닌 고정 공인 IP여도 사설 IP·설정이 바뀐다). 임시 공인 IP는 인스턴스 종료 때만 바뀐다(중지·시작은 유지, OCI 문서)
- 서버에서 `cargo build`를 돌리면 1GB 메모리로 OOM 날 수 있다. 빌드는 맥에서 크로스 컴파일
