"""손으로 짠 원본의 옆모습 다리를 골격(엉덩이→무릎→발목)으로 — tools/sd_template.py 의 골격을 빌려 쓴다.

    python3 tools/skeleton_legs.py dark_knight_v2 --write
    python3 tools/skeleton_legs.py forest_witch_v4 --write

원본의 「골격 다리」 절(// ── 골격 다리 … ── ~ // ── 골격 다리 끝 ──)에 자세별 다리 겹
`l_leg_e{n,f}_{i,w0..w3}` 을 쓰고, 걷기 프레임에서 몸을 얼마나 내려야 하는지(엉덩이 높이)를 출력한다 —
원본의 `frames walk_e_` 에서 몸·머리·팔의 dy 를 그 값으로 맞춘다.
다리 하나는 바지 자리(정강이받이·스타킹) + 장화 자리 두 재질을 한 겹에 겹친다.
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import sd_template as T  # noqa: E402

# 원본마다 — 다리 기하(sd_template 체형 수치와 같은 뜻)와 재질 문자
PRESETS = {
    # 흑갑 정강이받이 + 같은 갑옷 발, 금 테
    "dark_knight_v2": dict(
        body=dict(h=56, head_y=0, tl=16, lw=5, bh=4, stride=3),
        mats={"pants": ("D", "armor", "B", "E", None), "boot": ("C", "armor", "g")},
    ),
    # 줄무늬 스타킹 + 나무색 장화, 금 테
    "forest_witch_v4": dict(
        body=dict(h=56, head_y=0, tl=15, lw=4, bh=4, stride=3),
        mats={"pants": ("c", "cream", "B", "c", "B"), "boot": ("M", "wood", "g")},
    ),
}


def section(name):
    pre = PRESETS[name]
    b = pre["body"]
    out = ["// ── 골격 다리 — tools/skeleton_legs.py 로 생성 (자세: 대기 i, 걷기 w0~w3) ──"]
    for pose in ("i", "w0", "w1", "w2", "w3"):
        for near, nf in ((True, "n"), (False, "f")):
            lines = []
            for what in ("pants", "boot"):
                part = T.leg_side_pose(b, pose, near, what, pre["mats"])
                lines += [ln for ln in part if ln != "    outline o"]
            out += [f"frame l_leg_e{nf}_{pose}", *lines, "    outline o", "end"]
    out.append("// ── 골격 다리 끝 ──")
    return "\n".join(out) + "\n"


def main():
    name = sys.argv[1]
    text = section(name)
    # 고해상도 원본(tools/upres.py)이면 같은 배율로
    src_path = Path(__file__).resolve().parent.parent / "sprites" / f"{name}.canvas"
    cell = int(src_path.read_text().split("\ncell ", 1)[1].split()[0])
    if cell > 56:
        text = T.upscale(text, cell // 56)
    off = T.side_offsets(PRESETS[name]["body"])
    print(f"몸 오르내림 — 대기 {off['i']}, 걷기 [{' '.join(str(off[f'w{i}']) for i in range(4))}]")
    if "--write" not in sys.argv:
        sys.stdout.write(text)
        return
    path = Path(__file__).resolve().parent.parent / "sprites" / f"{name}.canvas"
    src = path.read_text()
    head = "// ── 골격 다리 — tools/skeleton_legs.py"
    end = "// ── 골격 다리 끝 ──\n"
    if head in src:
        a = src.index(head)
        src = src[:a] + text + src[src.index(end) + len(end):]
    else:
        # 처음이면 옛 다리 겹(l_leg_en) 바로 앞에
        a = src.index("frame l_leg_en\n")
        src = src[:a] + text + "\n" + src[a:]
    path.write_text(src)


if __name__ == "__main__":
    main()
