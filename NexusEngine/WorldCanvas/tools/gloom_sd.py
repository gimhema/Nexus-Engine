"""고딕 다크 SD (남) — 두건 쓴 등불지기. 어두운 분위기 게임용 화풍 시험 샘플.

    python3 tools/gloom_sd.py        # sprites/gloom_m.canvas 를 쓴다 (손으로 고치지 말고 이 파일을 고친다)

화풍 — 기존 SD(애니메 미소녀풍)·마스코트 SD(스티커풍)와 다른 점:
- 얼굴을 숨긴다: 큰 두건 안은 칠흑, 빛나는 두 눈만.
- 조명이 분위기를 만든다: 차가운 푸른 **역광 테두리**(왼쪽 위 가장자리 1~2px), 손에 든 **등불**이 망토를 주황으로
  물들이고(등불 둘레 원 안의 망토 색만 따뜻한 색으로) 반투명 빛무리(빈 자리에만 — `stamp` 의 반투명 규칙)를 둘린다.
- 색은 청회색 한 갈래 + 등불의 따뜻한 색만. 그늘 쪽은 체크 디더로 거친 질감, 망토 아랫단은 해진 지그재그.
- 움직임: 걷기에서 해진 단이 엇갈려 흔들리고 등불이 앞뒤로 흔들리며 깜빡인다.

명암 요령 — 도형 하나를 `mask 기본색` 안에서 두 번 칠한다:
  역광: `rect 전체 R` → 같은 도형을 오른쪽 아래로 1~2px 비켜 기본색  ⇒ 왼쪽 위 가장자리만 R
  그늘: `rect 전체 그늘` → 같은 도형을 왼쪽 위로 비켜 기본색      ⇒ 오른쪽 아래 초승달
"""
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "sprites" / "gloom_m.canvas"

PALETTE = """palette
    o #0b0a12
    a #4b5066
    k #343849
    h #343849
    c #24262f
    d #16161d
    R #93a9cf
    w #6e5446
    x #4a3834
    q #4a4450
    z #2e2930
    D #06050a
    E #ffe08a
    F #ff9a3c
    M #45434a
    m #222126
    G #ffc865
    Y #fff4c8
    S #c8b8b0
    s #8a7a78
    f #2b2420
    g #191513
    V #ffb45a30
    U #ffb45a60
end
"""


def pts(points, dx=0, dy=0):
    return " ".join(f"{x + dx} {y + dy}" for x, y in points)


def lit(shape, base, shadow, rim=True, sx=-3, sy=-2):
    """도형 `shape(dx, dy)` 를 기본색으로 칠한 뒤 역광 테두리와 그늘 초승달."""
    out = [shape(0, 0, base)]
    if rim:
        out += [f"    mask {base}", "        rect 0 0 112 112 R", shape(2, 1, base).replace("    ", "        ", 1), "    end"]
    out += [f"    mask {base}", f"        rect 0 0 112 112 {shadow}", shape(sx, sy, base).replace("    ", "        ", 1),
            "    end"]
    return out


def hem(x0, x1, y, phase, step=6):
    """해진 아랫단 — 오른쪽에서 왼쪽으로 지그재그 (phase 로 엇갈림)."""
    out = []
    for i, x in enumerate(range(x1, x0 - 1, -step)):
        out.append((x, y + (5 if (i + phase) % 2 else 0) + (1 if i % 3 == 1 else 0)))
    return out


def cloak_s(b, phase, back=False):
    top = 58 + b
    body = [(42, top), (70, top), (77, top + 12), (83, 96 + b)]
    body += hem(29, 83, 97 + b, phase)
    body += [(29, 96 + b), (35, top + 12)]

    def shape(dx, dy, c):
        return f"    poly {c} {pts(body, dx, dy)}"
    out = lit(shape, "k", "c")
    # 그늘 쪽 거친 질감 · 아랫단 짙은 디더 · 앞 여밈(앞모습)
    out += [f"    dither k c 30 {84 + b} 54 14", f"    dither c d 30 {92 + b} 56 12",
            f"    ellipse 40 {top - 2} 32 8 d"]
    if not back:
        out += [f"    line 56 {top + 6} 55 {97 + b} d", f"    line 57 {top + 8} 57 {95 + b} c",
                f"    rect 54 {top + 3} 4 3 M", f"    px 54 {top + 3} R"]
    else:
        out += [f"    poly c 46 {top} 66 {top} 56 {top + 16}"]  # 두건 자락
    return out


def cloak_e(b, phase):
    top = 58 + b
    body = [(46, top), (64, top), (71, top + 12), (78, 96 + b)]
    body += hem(33, 78, 97 + b, phase)
    body += [(33, 96 + b), (40, top + 12)]

    def shape(dx, dy, c):
        return f"    poly {c} {pts(body, dx, dy)}"
    out = lit(shape, "k", "c")
    out += [f"    dither k c 33 {84 + b} 46 14", f"    dither c d 33 {92 + b} 48 12",
            f"    line 48 {top + 4} 42 {96 + b} c"]
    return out


def hood(view, b):
    """두건 — 크고 둥글며 끝이 뒤로 처진다. 앞·옆은 칠흑 구멍에 빛나는 눈."""
    out = []
    if view == "e":
        tip = [(38, 22 + b), (20, 18 + b), (12, 30 + b), (24, 26 + b)]
        out += [f"    poly c {pts(tip)}", f"    line 21 {19 + b} 37 {22 + b} R"]

        def shape(dx, dy, c):
            return f"    ellipse {29 + dx} {12 + b + dy} 56 56 {c}"
        out += lit(shape, "h", "c")
        out += [f"    ellipse 55 {27 + b} 30 40 a", f"    ellipse 58 {30 + b} 28 36 D",
                f"    rect 70 {45 + b} 7 3 E", f"    px 70 {45 + b} F", f"    px 76 {47 + b} F",
                f"    line 52 {40 + b} 55 {64 + b} c"]
        return out
    def shape(dx, dy, c):
        return f"    ellipse {26 + dx} {12 + b + dy} 60 56 {c}"
    out += lit(shape, "h", "c")
    # 두건 끝 — 덩어리 명암 뒤에 얹는다 (먼저 그리면 역광 칠에 통째로 물든다)
    tip = [(48, 18 + b), (58, 0 + b), (68, 16 + b)]
    out += [f"    poly h {pts(tip)}", f"    line 49 {17 + b} 57 {2 + b} R", f"    line 59 {2 + b} 66 {15 + b} c"]
    if view == "s":
        out += [f"    ellipse 35 {27 + b} 42 40 a", f"    ellipse 37 {29 + b} 38 36 D",
                # 눈 — 조금 치켜 올라간 두 점, 바깥 끝은 주황
                f"    rect 44 {46 + b} 7 3 E", f"    px 44 {47 + b} F", f"    px 50 {46 + b} Y",
                f"    rect 61 {46 + b} 7 3 E", f"    px 67 {47 + b} F", f"    px 61 {46 + b} Y",
                f"    line 56 {12 + b} 56 {27 + b} c"]
    else:
        out += [f"    line 56 {6 + b} 56 {60 + b} c", f"    line 44 {20 + b} 40 {58 + b} c",
                f"    line 68 {20 + b} 72 {58 + b} c"]
    return out


def lantern(x, y, flick):
    """손에 든 등불 — 손잡이·덮개·유리·불씨. (x, y) 는 손 자리."""
    return [f"    ellipse {x - 4} {y - 3} 8 7 S", f"    px {x + 2} {y + 2} s",
            f"    line {x} {y + 3} {x} {y + 6} M",
            f"    poly M {x - 6} {y + 7} {x + 6} {y + 7} {x} {y + 3}",
            f"    rect {x - 5} {y + 7} 10 12 M", f"    rect {x - 3} {y + 9} 6 8 G",
            f"    rect {x - 1} {y + 11 - flick} 2 {3 + flick} Y",
            f"    line {x - 5} {y + 7} {x - 5} {y + 18} m", f"    rect {x - 5} {y + 19} 10 2 m"]


def warm(cx, cy, r, inner=True):
    """등불 빛 — 두 단계. 바깥 원은 살짝 따뜻한 회색, 안쪽 원(반지름 절반)만 주황빛 (경계를 체크 디더로 섞는다)."""
    def circle(rad):
        return f"{cx - rad} {cy - rad} {2 * rad} {2 * rad}"
    out = ["    mask ahk", f"        ellipse {circle(r)} q", "    end", "    mask cd", f"        ellipse {circle(r)} z", "    end"]
    if inner:
        h = r // 2
        out += ["    mask q", f"        ellipse {circle(h + 3)} w", "    end", "    mask z", f"        ellipse {circle(h + 3)} x", "    end",
                # 안쪽 원 가장자리 3px 를 바깥색과 체크로 섞는다
                "    mask w", f"        ellipse {circle(h + 3)} q", f"        ellipse {circle(h)} w", "    end",
                f"    dither q w {cx - h - 3} {cy - h - 3} {2 * h + 6} {2 * h + 6}"]
    return out


def feet(view, left, right):
    """발 — (x, 들어 올림) 둘. 옆모습은 먼 발이 짙다."""
    (lx, ll), (rx, rl) = left, right
    far = "g" if view == "e" else "f"
    return [f"    ellipse {lx} {98 - ll} 12 7 {far}", f"    ellipse {rx} {98 - rl} 12 7 f",
            f"    px {rx + 3} {99 - rl} M"]


def frame(name, view, bob, phase, left, right, lan, flick, halo):
    out = [f"frame {name}"]
    if view == "e":
        out += feet("e", left, right)
        out += cloak_e(bob, phase)
        lx, ly = 80 + lan, 70 + bob
        out += lantern(lx, ly, flick)
        out += [f"    ellipse 66 {64 + bob} 16 12 k", f"    ellipse 67 {64 + bob} 14 10 a"]  # 소매
        out += hood("e", bob)
        out += warm(lx, ly + 13, 24)
    else:
        out += feet(view, left, right)
        lx, ly = 70 + lan, 70 + bob
        if view == "n":
            out += lantern(lx, ly, flick)  # 몸 뒤 — 망토가 가린다
        out += cloak_s(bob, phase, back=view == "n")
        if view == "s":
            out += [f"    ellipse 60 {62 + bob} 16 12 k", f"    ellipse 61 {62 + bob} 14 10 a"]
            out += lantern(lx, ly, flick)
        out += hood(view, bob)
        out += warm(lx, ly + 13, 24 if view == "s" else 14, inner=view == "s")
    out.append("    outline o")
    # 빛무리 — 반투명이라 빈 자리에만 얹힌다 (몸 위에 구멍이 나지 않게)
    out.append(f"    stamp {halo} {lx - 22} {ly - 9}")
    out.append("end")
    return out


def main():
    lines = ["canvas 1", "cell 112 112", "",
             "// 고딕 다크 SD (남) — tools/gloom_sd.py 가 만든다. 손으로 고치지 말고 생성기를 고친다.",
             "// 화풍 설명은 생성기 머리말.", "", PALETTE,
             "part halo_a 44 44", "    ellipse 0 0 44 44 V", "    ellipse 10 10 24 24 U", "end",
             "part halo_b 44 44", "    ellipse 3 3 38 38 V", "    ellipse 12 12 20 20 U", "end", ""]
    clips = {
        # 대기 — 숨쉬기 1px, 등불 깜빡임
        "idle": [dict(bob=0, phase=0, lf=0, rf=0, lan=0, flick=0, halo="halo_a"),
                 dict(bob=1, phase=0, lf=0, rf=0, lan=0, flick=1, halo="halo_b")],
        # 걷기 — 0 두 발 · 1 왼발 듦 · 2 두 발 · 3 오른발 듦. 단이 엇갈리고 등불이 흔들린다
        "walk": [dict(bob=0, phase=0, lf=0, rf=0, lan=0, flick=0, halo="halo_a"),
                 dict(bob=-2, phase=1, lf=4, rf=0, lan=1, flick=1, halo="halo_b"),
                 dict(bob=0, phase=0, lf=0, rf=0, lan=0, flick=0, halo="halo_a"),
                 dict(bob=-2, phase=1, lf=0, rf=4, lan=-1, flick=1, halo="halo_b")],
    }
    # 옆모습 발 — (먼 발 x, 가까운 발 x): 0 가까운 발 앞 · 1·3 지나감 · 2 가까운 발 뒤
    side_x = [(44, 58), (50, 52), (58, 44), (50, 52)]
    for clip, frames in clips.items():
        for view in "sne":
            for i, f in enumerate(frames):
                if view == "e":
                    fx, nx = side_x[i] if clip == "walk" else (46, 54)
                    left, right = (fx, f["rf"]), (nx, f["lf"])
                else:
                    left, right = (42, f["lf"]), (58, f["rf"])
                lines += frame(f"{clip}_{view}_{i}", view, f["bob"], f["phase"], left, right, f["lan"], f["flick"],
                               f["halo"])
                lines.append("")
        n = len(frames)
        names = " ".join(f"{clip}_e_{i}" for i in range(n))
        lines += [f"frames {clip}_w_ {n} = [{names}]", "    flip_h", "end", ""]
    lines += ["sheet", "    directions 4", "    tinted false", "    pixels_per_meter 32"]
    for clip, ms in (("idle", 500), ("walk", 170)):
        n = len(clips[clip])
        lines.append(f"    clip {clip.capitalize()} {ms} loop")
        for view in "enws":
            lines.append("        row " + " ".join(f"{clip}_{view}_{i}" for i in range(n)))
        lines.append("    end")
    lines += ["end", "", "output", "    image ../../WorldEditor/assets/worldcanvas/gloom_m.png",
              "    sheet ../../WorldEditor/assets/worldcanvas/gloom_m.sheet.ron", "end", ""]
    text = "\n".join(lines)
    if "--print" in sys.argv:  # tools/check_generators.py 가 비교한다
        sys.stdout.write(text)
        return
    OUT.write_text(text)
    print(OUT)


if __name__ == "__main__":
    main()
