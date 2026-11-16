# LJS 핸드오프

## 지금 하는 것

M3 이미지 계산 — 끝 (2026-11-16, 검증 2회차 `확인 못 함 있음`, 사람 확인 `확인`)

- 원본 `nyangmeong-ai` e9c3edd의 `engine`·`canvas`·`bake`·`compose` 이미지 계산을 `src/pil.rs`·`ndimage.rs`·`engine.rs`·`canvas.rs`·`bake.rs`·`compose.rs`로 옮겼다. 아직 아무 경로도 부르지 않는다(M4)
- 원본 Python과 화소 단위로 같다: 합성 시트 10 · 아이템 그림 6 · 사진 정규화 20 · scipy 마스크 8, 이 PC의 실제 시트(맨몸 1 + 착용 5)와 임의 크기 사진 31장. 정답은 원본 venv(Pillow 13.0.0·numpy 2.5.4·scipy 1.18.1)가 만들었다
- Pillow 13 소스(`Resample.c`·`Reduce.c`)에 맞춘 것: `thumbnail`은 세로가 가로보다 두 배 넘게 줄면 세로 먼저, `reduce`는 f32 승수와 `>> 24`, 필터 계수는 `double` 나눗셈
- 새 의존성 `image` 0.25.10(`png`·`jpeg`·`webp`). `Cargo.toml`에 `[profile.dev] opt-level = 2`(테스트 125초 → 11초). `build.yml`에서 `image_bench`를 빌드해 Actions 산출물로 올린다
- 합성 정답은 `tests/fixtures/image/`(4.3MB, 실제 그림 아님)에 커밋, 다시 만들기 `sh scripts/gold.sh synth`. 사진 정규화 정답은 크기·CRC32만 둔다. 정답 float는 repr 문자열(serde_json 기본 실수 읽기가 마지막 한 비트를 틀림)
- 실제 시트 비교: `CHARACTER_SHEETS_DIR=<bare.png·worn_<부위코드>.png 폴더> sh scripts/check.sh`(정답은 `target/character-gold`, 커밋 안 함). 이번 시트는 DB에 등록되지 않은 로컬 파일 34장 중 맨몸 1(나머지와 차이가 가장 작은 것으로 추정)과 착용 5이고 부위 이름은 짐작이다(비교에는 영향 없음). M0 측정용 캐릭터와 같은지는 모른다
- 알려진 차이(`docs/design/migration.md`): PNG 바이트, JPEG·손실 WebP 화소, 사진 방향은 EXIF 청크만(XMP·`Raw profile type exif`는 안 읽음)

### 측정 (실제 시트 1024×1024, 이 PC, 반복 3회 중 가장 빠른 시간, 힙은 할당량 최고치)

| 작업 | 러스트 시간(ms) | 러스트 힙(MB) | Python 시간(ms) | Python 힙(MB) |
| --- | --- | --- | --- | --- |
| stand_front_cut(맨몸) | 51.9 | 18.0 | 275.3 | 95.4 |
| place | 4.2 | 1.4 | 87.5 | 5.1 |
| validate(MYPC0002~0005) | 113~119 | 22.5 | 653~770 | 95.8 |
| render(착용 5개 전부) | 459.0 | 23.9 | 2351.7 | 97.6 |

## 다음에 할 것

- 2번 서버 최대 메모리: PR의 Actions `build` 실행에서 `image_bench` 산출물을 받아 2번에 올리고 `chmod +x image_bench && /usr/bin/time -v ./image_bench <시트 폴더>`의 `Maximum resident set size`를 위 표 옆에 적는다(RAM 1GB 안에 여유가 있어야 함). 서버에서 cargo build 안 함
- M4 `/start M4`: 시작 전에 시크릿 `ENV_FILE`에 `BACKEND_BASE_URL`(2번에서 1번 Java 사설 주소)·`OPENAI_API_KEY`를 넣고 다시 배포한다
- M4 주의: 원본 `place`는 눈이 위쪽이면 `origin_y`로 정수 `100`을 돌려줘 `repository.py`가 Java에 `"100"`을 보낸다. 러스트 `Layout.origin_y`는 f64(`100.0`)라 Java로 보낼 때 문자열 모양을 맞춘다
- 쓰고 난 실제 시트 폴더와 `target/character-gold`는 실제 반려동물 그림이라 지운다
