#!/bin/sh
# Python↔러스트 대조 테스트(contract/). check.sh가 부른다.
# nyangmeong-ai 저장소(AI_REPO, 없으면 ../ai·../nyangmeong-ai)의 가상환경으로 가짜 Java·Python 서버를 띄우고,
# 러스트 서버와 같은 요청을 보내 비교한 뒤 셋 다 끈다. 포트는 빈 포트를 고른다.
# 사용: sh scripts/contract.sh   (AI_REPO=<nyangmeong-ai 경로> 로 위치를 줄 수 있다)
cd "$(git rev-parse --show-toplevel)" || exit 1
PATH="$HOME/.cargo/bin:$PATH"

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

cargo build -q || exit 1
exec "$py" -I contract/run.py "$ai"
