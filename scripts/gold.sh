#!/bin/sh
# 이미지 계산 정답 만들기(contract/gold_images.py). nyangmeong-ai 저장소(AI_REPO, 없으면 ../ai·../nyangmeong-ai)의 가상환경 Python으로 돈다.
# 사용: sh scripts/gold.sh          실제 시트 정답 : CHARACTER_SHEETS_DIR(bare.png, worn_<부위코드>.png) → CHARACTER_GOLD_DIR(기본 target/character-gold). 커밋하지 않는다
#       sh scripts/gold.sh synth    합성 정답(tests/fixtures/image, 커밋 대상)을 다시 만든다 : 원본 코드가 바뀌었을 때만
cd "$(git rev-parse --show-toplevel)" || exit 1

ai=
for d in "${AI_REPO:-}" ../ai ../nyangmeong-ai; do
  [ -n "$d" ] && [ -f "$d/app/main.py" ] && { ai=$d; break; }
done
[ -n "$ai" ] || { echo "nyangmeong-ai 저장소를 찾지 못했습니다. 옆 폴더에 클론하거나 AI_REPO=<경로>로 알려 주세요"; exit 1; }

py=
for p in "$ai/.venv/Scripts/python.exe" "$ai/.venv/bin/python"; do
  [ -x "$p" ] && { py=$p; break; }
done
[ -n "$py" ] || { echo "$ai에 .venv가 없습니다. 그 저장소에서 uv sync 하세요"; exit 1; }

if [ "$1" = synth ]; then
  rm -rf tests/fixtures/image/sheets tests/fixtures/image/items tests/fixtures/image/photos tests/fixtures/image/ndimage.json
  exec "$py" -I contract/gold_images.py synth "$ai" tests/fixtures/image
fi

[ -n "${CHARACTER_SHEETS_DIR:-}" ] || { echo "CHARACTER_SHEETS_DIR(bare.png, worn_<부위코드>.png가 있는 폴더)를 알려 주세요"; exit 1; }
gold=${CHARACTER_GOLD_DIR:-target/character-gold}
rm -rf "$gold"
exec "$py" -I contract/gold_images.py real "$ai" "$CHARACTER_SHEETS_DIR" "$gold"
