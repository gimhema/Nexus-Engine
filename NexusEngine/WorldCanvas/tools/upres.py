"""손으로 짠 원본을 고해상도로 — 좌표를 k 배로 하고 머리는 머리 엔진으로 다시 칠한다.

    python3 tools/upres.py dark_knight_v2 2
    python3 tools/upres.py template_f 2

1. 「머리」 절을 비우고 나머지 연산의 좌표를 `sd_template.upscale()` 로 k 배 (칸·부품 크기·도형·stamp 위치·
   회전 피벗·`[ ]` 값 표·pixels_per_meter 까지).
2. 프레임 마무리(`aa o …` 앞)에 `outline o` 한 겹 — 고해상도의 바깥 윤곽 두 겹.
3. 머리 스크립트(`tools/<원본>_head.py`)를 `--res=k` 로 다시 돌려 머리 grid 를 k 배 격자에 칠한다.

`grid`·`patch` 같은 그림 블록은 좌표가 아니라 늘릴 수 없다 — 머리 밖에 있으면 멈춘다.
이미 고해상도인 원본(칸이 56 의 k 배)은 건드리지 않는다.
"""
import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import sd_template as T  # noqa: E402

HEAD = "// ── 머리 — "
END = "// ── 머리 끝 ──"


def main():
    name, k = sys.argv[1], int(sys.argv[2])
    root = Path(__file__).resolve().parent.parent
    path = root / "sprites" / f"{name}.canvas"
    src = path.read_text()
    cell = re.search(r"^cell (\d+) (\d+)", src, re.M)
    if cell and int(cell.group(1)) > 56:
        sys.exit(f"{path.name}: 이미 고해상도 (칸 {cell.group(1)}×{cell.group(2)})")
    a = src.index(HEAD)
    head_line_end = src.index("\n", a) + 1
    b = src.index(END)
    rest = src[:a] + src[b:]
    if re.search(r"^\s+(grid|patch)\b", rest, re.M):
        sys.exit(f"{path.name}: 머리 밖에 grid·patch 가 있어 늘릴 수 없다")
    before = T.upscale(src[:a], k)
    after = T.upscale(src[b:], k)
    # 바깥 윤곽 두 겹 — 프레임 마무리의 aa 앞에
    after = re.sub(r"^(    aa o )", "    outline o\n\\1", after, flags=re.M)
    before = re.sub(r"^(    aa o )", "    outline o\n\\1", before, flags=re.M)
    path.write_text(before + src[a:head_line_end] + after)
    subprocess.run(["python3", f"tools/{name}_head.py", "--write", f"--res={k}"], check=True, cwd=root)
    print(path)


if __name__ == "__main__":
    main()
