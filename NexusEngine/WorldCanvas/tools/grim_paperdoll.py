"""grim 종이 인형 — 오브젝트헤드 + 장비 외형. 공용 몸 하나에 겹치는 시트들을 만든다.

    cargo build --release --no-default-features    # target/release/worldcanvas 를 쓴다
    python3 tools/grim_paperdoll.py                # → WorldEditor/assets/worldcanvas/paperdoll/grim/
    python3 tools/grim_paperdoll.py --print=top_robe   # 파츠 하나의 원본 텍스트 (tools/check_generators.py)

아트 컨셉 (2026-11-15):
- **머리는 오브젝트헤드** — 캐릭터 만들기에서 고른다 (분류 `head`). 성별 없음, **몸은 공용 하나**.
- **나머지 외형은 장비** — 상의(Body)·하의(Legs)·신발(Shoes)·장갑(Hand)·무기(Weapon). 투구(Head)는 외형 없이 능력치만.
- 장비 시트는 템플릿 카탈로그에 넣지 않는다 — `data/display.ron` 아이템의 `parts` 가 이 폴더의 시트를 가리킨다.
  판금 갑옷처럼 한 장비가 여러 장(상의 + 망토)일 수 있다.

같은 칸·같은 프레임이어야 겹치므로(WorldEditor `sprites.rs` 「파츠」) 모든 파츠가 **같은 기준점**을 쓴다:
손(정면 왼 39~46·오른 67~74, 87~93), 다리(정면 44·58 폭 10, 76~98), 발(98~105), 어깨선 60, 목(깃) 50~62.
화풍·도형 도우미는 tools/grim_sd.py 와 같다 (각진 다각형, 면 셰이딩, 1px 외곽선).

앞뒤(`layer`, 동·북·서·남): 몸 0 < 하의 2 < 신발 3 < 상의 4 < 장갑 5 < 머리 6 < 무기 7 (뒷모습은 -1, 몸 뒤) ·
망토는 앞·옆 -1 / 뒷모습 8.
"""
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import grim_sd as G  # noqa: E402
from grim_sd import P, L_, tatters, sway  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT.parent / "WorldEditor" / "assets" / "worldcanvas" / "paperdoll" / "grim"
WC = Path(os.environ.get("WORLDCANVAS_BIN") or ROOT / "target" / "release" / "worldcanvas")

# 걷기 표 — grim_sd 와 같다 (bob 위아래 · phase 옷자락 · lf/rf 정면 발 들기 · arm 옆 팔 · side (먼 발, 가까운 발))
CLIPS = {
    "idle": [dict(bob=0, phase=0, lf=0, rf=0, arm=0, side=((46, 0), (52, 0))),
             dict(bob=1, phase=1, lf=0, rf=0, arm=0, side=((46, 0), (52, 0)))],
    "walk": [dict(bob=0, phase=0, lf=0, rf=0, arm=-2, side=((42, 0), (56, 0))),
             dict(bob=-2, phase=2, lf=4, rf=0, arm=0, side=((48, 0), (50, 4))),
             dict(bob=0, phase=4, lf=0, rf=0, arm=2, side=((56, 0), (42, 0))),
             dict(bob=-2, phase=6, lf=0, rf=4, arm=0, side=((50, 4), (48, 0)))],
}
_STAND = dict(lf=0, rf=0, phase=0, side=((46, 0), (52, 0)))
# 동작 — rr/rl 오른손·왼손 들어 올림(px), rs 옆모습 손 들어 올림, arm 옆 팔 앞뒤, wa 무기 각도(도, 음수 = 앞/동쪽으로),
# dx 옆모습 몸 밀림 (공격은 내딛고 피격은 뒤로)
CLIPS["attack"] = [dict(_STAND, bob=-1, rr=14, rs=14, arm=-4, wa=-160),
                   dict(_STAND, bob=0, rr=4, rs=6, arm=6, wa=-90, dx=1),
                   dict(_STAND, bob=0, rr=0, rs=2, arm=5, wa=-35, dx=1),
                   dict(_STAND, bob=0, rr=0, rs=0, arm=0, wa=0)]
CLIPS["cast"] = [dict(_STAND, bob=0, rr=6, rl=6, rs=8, arm=3, wa=-20),
                 dict(_STAND, bob=-1, rr=12, rl=12, rs=14, arm=5, wa=-40),
                 dict(_STAND, bob=-1, rr=14, rl=14, rs=16, arm=6, wa=-50),
                 dict(_STAND, bob=0, rr=8, rl=8, rs=8, arm=3, wa=-20)]
CLIPS["hit"] = [dict(_STAND, bob=1, rr=3, rl=3, rs=3, arm=-3, wa=10, dx=-2),
                dict(_STAND, bob=0, rr=1, rl=1, rs=1, arm=-1, wa=5, dx=-1)]
CLIP_MS = {"idle": 550, "walk": 180, "attack": 110, "cast": 140, "hit": 130, "die": 180}
LOOPING = {"idle", "walk"}
# 사망 — 대기 첫 프레임을 발을 축으로 뒤로 눕힌다. 칸을 넘지 않게 눕는 만큼 오른쪽으로 민다 (모든 파츠가 같은 피벗)
DIE = dict(deg=[0, -20, -40, -55], dx=[0, 8, 18, 28], dy=[0, 0, 2, 4])

# 기준점
ARM_X = (38, 66)          # 정면 소매 왼쪽 x (왼팔 · 오른팔), 폭 8
HAND_Y = 87               # 손 윗변 (몸 오르내림 b 를 더한다)
LEG_X = (44, 58)          # 정면 다리 왼쪽 x, 폭 10


# ── 공통 도형 ──

def leg_pair_front(b, f, cols, w=10, top=76, knee=None):
    """정면 다리 둘 (발 제외) — cols (밝은·가운데·그늘). knee 면 무릎 판 (판금)."""
    out = []
    lit, mid, dark = cols
    for x, lift in ((LEG_X[0], f["lf"]), (LEG_X[1], f["rf"])):
        y = -lift
        out += [P(mid, (x, top + b), (x + w, top + b), (x + w - 1, 98), (x + 1, 98), dy=y),
                P(lit, (x, top + b), (x + 4, top + b), (x + 4, 98), (x + 1, 98), dy=y),
                P(dark, (x + w - 3, top + b), (x + w, top + b), (x + w - 1, 98), (x + w - 3, 98), dy=y)]
        if knee:
            out += [P(knee[0], (x + 1, 86), (x + 9, 86), (x + 5, 90), dy=y), L_(knee[1], x + 1, 90, x + 9, 90, dy=y)]
    return out


def leg_pair_side(b, f, cols, top=76, knee=None):
    out = []
    far, near = f["side"]
    for (x, lift), k in ((far, 1), (near, 0)):
        lit, mid, dark = cols
        if k:  # 먼 다리는 한 단계 어둡게
            lit, mid = mid, dark
        y = -lift
        out += [P(mid, (x, top + b), (x + 9, top + b), (x + 9, 98), (x + 1, 98), dy=y),
                P(lit, (x + 5, top + b), (x + 9, top + b), (x + 9, 98), (x + 6, 98), dy=y)]
        if knee:
            out += [P(lit, (x + 4, 86), (x + 10, 87), (x + 8, 91), dy=y)]
    return out


def feet_front(f, cols, pointed=False, shaft=None):
    """정면 발 둘 — shaft 면 장화 목(그 높이부터). pointed 면 뾰족한 철화."""
    out = []
    lit, mid, dark = cols
    for x, lift in ((LEG_X[0], f["lf"]), (LEG_X[1], f["rf"])):
        y = -lift
        if shaft:
            out += [P(mid, (x, shaft), (x + 10, shaft), (x + 9, 98), (x + 1, 98), dy=y),
                    L_(lit, x + 2, shaft + 1, x + 2, 97, dy=y), L_(dark, x, shaft, x + 10, shaft, dy=y)]
        if pointed:
            out += [P(mid, (x - 1, 98), (x + 11, 98), (x + 12, 103), (x + 5, 105), (x - 2, 103), dy=y),
                    L_(lit, x, 99, x + 5, 99, dy=y)]
        else:
            out += [P(mid, (x - 1, 98), (x + 11, 98), (x + 11, 103), (x + 9, 105), (x, 105), (x - 1, 103), dy=y),
                    P(lit, (x - 1, 98), (x + 4, 98), (x + 4, 104), (x, 105), (x - 1, 103), dy=y),
                    L_(dark, x + 8, 99, x + 10, 103, dy=y)]
    return out


def feet_side(f, cols, pointed=False, shaft=None):
    out = []
    far, near = f["side"]
    for (x, lift), k in ((far, 1), (near, 0)):
        lit, mid, dark = cols
        if k:
            lit, mid = mid, dark
        y = -lift
        if shaft:
            out += [P(mid, (x, shaft), (x + 9, shaft), (x + 9, 98), (x + 1, 98), dy=y), L_(lit, x + 7, shaft + 1, x + 7, 97, dy=y)]
        tip = 17 if pointed else 14
        out += [P(mid, (x, 98), (x + 10, 98), (x + tip, 101), (x + tip, 104), (x + 1, 104), dy=y),
                L_(lit, x + 5, 99, x + tip - 2, 101, dy=y)]
    return out


def hand_front(b, f, cols):
    """두 손 (정면·뒷면) — 기준점 고정, 동작에서는 rr/rl 만큼 들린다. cols (밝은, 가운데, 그늘)."""
    lit, mid, dark = cols
    out = []
    for x, side in ((ARM_X[0], -1), (ARM_X[1], 1)):
        c = mid if side < 0 else dark
        y = hy(side, f)
        out += [P(c, (x + 1, y), (x + 8, y), (x + 8, y + 5), (x + 2, y + 6), dy=b),
                L_(lit if side < 0 else mid, x + 2, y, x + 2, y + 4, dy=b)]
    return out


def hand_side(b, f, cols):
    a, y = f["arm"], hys(f)
    lit, mid, _ = cols
    return [P(mid, (54 + a, y), (61 + a, y), (61 + a, y + 5), (55 + a, y + 6), dy=b),
            L_(lit, 60 + a, y, 60 + a, y + 4, dy=b)]


def sleeves_front(b, f, cols, cuff=None, wide=0, top=60):
    """정면 소매 둘 — 손 위에서 끝난다 (손이 들리면 함께 짧아진다). cuff 면 끝 두 줄."""
    lit, mid, dark = cols
    out = []
    for x, sign in ((ARM_X[0], -1), (ARM_X[1], 1)):
        y = hy(sign, f)
        bottom = y - (2 if cuff else 0)
        body = dark if sign > 0 else mid
        wl, wr = wide * (sign < 0), wide * (sign > 0)
        out += [P(body, (x, top), (x + 8, top), (x + 9 + wr, bottom), (x - wl, bottom), dy=b),
                L_(lit if sign < 0 else mid, x + 1, top + 1, x + 1 - wl, bottom - 1, dy=b)]
        if cuff:
            out += [P(cuff[0], (x - wl, bottom), (x + 9 + wr, bottom), (x + 9 + wr, y), (x - wl, y), dy=b),
                    L_(cuff[1], x - wl + 1, bottom + 1, x + 8, bottom + 1, dy=b)]
    return out


def sleeve_side(b, f, cols, cuff=None, wide=0, top=60):
    a, y = f["arm"], hys(f)
    lit, mid, _ = cols
    bottom = y - (2 if cuff else 0)
    out = [P(mid, (52 + a, top), (60 + a, top), (63 + a + wide, bottom), (53 + a - wide, bottom), dy=b),
           L_(lit, 59 + a, top + 1, 62 + a + wide, bottom - 1, dy=b)]
    if cuff:
        out += [P(cuff[0], (53 + a - wide, bottom), (63 + a + wide, bottom), (63 + a + wide, y), (53 + a - wide, y), dy=b)]
    return out


SKIN = ("P", "p", "q")


def hy(sign, f):
    """정면 손 윗변 — 오른손(sign 1)은 rr, 왼손은 rl 만큼 들린다."""
    return HAND_Y - f.get("rr" if sign > 0 else "rl", 0)


def hys(f):
    """옆모습 손 윗변."""
    return HAND_Y - f.get("rs", 0)


# ── 몸 (속옷 차림 — 장비가 없을 때 보이는 것) ──

def part_body(view, b, f, i):
    linen = ("X", "W", "i")
    if view == "e":
        out = leg_pair_side(b, f, linen)
        out += feet_side(f, SKIN)
        out += [P("W", (46, 58), (66, 58), (66, 81), (48, 81), dy=b), P("X", (58, 58), (66, 58), (66, 81), (60, 81), dy=b),
                L_("i", 47, 62, 48, 80, dy=b)]
        out += sleeve_side(b, f, linen, top=60)
        out += [P("p", (54 + f["arm"], 70), (60 + f["arm"], 70), (61 + f["arm"], hys(f)), (55 + f["arm"], hys(f)), dy=b)]
        out += hand_side(b, f, SKIN)
        return out
    out = leg_pair_front(b, f, linen)
    out += feet_front(f, SKIN)
    out += [P("W", (44, 58), (68, 58), (68, 81), (44, 81), dy=b), P("X", (44, 58), (54, 58), (54, 81), (44, 81), dy=b),
            P("i", (63, 58), (68, 58), (68, 81), (63, 81), dy=b), L_("i", 44, 80, 68, 80, dy=b)]
    # 짧은 소매 + 맨팔
    for x, sign in ((ARM_X[0], -1), (ARM_X[1], 1)):
        out += [P("W" if sign < 0 else "i", (x, 60), (x + 8, 60), (x + 8, 69), (x, 69), dy=b),
                P("p" if sign < 0 else "q", (x + 1, 69), (x + 7, 69), (x + 8, max(70, hy(sign, f))), (x + 1, max(70, hy(sign, f))), dy=b),
                L_("P" if sign < 0 else "p", x + 2, 70, x + 2, max(70, hy(sign, f) - 1), dy=b)]
    out += hand_front(b, f, SKIN)
    return out


# ── 머리 사물 (캐릭터 만들기 — 분류 head) ──

def part_head(obj):
    def draw(view, b, f, i):
        G.OBJ = obj
        G.LIT = True           # GPU 조명이 비춘다 — 빛무리·따뜻한 반사를 칠하지 않는다
        G.BODY = "knight"      # 깃 색 — 짙은 천 (어떤 상의와도 어울리게)
        return G.obj_head(view, b, i, f)
    return draw


# ── 상의 (Body) ──

def part_top_robe(view, b, f, i):
    sw = sway(f)
    cols = ("1", "2", "3")
    if view == "e":
        out = [P("2", (47, 58), (65, 58), (68, 80), (72 + sw, 100), (40 + sw, 100), (44, 80), dy=b),
               P("1", (58, 58), (65, 58), (68, 80), (72 + sw, 100), (62 + sw, 100), dy=b),
               L_("3", 47, 62 + b, 41 + sw, 99), L_("3", 56, 80 + b, 55 + sw, 99),
               P("2", (43, 55), (52, 55), (52, 68), (41, 66), dy=b), L_("3", 43, 62, 50, 64, dy=b),
               L_("5", 45, 76, 68, 76, dy=b), P("5", (66, 77), (68, 77), (69, 90), (67, 90), dy=b),
               P("5", (66, 90), (70, 90), (69, 93), (67, 93), dy=b)]
        return out + sleeve_side(b, f, cols, cuff=("3", "4"), wide=2) + hand_side(b, f, SKIN)
    out = [P("2", (44, 58), (68, 58), (73, 80), (77 + sw, 100), (35 + sw, 100), (39, 80), dy=b),
           P("1", (44, 58), (55, 58), (55, 80), (55 + sw, 100), (35 + sw, 100), (39, 80), dy=b),
           P("3", (62, 58), (68, 58), (73, 80), (77 + sw, 100), (64 + sw, 100), dy=b),
           L_("3", 47, 80 + b, 44 + sw, 99), L_("1", 46, 80 + b, 43 + sw, 99), L_("3", 60, 82 + b, 62 + sw, 99),
           P("4", (52, 58), (60, 58), (60, 96), (52, 96), dy=b), L_("2", 53, 59, 53, 95, dy=b),
           L_("5", 40, 76, 72, 76, dy=b)]
    if view == "n":
        out += [P("2", (46, 55), (66, 55), (63, 70), (49, 70), dy=b), P("3", (58, 55), (66, 55), (63, 70), (58, 70), dy=b),
                L_("3", 50, 66, 62, 66, dy=b)]
    else:
        out += [P("5", (54, 74), (58, 74), (58, 78), (54, 78), dy=b),
                L_("5", 57, 78, 59, 90, dy=b), P("5", (57, 90), (61, 90), (60, 94), (58, 94), dy=b)]
    out += sleeves_front(b, f, cols, cuff=("3", "4"), wide=3) + hand_front(b, f, SKIN)
    if view == "s":
        out += [f"    px {x} {y + b} G" for x, y in ((40, 95), (41, 97), (42, 99), (43, 101))]
    return out


def part_top_coat(view, b, f, i):
    sw = sway(f)
    cols = ("6", "7", "8")
    lace = ("X", "W")
    if view == "e":
        out = [P("7", (46, 58), (66, 58), (66, 81), (48, 81), dy=b), P("6", (58, 58), (66, 58), (66, 81), (59, 81), dy=b),
               P("7", (46, 80), (55, 80), (51 + sw, 96), (40 + sw, 96), dy=b), L_("G", 40 + sw, 96 + b, 51 + sw, 96 + b),
               L_("8", 48, 82 + b, 44 + sw, 95 + b),
               P("6", (58, 80), (66, 80), (66, 86), (58, 86), dy=b), L_("G", 66, 58, 66, 86, dy=b),
               P("X", (64, 58), (68, 58), (67, 64), (65, 66), dy=b), f"    px 64 {66 + b} G", f"    px 64 {72 + b} G"]
        return out + sleeve_side(b, f, cols, cuff=lace) + hand_side(b, f, SKIN)
    out = [P("7", (44, 58), (68, 58), (68, 80), (44, 80), dy=b),
           P("6", (44, 58), (54, 58), (54, 80), (44, 80), dy=b), P("8", (62, 58), (68, 58), (68, 80), (62, 80), dy=b),
           P("7", (44, 80), (55, 80), (53 + sw, 95), (42 + sw, 95), dy=b), P("6", (44, 80), (49, 80), (47 + sw, 95), (42 + sw, 95), dy=b),
           P("8", (57, 80), (68, 80), (70 + sw, 95), (59 + sw, 95), dy=b),
           L_("G", 42 + sw, 95 + b, 53 + sw, 95 + b), L_("g", 59 + sw, 95 + b, 70 + sw, 95 + b),
           L_("G", 44, 80, 42 + sw, 95, dy=b), L_("g", 68, 80, 70 + sw, 95, dy=b),
           P("6", (38, 58), (46, 57), (46, 63), (38, 64), dy=b), P("8", (66, 57), (74, 58), (74, 64), (66, 63), dy=b)]
    if view == "n":
        out += [L_("8", 56, 59, 56, 95, dy=b), f"    px 52 {80 + b} G", f"    px 60 {80 + b} G"]
    else:
        out += [P("8", (50, 58), (54, 58), (56, 72), (52, 72), dy=b), P("8", (58, 58), (62, 58), (60, 72), (56, 72), dy=b),
                P("X", (53, 58), (59, 58), (58, 64), (56, 68), (54, 64), dy=b), L_("W", 56, 60, 56, 66, dy=b),
                L_("G", 44, 58, 44, 80, dy=b)]
        out += [f"    px {x} {y + b} G" for x in (49, 63) for y in (64, 70, 76)]
    return out + sleeves_front(b, f, cols, cuff=lace) + hand_front(b, f, SKIN)


def part_top_rags(view, b, f, i):
    sw = sway(f)
    hem = [(71, 84), (66, 88), (62, 85), (57, 89), (52, 85), (47, 88), (41, 84)]
    if view == "e":
        a = f["arm"]
        out = [P("0", (46, 58), (66, 58), (68, 84), (64 + sw, 88), (58, 85), (52 + sw, 89), (45, 85), dy=b),
               P("9", (58, 58), (66, 58), (68, 84), (64 + sw, 88), (60, 85), dy=b),
               P("r", (49, 66), (54, 66), (54, 71), (49, 71), dy=b), f"    px 49 {66 + b} X", f"    px 53 {70 + b} X",
               L_("5", 46, 78, 67, 78, dy=b),
               P("0", (52 + a, 60), (60 + a, 60), (60 + a, 70), (53 + a, 71), dy=b),
               P("p", (54 + a, 70), (60 + a, 70), (61 + a, max(71, hys(f))), (55 + a, max(71, hys(f))), dy=b),
               *[L_("X", 54 + a, yy, 61 + a, yy + 1, dy=b) for yy in (74, 78, 82)]]
        return out + hand_side(b, f, SKIN)
    out = [P("0", (44, 58), (68, 58), *[(x + sw * (k % 2), y) for k, (x, y) in enumerate(hem)], dy=b),
           P("9", (44, 58), (55, 58), (55, 86), (47 + sw, 88), (41, 84), dy=b),
           P("$", (63, 58), (68, 58), (71, 84), (66 + sw, 88), (63, 86), dy=b),
           L_("5", 42, 77, 70, 77, dy=b), P("5", (47, 76), (51, 76), (51, 79), (47, 79), dy=b), L_("5", 49, 79, 48, 86, dy=b)]
    if view == "n":
        out += [P("K", (58, 63), (64, 63), (64, 69), (58, 69), dy=b), f"    px 58 {63 + b} X", f"    px 63 {68 + b} X"]
    else:
        out += [P("r", (46, 63), (51, 63), (51, 68), (46, 68), dy=b), f"    px 46 {63 + b} X", f"    px 50 {67 + b} X",
                P("K", (60, 71), (65, 71), (65, 75), (60, 75), dy=b), f"    px 64 {71 + b} X"]
    for x, sign in ((ARM_X[0], -1), (ARM_X[1], 1)):
        mid = "p" if sign < 0 else "q"
        out += [P("0" if sign < 0 else "$", (x, 60), (x + 8, 60), (x + 8, 69), (x + 4, 71), (x, 69), dy=b),
                P(mid, (x + 1, 69), (x + 7, 69), (x + 8, max(70, hy(sign, f))), (x + 1, max(70, hy(sign, f))), dy=b),
                *[L_("X" if sign < 0 else "W", x + 1, yy, x + 7, yy + 1, dy=b) for yy in (73, 77, 81)]]
    return out + hand_front(b, f, SKIN)


def part_top_traveler(view, b, f, i):
    sw = sway(f)
    cols = ("&", "*", "+")
    if view == "e":
        out = [P("*", (46, 58), (66, 58), (68, 80), (70 + sw, 100), (38 + sw, 100), (43, 80), dy=b),
               P("&", (58, 58), (66, 58), (68, 80), (70 + sw, 100), (62 + sw, 100), dy=b),
               L_("+", 46, 62 + b, 39 + sw, 99), L_("+", 54, 80 + b, 52 + sw, 99),
               P("r", (48, 56), (64, 56), (64, 62), (48, 62), dy=b), P("r", (44, 58), (49, 58), (42 + sw, 72), (39 + sw, 70), dy=b),
               P("m", (44, 60), (47, 60), (42 + sw, 71), (40 + sw, 70), dy=b),
               P("L", (44, 80), (52, 80), (52, 90), (44, 90), dy=b), P("l", (44, 80), (52, 80), (52, 84), (44, 84), dy=b),
               f"    px 48 {84 + b} G", L_("L", 52, 60, 46, 80, dy=b)]
        return out + sleeve_side(b, f, cols) + hand_side(b, f, SKIN)
    out = [P("*", (44, 58 + b), (68, 58 + b), (74, 100 - 2 * (sw < 0)), (38, 100 - 2 * (sw > 0))),
           P("&", (44, 58 + b), (52, 58 + b), (51, 100), (38, 100)),
           P("+", (61, 58 + b), (68, 58 + b), (74, 100), (61, 100))]
    if view == "n":
        out += [L_("+", 56, 60 + b, 56, 99), L_("L", 46, 60 + b, 66, 84 + b), L_("l", 47, 60 + b, 67, 84 + b),
                P("r", (47, 56), (65, 56), (64, 62), (48, 62), dy=b), P("r", (50, 60), (54, 60), (53, 72), (49, 70), dy=b)]
    else:
        out += [P("0", (52, 62 + b), (60, 62 + b), (61, 100), (51, 100)), L_("$", 59, 63 + b, 60, 99),
                L_("L", 46, 60 + b, 66, 84 + b), L_("l", 47, 60 + b, 67, 84 + b),
                P("L", (62, 80), (72, 80), (72, 90), (62, 90), dy=b), P("l", (62, 80), (72, 80), (72, 84), (62, 84), dy=b),
                f"    px 67 {84 + b} G",
                P("r", (47, 56), (65, 56), (64, 62), (48, 62), dy=b), P("r", (58, 60), (62, 60), (63 + sw, 73), (59 + sw, 71), dy=b),
                P("m", (60, 60), (62, 60), (63 + sw, 73), (61 + sw, 72), dy=b)]
    return out + sleeves_front(b, f, cols) + hand_front(b, f, SKIN)


def part_top_plate(view, b, f, i):
    """판금 — 가슴판·허리띠·핏빛 앞자락·판금 팔·각진 견갑 (grim 기사). 망토는 따로(cape_plate)."""
    G.FEMALE = False
    metal = ("A", "B", "C")
    if view == "e":
        a = f["arm"]
        out = [P("B", (46, 60), (66, 60), (66, 81), (48, 81), dy=b), P("A", (58, 60), (66, 60), (66, 81), (60, 81), dy=b),
               L_("R", 66, 61, 66, 80, dy=b),
               P("l", (47, 79), (67, 79), (67, 82), (47, 82), dy=b), P("G", (63, 79), (67, 79), (67, 83), (63, 83), dy=b),
               P("r", (62, 82), (67, 82), (67, 94), (64, 92), (62, 96), dy=b),
               P("B", (53 + a, 70), (61 + a, 70), (62 + a, max(71, hys(f))), (54 + a, max(71, hys(f))), dy=b),
               L_("A", 60 + a, 71, 61 + a, max(71, hys(f) - 1), dy=b),
               P("B", (46, 57), (62, 56), (70, 60), (66, 68), (58, 72), (47, 70), (44, 63), dy=b),
               P("A", (50, 57), (62, 56), (70, 60), (60, 62), (50, 62), dy=b),
               L_("R", 50, 57, 62, 56, dy=b), L_("C", 47, 68, 58, 70, dy=b), f"    px 57 {64 + b} G"]
        return out + hand_side(b, f, SKIN)
    out = G.body_s(b) if view == "s" else [P("B", (42, 60), (70, 60), (68, 81), (44, 81), dy=b),
                                           P("C", (56, 60), (70, 60), (68, 81), (56, 81), dy=b),
                                           L_("Q", 56, 61, 56, 80, dy=b)]
    for x, sign in ((ARM_X[0], -1), (ARM_X[1], 1)):
        y = max(71, hy(sign, f))
        out += [P("B" if sign < 0 else "C", (x, 70), (x + 8, 70), (x + 9, y), (x + 1, y), dy=b),
                L_("A" if sign < 0 else "B", x + 1, 71, x + 1, y - 1, dy=b)]
    out += G.pauldron(41, -1, b) + G.pauldron(71, 1, b)
    return out + hand_front(b, f, SKIN)


def part_cape_plate(view, b, f, i):
    if view == "e":
        return G.cape_e(b, f["phase"])
    return G.cape_s(b, f["phase"], back=view == "n")


# ── 하의 (Legs) ──

def legs_part(cols, knee=None, wraps=None, torn=None):
    def draw(view, b, f, i):
        if view == "e":
            out = leg_pair_side(b, f, cols, knee=knee)
            if wraps:
                far, near = f["side"]
                for (x, lift), k in ((far, 1), (near, 0)):
                    out += [P(wraps[1 + k], (x, 90), (x + 9, 90), (x + 9, 98), (x + 1, 98), dy=-lift)]
                    out += [L_(wraps[k], x + 1, yy, x + 8, yy + 1, dy=-lift) for yy in (91, 94)]
            return out
        out = leg_pair_front(b, f, cols, knee=knee)
        if wraps:
            for x, lift in ((LEG_X[0], f["lf"]), (LEG_X[1], f["rf"])):
                out += [P(wraps[1], (x, 90), (x + 10, 90), (x + 9, 98), (x + 1, 98), dy=-lift)]
                out += [L_(wraps[0], x + 1, yy, x + 9, yy + 1, dy=-lift) for yy in (91, 94, 97)]
        if torn:
            out += [P("p", (46, 85), (50, 85), (49, 88), dy=-f["lf"]), P("q", (61, 86), (65, 86), (63, 89), dy=-f["rf"])]
        return out
    return draw


# ── 신발 (Shoes) ──

def shoes_part(cols, pointed=False, shaft=None):
    def draw(view, b, f, i):
        if view == "e":
            return feet_side(f, cols, pointed=pointed, shaft=shaft)
        return feet_front(f, cols, pointed=pointed, shaft=shaft)
    return draw


# ── 장갑 (Hand) ──

def gloves_part(cols, cuff):
    def draw(view, b, f, i):
        lit, mid, dark = cols
        if view == "e":
            a, y = f["arm"], hys(f)
            return [P(mid, (53 + a, y - 2), (62 + a, y - 2), (62 + a, y + 5), (55 + a, y + 7), dy=b),
                    L_(cuff, 53 + a, y - 2, 62 + a, y - 2, dy=b), L_(lit, 60 + a, y, 60 + a, y + 4, dy=b)]
        out = []
        for x, side in ((ARM_X[0], -1), (ARM_X[1], 1)):
            c = mid if side < 0 else dark
            y = hy(side, f)
            out += [P(c, (x, y - 2), (x + 9, y - 2), (x + 9, y + 5), (x + 7, y + 7), (x + 2, y + 7), (x, y + 5), dy=b),
                    L_(cuff, x, y - 2, x + 9, y - 2, dy=b),
                    L_(lit if side < 0 else mid, x + 2, y, x + 2, y + 4, dy=b)]
        return out
    return draw


# ── 무기 (Weapon) — 오른손. 뒷모습은 몸 뒤(층 -1)라 손 자리를 좌우 반전 ──

def _rot(pts, deg, cx, cy, mirror):
    """손(cx, cy) 기준 오프셋 점들을 deg 만큼(시계 방향 +, 칼날 기본 방향 = 아래) 돌려 절대 좌표로. mirror 면 좌우 반전."""
    import math
    r = math.radians(deg)
    c, sn = math.cos(r), math.sin(r)
    out = []
    for x, y in pts:
        x = -x if mirror else x
        out.append((round(cx + x * c - y * sn), round(cy + x * sn + y * c)))
    return out


# 무기 모양 — 손 기준 오프셋, 칼날·자루가 아래(+y)로. (색, 다각형 점들) · 선은 점 두 개
WEAPONS = {
    "sword": [("S", [(-1, 2), (2, 2), (8, 22), (6, 23)]), ("s", [(1, 3), (7, 22)]),
              ("G", [(-4, 0), (5, -1), (5, 1), (-4, 2)]), ("g", [(-1, -6), (2, -6), (2, 0), (-1, 0)]),
              ("G", [(-1, -9), (2, -9), (2, -6), (-1, -6)])],
    "staff": [("l", [(2, -44), (4, -44), (5, 17), (3, 17)]), ("L", [(2, -43), (3, 16)]),
              ("L", [(0, -47), (5, -47), (5, -42), (0, -42)])],
    "mallet": [("L", [(-1, 0), (4, -10)]), ("l", [(0, 0), (5, -10)]),
               ("L", [(2, -16), (11, -14), (9, -8), (1, -10)]), ("l", [(7, -15), (11, -14), (9, -8), (6, -9)])],
    "cane": [("Q", [(0, 0), (4, 15)]), ("D", [(1, 0), (5, 15)]),
             ("G", [(-2, -4), (3, -4), (3, -1), (-2, -1)])],
}


def weapon_part(kind):
    def draw(view, b, f, i):
        if view == "e":
            hx, hy_ = 57 + f["arm"], hys(f) + 3 + b
        else:
            hx, hy_ = ARM_X[1] + 5, hy(1, f) + 3 + b
            if view == "n":
                hx = 112 - hx
        mirror = view == "n"
        deg = f.get("wa", 0) * (-1 if mirror else 1)
        out = []
        for colour, pts in WEAPONS[kind]:
            q = _rot(pts, deg, hx, hy_, mirror)
            if len(q) == 2:
                out.append(L_(colour, *q[0], *q[1]))
            else:
                out.append(P(colour, *q))
        return out
    return draw


# 파츠 — (키, 이름, 분류 · 장비 자리, 그리기, 앞뒤 [동, 북, 서, 남])
L = lambda n: [n]  # noqa: E731 — 모든 방향이 같은 앞뒤
PARTS = [
    ("body", "몸", "body", part_body, None),
    *[(f"head_{k}", n, "head", part_head(k), L(6)) for k, n in
      (("lantern", "등불"), ("cage", "새장"), ("bell", "종"), ("candle", "촛대"))],
    ("top_robe", "수도복", "Body", part_top_robe, L(4)),
    ("top_coat", "귀족 외투", "Body", part_top_coat, L(4)),
    ("top_rags", "넝마", "Body", part_top_rags, L(4)),
    ("top_traveler", "여행 외투", "Body", part_top_traveler, L(4)),
    ("top_plate", "판금", "Body", part_top_plate, L(4)),
    ("cape_plate", "판금 망토", "Body", part_cape_plate, [-1, 8, -1, -1]),
    ("legs_trousers", "바지", "Legs", legs_part(("C", "B", "Q")), L(2)),
    ("legs_travel", "여행 바지", "Legs", legs_part(("0", "$", "4")), L(2)),
    ("legs_rags", "해진 바지", "Legs", legs_part(("0", "$", "4"), wraps=("X", "W", "i"), torn=True), L(2)),
    ("legs_plate", "정강이받이", "Legs", legs_part(("A", "B", "C"), knee=("A", "Q")), L(2)),
    ("shoes_boots", "장화", "Shoes", shoes_part(("B", "Q", "D"), shaft=88), L(3)),
    ("shoes_leather", "가죽 신", "Shoes", shoes_part(("L", "l", "4")), L(3)),
    ("shoes_sandals", "샌들", "Shoes", shoes_part(("L", "l", "4")), L(3)),
    ("shoes_sabatons", "철화", "Shoes", shoes_part(("B", "C", "Q"), pointed=True), L(3)),
    ("shoes_wraps", "감은 천", "Shoes", shoes_part(("W", "i", "$")), L(3)),
    ("gloves_white", "흰 장갑", "Hand", gloves_part(("X", "W", "i"), "X"), L(5)),
    ("gloves_leather", "가죽 장갑", "Hand", gloves_part(("L", "l", "4"), "L"), L(5)),
    ("gloves_gauntlet", "건틀릿", "Hand", gloves_part(("B", "C", "Q"), "A"), L(5)),
    ("weapon_sword", "장검", "Weapon", weapon_part("sword"), [7, -1, 7, 7]),
    ("weapon_staff", "지팡이", "Weapon", weapon_part("staff"), [7, -1, 7, 7]),
    ("weapon_mallet", "나무 망치", "Weapon", weapon_part("mallet"), [7, -1, 7, 7]),
    ("weapon_cane", "지팡이칼", "Weapon", weapon_part("cane"), [7, -1, 7, 7]),
]


def part_canvas(key, draw, layer):
    head = key.startswith("head_")
    obj = key[5:] if head else None
    pal = G.PALETTE.replace("\nend\n", f"\n    glow {G.GLOW_KEYS}\nend\n") if head else G.PALETTE
    lines = ["canvas 1", "cell 112 112", "", f"// grim 종이 인형 파츠 {key} — tools/grim_paperdoll.py 가 만든다.", "", pal]
    for clip, frames in CLIPS.items():
        for view in "sne":
            for i, f in enumerate(frames):
                b = f["bob"] - G.UP
                shift = [f"    shift {f['dx']} 0"] if view == "e" and f.get("dx") else []
                lines += [f"frame {clip}_{view}_{i}", *draw(view, b, f, i), *shift,
                          "    speckle B C 4 7", "    speckle A B 3 11", "    outline o", "end", ""]
    # 사망 — 대기 첫 프레임을 발 축으로 돌린다 (파츠마다 같은 피벗이라 겹친 그대로 쓰러진다)
    for view in "sne":
        # 미는 양은 돌리기 안에서 — 돌린 뒤 shift 하면 칸 밖으로 나간 머리가 이미 잘려 있다
        lines += [f"frames die_{view}_ 4",
                  f"    stamp_rot idle_{view}_0 [{' '.join(map(str, DIE['dx']))}] [{' '.join(map(str, DIE['dy']))}] "
                  f"[{' '.join(map(str, DIE['deg']))}] 56 104", "end", ""]
    for clip, n in [(c, len(fr)) for c, fr in CLIPS.items()] + [("die", 4)]:
        names = " ".join(f"{clip}_e_{i}" for i in range(n))
        lines += [f"frames {clip}_w_ {n} = [{names}]", "    flip_h", "end", ""]
    lines += ["sheet", "    directions 4", "    tinted false", "    pixels_per_meter 32"]
    if layer is not None:
        lines.append("    layer " + " ".join(map(str, layer)))
    if head and obj in G.LIGHTS:
        x, y, colour, radius, intensity, flicker = G.LIGHTS[obj]
        lines.append(f"    light {x} {y - G.UP} {colour} {radius} {intensity} {flicker}")
    for clip, n in [(c, len(fr)) for c, fr in CLIPS.items()] + [("die", 4)]:
        mode = "loop" if clip in LOOPING else "once"
        lines.append(f"    clip {clip.capitalize()} {CLIP_MS[clip]} {mode}")
        for view in "enws":
            lines.append("        row " + " ".join(f"{clip}_{view}_{i}" for i in range(n)))
        lines.append("    end")
    image = "body.png" if key == "body" else f"{key}.png"
    lines += ["end", "", "output", f"    image {image}", f"    sheet {image.replace('.png', '.sheet.ron')}", "end", ""]
    return "\n".join(lines)


def ron_str(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def catalog():
    """템플릿 카탈로그 — 고르는 것은 머리 사물뿐. 장비 파츠는 아이템이 가리킨다."""
    out = ["// tools/grim_paperdoll.py 가 만든 템플릿 카탈로그 — 오브젝트헤드 · 공용 몸.",
           "// 고르는 것은 머리 사물(head)뿐이다. 상의·하의·신발·장갑·무기는 장비 외형 — data/display.ron 아이템의",
           "// parts 가 이 폴더의 시트(top_* · legs_* · shoes_* · gloves_* · weapon_* · cape_plate)를 가리킨다.",
           "(", "    version: 1,", '    key: "grim",', '    name: "그림 · 오브젝트헤드",',
           '    reference: "grim 화풍 — 각진 다크 판타지 SD, 머리 자리에 사물",',
           '    body: "body.sheet.ron",', "    parts: ["]
    for key, name, cat, _, _ in PARTS:
        if cat == "head":
            out.append(f"        (key: {ron_str(key)}, name: {ron_str(name)}, category: \"head\", "
                       f"palette: \"cloth\", tinted: false, slot: None),")
    return "\n".join(out + ["    ],", ")", ""])


def register():
    """공용 카탈로그의 템플릿 목록에 grim 을 (한 번) 적는다."""
    path = OUT.parent / "catalog.ron"
    if not path.exists():
        return
    lines = path.read_text().split("\n")
    a = lines.index("    templates: [")
    b = lines.index("    ],", a)
    entry = '        "grim/catalog.ron",'
    if entry not in lines[a:b]:
        lines.insert(b, entry)
        path.write_text("\n".join(lines))


def main():
    want = next((a.split("=", 1)[1] for a in sys.argv if a.startswith("--print=")), None)
    if want:
        part = next(p for p in PARTS if p[0] == want)
        sys.stdout.write(part_canvas(part[0], part[3], part[4]))
        return
    if not WC.exists():
        sys.exit(f"{WC} 가 없다 — 먼저 `cargo build --release --no-default-features`")
    OUT.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        for key, _, _, draw, layer in PARTS:
            src = Path(tmp) / f"{key}.canvas"
            src.write_text(part_canvas(key, draw, layer))
            r = subprocess.run([str(WC), "build", str(src), "--out", str(OUT)], capture_output=True, text=True)
            if r.returncode:
                sys.exit(f"{key}: {r.stderr}")
    (OUT / "catalog.ron").write_text(catalog())
    register()
    print(f"{OUT} — 파츠 {len(PARTS)}장")


if __name__ == "__main__":
    main()
