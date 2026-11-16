"""이미지 계산 측정(Python 쪽) : `examples/image_bench.rs`와 같은 폴더·같은 작업의 실행 시간과 최대 힙

원본 가상환경의 Python으로 부른다(`-I`: 현재 폴더의 파이썬 파일을 불러오지 않는다).
사용: <ai python> -I contract/image_bench.py <nyangmeong-ai 경로> <시트 폴더> [반복 횟수=3]
- 시간 : tracemalloc 없이 가장 빠른 값
- 힙   : tracemalloc 최고치(numpy 배열 버퍼 포함, 작업 시작 시점 대비). 프로세스 전체(RSS)는 `/usr/bin/time -v`의 "Maximum resident set size"
"""

import logging
import sys
import time
import tracemalloc
from pathlib import Path


def main() -> int:
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    sys.path.insert(0, sys.argv[1])
    from app.character import bake, canvas, compose, engine

    sys.stdout.reconfigure(encoding="utf-8")  # Windows 콘솔(cp949)에서 한글이 깨지지 않게
    logging.disable(logging.CRITICAL)
    folder = Path(sys.argv[2])
    repeat = int(sys.argv[3]) if len(sys.argv) > 3 else 3
    bare = (folder / "bare.png").read_bytes()
    worn = {p.stem.removeprefix("worn_"): p.read_bytes() for p in sorted(folder.glob("worn_*.png"))}

    def measure(work):
        best = float("inf")
        for _ in range(repeat):
            started = time.perf_counter()
            work()
            best = min(best, time.perf_counter() - started)
        tracemalloc.start()
        work()
        _, peak = tracemalloc.get_traced_memory()
        tracemalloc.stop()
        return best * 1000.0, peak / (1024 * 1024)

    print("작업\t시간(ms)\t최대 힙(MB)")

    def row(name, result):
        print(f"{name}\t{result[0]:.1f}\t{result[1]:.1f}")

    cut = engine.stand_front_cut(bare)
    row("stand_front_cut(맨몸)", measure(lambda: engine.stand_front_cut(bare)))
    placement = canvas.place(cut.image)
    row("place", measure(lambda: canvas.place(cut.image)))
    layout = placement.layout if placement else canvas.Layout(1.0, 0.0, 0.0)
    for regn, sheet in worn.items():
        if regn != bake.REGN_HEAD:
            row(f"validate({regn})", measure(lambda sheet=sheet, regn=regn: bake.validate(bare, sheet, regn)))
    row(f"render(착용 {len(worn)}개 전부)", measure(lambda: compose.render(bare, layout, worn)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
