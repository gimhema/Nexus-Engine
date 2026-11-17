#!/usr/bin/env python3
"""종이 인형(paper-doll) 캐릭터 생성기 — 베이스 템플릿과 파츠 시트를 만든다.

    python3 tools/paperdoll.py            # 모든 템플릿·파츠를 WorldEditor 애셋으로 빌드
    python3 tools/paperdoll.py --preview  # + preview/paperdoll_*.png 견본 그림

왜 생성기인가: 파츠(머리카락·옷·모자…)는 몸과 **같은 칸·같은 프레임**을 그려야 겹친다
(WorldEditor `sprites.rs` 「파츠」). 템플릿마다 손으로 그리면 템플릿 수 × 파츠 수만큼 그려야 하고,
체형을 고치면 전부 다시 그려야 한다. 여기서는 템플릿을 **체형 수치**로 적고, 몸과 모든 파츠를
같은 기하(`Geo`)에서 그린다 — 템플릿을 하나 더하면 파츠가 전부 그 체형으로 따라 나온다.

- 표준 라이브러리만 쓴다 (PNG 는 zlib 로 직접 쓴다).
- 색을 바꿀 부위(피부·머리·옷)는 **밝은 무채색**으로 그리고 `tinted: true` — 에디터가 색을 곱한다.
  곱하기라 어둡게만 되므로 밝게 그린다 (WorldCanvas CLAUDE.md 「지켜야 할 것」).
- 시트 배치는 `player.sheet.ron` 과 같다: Idle 2프레임(0행~) + Walk 4프레임(4행~), 방향 동·북·서·남.
- 빛은 왼쪽 위. 모양마다 **안쪽 외곽선**을 둔다 — 옷이 몸의 실루엣을 넘지 않게.

결과: `WorldEditor/assets/worldcanvas/paperdoll/<템플릿>/<파츠>.png` + `.sheet.ron`,
그리고 같은 폴더의 `catalog.ron`(이 템플릿에 맞는 파츠 목록과 앞뒤·자리).
"""

import math
import struct
import sys
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT.parent / "WorldEditor" / "assets" / "worldcanvas" / "paperdoll"
PREVIEW = ROOT / "preview"

# 엔진 방향 순서 (0=동, 반시계): 동·북·서·남. 서는 동의 좌우 반전.
DIRS = ["E", "N", "W", "S"]

# 클립 — (상태, 프레임 수, 프레임 ms). 시트 배치는 클립 순서 × 방향.
CLIPS = [("Idle", 2, 500), ("Walk", 4, 150)]

# 무채색 단계 (tinted 파츠). 색을 곱하므로 밝게.
GRAY = {"O": 72, "D": 150, "M": 205, "L": 236, "H": 255}
# 피부 — 외곽선을 조금 밝게 두어 곱한 뒤에도 갈색 테두리가 된다.
SKIN = {"O": 96, "D": 196, "M": 228, "L": 246, "H": 255}


# ─────────────────────────────────────────────────────────────────────────────
# 템플릿 — 체형 수치
# ─────────────────────────────────────────────────────────────────────────────


class Template:
    """체형 하나. 길이는 픽셀이고 세로는 위→아래로 잰다 (발밑 = 칸 맨 아래 줄)."""

    def __init__(self, key, name, ref, cell, **kw):
        self.key = key
        self.name = name
        self.ref = ref  # 참고한 장르 (특정 게임 캐릭터를 옮기지 않는다)
        self.head_w = kw["head"][0]
        self.head_h = kw["head"][1]
        # 머리 위 여백 — 모자·뿔·귀·올림머리가 칸 밖으로 잘리지 않게. 몸 크기는 그대로다.
        self.pad = kw.get("pad", max(3, self.head_h // 2 + 1))
        self.w, self.h = cell[0], cell[1] + self.pad
        self.head_top = kw.get("head_top", 1) + self.pad
        self.neck = kw.get("neck", 0)
        self.torso_h = kw["torso"][0]
        self.shoulder = kw["torso"][1]  # 어깨 반폭
        self.waist = kw["torso"][2]  # 허리 반폭
        self.arm_w = kw["arm"][0]
        self.arm_len = kw["arm"][1]
        self.leg_w = kw["leg"][0]
        self.leg_gap = kw["leg"][1]  # 다리 안쪽 끝과 중심선 사이
        self.foot_h = kw.get("foot_h", 2)
        self.eye = kw.get("eye", (2, 3))  # 눈 (폭, 높이)
        self.eye_y = kw.get("eye_y", 0.55)  # 머리 안 눈 높이 (0 위 ~ 1 아래)
        self.eye_gap = kw.get("eye_gap", 2)  # 눈 사이 반간격
        self.ears = kw.get("ears")  # None / elf / cat / horns / round
        self.tail = kw.get("tail")  # None / cat / fox / devil
        self.flat = kw.get("flat", False)  # 8비트 — 그늘 없이 두 단계
        self.ppm = kw.get("ppm", 16)
        self.tusks = kw.get("tusks", False)
        # 몸통 세로 시작 = 머리 아래 + 목
        self.shoulder_y = self.head_top + self.head_h + self.neck - kw.get("chin_overlap", 1)
        self.hip_y = self.shoulder_y + self.torso_h
        self.ground = self.h - 1

    @property
    def cx(self):
        return self.w // 2


TEMPLATES = [
    Template(
        "sd_human", "SD 2.5등신 · 사람", "쿼터뷰 MMORPG 의 SD 캐릭터 — 큰 머리, 짧은 팔다리",
        (24, 32), head=(16, 14), head_top=2, torso=(7, 5, 4), arm=(3, 7), leg=(3, 1), eye=(2, 3),
    ),
    Template(
        "sd_elf", "SD 2.5등신 · 엘프", "같은 SD 체형에 뾰족한 귀",
        (24, 32), head=(16, 14), head_top=2, torso=(7, 5, 4), arm=(3, 7), leg=(3, 1), eye=(2, 3),
        ears="elf",
    ),
    Template(
        "sd_beast", "SD 2.5등신 · 수인", "고양이 귀와 꼬리 — 판타지 MMO 의 수인 종족",
        (24, 32), head=(16, 14), head_top=4, torso=(7, 5, 4), arm=(3, 7), leg=(3, 1), eye=(2, 3),
        ears="cat", tail="cat",
    ),
    Template(
        "sd_demon", "SD 2.5등신 · 마족", "뿔과 가는 꼬리",
        (24, 32), head=(16, 14), head_top=4, torso=(7, 5, 4), arm=(3, 7), leg=(3, 1), eye=(2, 3),
        ears="horns", tail="devil",
    ),
    Template(
        "sd_orc", "SD 2.5등신 · 오크", "넓은 어깨, 엄니 — 피부색은 초록 계열로 입힌다",
        (24, 32), head=(16, 13), head_top=3, torso=(8, 6, 5), arm=(4, 7), leg=(3, 1), eye=(2, 2),
        eye_y=0.5, tusks=True, ears="round",
    ),
    Template(
        "sd_dwarf", "SD · 드워프", "작은 키, 굵은 몸통과 팔",
        (24, 28), head=(14, 12), head_top=1, torso=(7, 6, 6), arm=(4, 6), leg=(4, 0), eye=(2, 2),
        eye_y=0.5,
    ),
    Template(
        "jrpg16", "16비트 JRPG 3등신", "16비트 콘솔 RPG 의 필드 캐릭터 — 작고 또렷한 실루엣",
        (16, 24), head=(12, 10), head_top=1, torso=(6, 4, 3), arm=(2, 6), leg=(2, 1), eye=(1, 2),
        eye_gap=2, foot_h=1,
    ),
    Template(
        "adventure", "탑다운 액션 2등신", "탑다운 액션 어드벤처 — 머리와 몸이 반반인 땅딸막한 체형",
        (16, 20), head=(12, 10), head_top=0, torso=(5, 4, 4), arm=(2, 4), leg=(2, 1), eye=(1, 2),
        eye_gap=2, foot_h=1,
    ),
    Template(
        "lifesim", "생활 시뮬 3등신", "농장·생활 시뮬레이션 — 가는 몸, 긴 다리",
        (16, 32), head=(12, 11), head_top=2, torso=(8, 4, 3), arm=(2, 8), leg=(2, 1), eye=(1, 2),
        eye_gap=2, eye_y=0.6,
    ),
    Template(
        "hd2d", "HD-2D 5등신", "고해상 도트 RPG 의 늘씬한 등신 — 작은 머리, 긴 팔다리",
        (24, 40), head=(10, 10), head_top=1, torso=(11, 5, 4), arm=(3, 13), leg=(3, 1), eye=(1, 2),
        eye_gap=2, eye_y=0.6, neck=1,
    ),
    Template(
        "minihead", "큰 머리 미니", "사이드뷰 MMO 계열의 아주 큰 머리 — 머리가 키의 절반 이상",
        (24, 28), head=(20, 17), head_top=0, torso=(5, 4, 3), arm=(2, 4), leg=(2, 1), eye=(3, 4),
        eye_gap=3, eye_y=0.6,
    ),
    Template(
        "retro8", "8비트 레트로", "8비트 시절의 16×16 — 그늘 없이 외곽선과 두 단계",
        (16, 16), head=(10, 8), head_top=0, torso=(4, 4, 3), arm=(2, 3), leg=(2, 1), eye=(1, 2),
        eye_gap=2, foot_h=1, flat=True,
    ),
]


# ─────────────────────────────────────────────────────────────────────────────
# 비트맵 — 칸 하나. 값은 팔레트 문자 (None = 투명)
# ─────────────────────────────────────────────────────────────────────────────


class Bitmap:
    def __init__(self, w, h):
        self.w, self.h = w, h
        self.px = [[None] * w for _ in range(h)]

    def get(self, x, y):
        if 0 <= x < self.w and 0 <= y < self.h:
            return self.px[y][x]
        return None

    def put(self, x, y, c):
        if 0 <= x < self.w and 0 <= y < self.h:
            self.px[y][x] = c

    def flip(self):
        out = Bitmap(self.w, self.h)
        for y in range(self.h):
            out.px[y] = list(reversed(self.px[y]))
        return out

    def stamp(self, other):
        for y in range(self.h):
            for x in range(self.w):
                if other.px[y][x] is not None:
                    self.px[y][x] = other.px[y][x]


def ellipse_mask(x0, y0, w, h):
    """사각형에 내접하는 채운 타원 (WorldCanvas `ellipse` 와 같은 규칙)."""
    out = set()
    cx, cy = x0 + w / 2, y0 + h / 2
    rx, ry = w / 2, h / 2
    for y in range(y0, y0 + h):
        for x in range(x0, x0 + w):
            dx = (x + 0.5 - cx) / rx
            dy = (y + 0.5 - cy) / ry
            if dx * dx + dy * dy <= 1.0:
                out.add((x, y))
    return out


def rect_mask(x0, y0, w, h):
    return {(x, y) for y in range(y0, y0 + h) for x in range(x0, x0 + w)}


def trap_mask(y0, y1, top, bottom):
    """위 `(x0, x1)` 에서 아래 `(x0, x1)` 로 퍼지는 사다리꼴 (양 끝 포함)."""
    out = set()
    n = max(1, y1 - y0)
    for y in range(y0, y1 + 1):
        t = (y - y0) / n
        a = round(top[0] + (bottom[0] - top[0]) * t)
        b = round(top[1] + (bottom[1] - top[1]) * t)
        for x in range(a, b + 1):
            out.add((x, y))
    return out


def poly_mask(pts):
    """채운 다각형 — 픽셀 중심 기준 짝홀 규칙."""
    out = set()
    xs = [p[0] for p in pts]
    ys = [p[1] for p in pts]
    for y in range(math.floor(min(ys)), math.ceil(max(ys)) + 1):
        for x in range(math.floor(min(xs)), math.ceil(max(xs)) + 1):
            px, py = x + 0.5, y + 0.5
            inside = False
            j = len(pts) - 1
            for i in range(len(pts)):
                xi, yi = pts[i]
                xj, yj = pts[j]
                if (yi > py) != (yj > py) and px < (xj - xi) * (py - yi) / (yj - yi) + xi:
                    inside = not inside
                j = i
            if inside:
                out.add((x, y))
    return out


def bounds(mask):
    xs = [p[0] for p in mask]
    ys = [p[1] for p in mask]
    return min(xs), min(ys), max(xs), max(ys)


def blob(bmp, mask, flat=False, outline=True, light=0.5, hl=False):
    """모양 하나를 칠한다 — 안쪽 외곽선 + 왼쪽 위 빛. `light` 가 클수록 밝은 면이 넓다."""
    if not mask:
        return
    x0, y0, x1, y1 = bounds(mask)
    w = max(1, x1 - x0)
    h = max(1, y1 - y0)
    for (x, y) in mask:
        edge = outline and any((x + dx, y + dy) not in mask for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)))
        if edge:
            c = "O"
        elif flat:
            c = "M"
        else:
            v = (x - x0) / w * 0.6 + (y - y0) / h * 0.4
            if v > 0.78 - light * 0.2:
                c = "D"
            elif v < 0.12 + light * 0.25:
                c = "L"
            else:
                c = "M"
        bmp.put(x, y, c)
    if hl and not flat and w >= 5 and h >= 4:
        bmp.put(x0 + max(1, w // 4), y0 + max(1, h // 4), "H")


def column(bmp, x, y0, y1, w, flat=False):
    """세로 원기둥(팔·다리) — 왼쪽 밝게, 오른쪽 어둡게, 안쪽 외곽선."""
    mask = rect_mask(x, y0, w, max(1, y1 - y0 + 1))
    for (px, py) in mask:
        edge = px == x or px == x + w - 1 or py == y1
        if w <= 2:
            edge = px == x + w - 1 or py == y1
        if edge:
            c = "O"
        elif flat:
            c = "M"
        elif px == x + 1:
            c = "L"
        elif px == x + w - 2 and w >= 4:
            c = "D"
        else:
            c = "M"
        bmp.put(px, py, c)
    return mask


def mirror_x(t, x):
    return t.w - 1 - x


def sym(mask, t):
    """왼쪽에 그린 모양을 중심선 대칭으로 더한다."""
    return mask | {(mirror_x(t, x), y) for (x, y) in mask}


# ─────────────────────────────────────────────────────────────────────────────
# 기하 — 템플릿·방향·자세에서 부위 위치를 계산한다. 몸과 모든 파츠가 이것을 쓴다.
# ─────────────────────────────────────────────────────────────────────────────


class Pose:
    def __init__(self, bob=0, stride=0):
        self.bob = bob  # 상체가 내려앉는 양 (0/1)
        self.stride = stride  # 걸음 위상 (-1, 0, +1)


POSES = {
    "Idle": [Pose(0, 0), Pose(1, 0)],
    "Walk": [Pose(1, 1), Pose(0, 0), Pose(1, -1), Pose(0, 0)],
}


class Geo:
    """한 프레임의 부위 위치. `side` 는 동쪽(오른쪽)을 보는 옆모습."""

    def __init__(self, t, d, pose):
        self.t, self.d, self.pose = t, d, pose
        b = pose.bob
        cx = t.cx
        self.side = d == "E"
        self.back = d == "N"
        # 머리
        hx = cx - t.head_w // 2
        if self.side:
            hx += 1  # 옆모습은 얼굴 쪽으로 살짝
        self.head = (hx, t.head_top + b, t.head_w, t.head_h)
        self.shoulder_y = t.shoulder_y + b
        self.hip_y = t.hip_y + b
        self.ground = t.ground
        self.foot_top = t.ground - t.foot_h + 1
        # 몸통 — 앞뒤는 어깨·허리 반폭, 옆은 두께를 줄인다
        if self.side:
            half_s = max(2, t.shoulder - 2)
            half_w = max(2, t.waist - 1)
            self.torso_x = (cx - half_s, cx + half_s - 1, cx - half_w, cx + half_w - 1)
        else:
            self.torso_x = (cx - t.shoulder, cx + t.shoulder - 1, cx - t.waist, cx + t.waist - 1)
        # 다리 — (먼 쪽인가, x, 위, 아래(발목))
        s = pose.stride
        self.legs = []
        self.feet = []
        if self.side:
            lx = cx - t.leg_w // 2
            for far, off in ((True, -s), (False, s)):
                x = lx + off * 2 - (1 if far else 0)
                self.legs.append((far, x, self.hip_y, self.foot_top - 1))
                self.feet.append((far, x, self.foot_top, t.leg_w + 1, t.foot_h))
        else:
            for i, sign in enumerate((-1, 1)):
                lift = 1 if (s == 1 and sign == 1) or (s == -1 and sign == -1) else 0
                if sign < 0:
                    x = cx - t.leg_gap - t.leg_w
                else:
                    x = cx + t.leg_gap
                self.legs.append((False, x, self.hip_y, self.foot_top - 1 - lift))
                self.feet.append((False, x, self.foot_top - lift, t.leg_w, t.foot_h))
        # 팔 — (먼 쪽인가, x, 위, 아래)
        self.arms = []
        ay0 = self.shoulder_y + 1
        ay1 = ay0 + t.arm_len - 1
        if self.side:
            ax = cx - t.arm_w // 2 - s
            self.arms.append((True, ax + s * 2 - 1, ay0, ay1))
            self.arms.append((False, ax, ay0, ay1))
        else:
            # 앞뒤 모습의 팔 흔들기 — 앞으로 나간 팔이 1px 내려와 보인다 (다리와 반대쪽).
            sl, sr = self.torso_x[0], self.torso_x[1]
            for i, x in enumerate((sl - t.arm_w + 1, sr)):
                dy = 1 if (s == 1 and i == 0) or (s == -1 and i == 1) else 0
                self.arms.append((False, x, ay0 + dy, ay1 + dy))

    # 몸통 사다리꼴
    def torso_mask(self, y0=None, y1=None, grow=0, flare=0):
        a, b, c, d = self.torso_x
        top = self.shoulder_y if y0 is None else y0
        bot = self.hip_y if y1 is None else y1
        return trap_mask(top, bot, (a - grow, b + grow), (c - grow - flare, d + grow + flare))

    def head_mask(self, grow=0):
        x, y, w, h = self.head
        return ellipse_mask(x - grow, y - grow, w + 2 * grow, h + 2 * grow)

    def eye_row(self):
        x, y, w, h = self.head
        return y + round(h * self.t.eye_y) - self.t.eye[1] // 2


# ─────────────────────────────────────────────────────────────────────────────
# 그리기 — 파츠 하나 = 함수 하나. (bmp, geo) 를 받아 그 프레임을 칠한다.
# ─────────────────────────────────────────────────────────────────────────────


def draw_body(bmp, g):
    """맨몸 — 피부 전체 (머리·팔·다리·손발). 속옷 없이, 기본 옷 파츠가 덮는다."""
    t = g.t
    flat = t.flat
    # 먼 팔·먼 다리를 먼저 (옆모습)
    for far, x, y0, y1 in g.arms:
        if far:
            column(bmp, x, y0, y1, t.arm_w, flat)
    for far, x, y0, y1 in g.legs:
        if far:
            column(bmp, x, y0, y1, t.leg_w, flat)
    for far, x, y, w, h in g.feet:
        if far:
            blob(bmp, rect_mask(x, y, w, h), flat)
    if t.tail and g.back:
        pass  # 꼬리는 파츠 (앞뒤가 방향마다 다르다)
    blob(bmp, g.torso_mask(), flat, light=0.6)
    for far, x, y0, y1 in g.legs:
        if not far:
            column(bmp, x, y0, y1, t.leg_w, flat)
    for far, x, y, w, h in g.feet:
        if not far:
            blob(bmp, rect_mask(x, y, w + (1 if g.side else 0), h), flat)
    blob(bmp, g.head_mask(), flat, light=0.55, hl=False)
    for far, x, y0, y1 in g.arms:
        if not far:
            column(bmp, x, y0, y1, t.arm_w, flat)


def draw_side_ears(bmp, g):
    """머리 옆으로 뻗는 귀 (엘프·오크) — 피부색. 머리카락보다 앞에 둬야 보인다."""
    t = g.t
    x, y, w, h = g.head
    ey = y + h // 2 - 1
    if t.ears == "elf":
        if g.side:
            cx = x + w // 2 - 1
            m = poly_mask([(cx + 1, ey - 2), (cx - 5, ey - 5), (cx - 4, ey - 3), (cx, ey + 4)])
        else:
            m = sym(poly_mask([(x + 3, ey - 2), (x - 4, ey - 5), (x - 3, ey - 3), (x + 3, ey + 4)]), t)
    else:  # round
        if g.side:
            m = ellipse_mask(x + w // 2 - 2, ey, 3, 4)
        else:
            m = sym(ellipse_mask(x - 1, ey, 3, 4), t)
    if g.back and t.ears == "round":
        m = sym(ellipse_mask(x - 1, ey, 3, 4), t)
    blob(bmp, m, t.flat)


def draw_tusks(bmp, g):
    """오크 엄니 — 아랫입술에서 솟는 두 개. 색 그대로."""
    if g.back:
        return
    x, y, w, h = g.head
    my = y + h - 3
    if g.side:
        bmp.put(x + w - 3, my, "t")
        bmp.put(x + w - 3, my - 1, "t")
    else:
        cx = g.t.cx
        for dx in (-3, 2):
            bmp.put(cx + dx, my, "t")
            bmp.put(cx + dx, my - 1, "t")


def head_feature_x(g):
    """옆모습 얼굴 쪽 끝 x."""
    x, y, w, h = g.head
    return x + w - 1


def draw_eyes(style):
    """눈 — 무채색 홍채(색을 곱한다) + 짙은 눈꺼풀 + 흰 반짝임. 뒷모습에는 없다."""

    def f(bmp, g):
        if g.back:
            return
        t = g.t
        ew, eh = t.eye
        ey = g.eye_row()
        if g.side:
            xs = [head_feature_x(g) - 2 - ew]
        else:
            xs = [t.cx - t.eye_gap - ew, t.cx + t.eye_gap]
        for ex in xs:
            if style == "round":
                for yy in range(eh):
                    for xx in range(ew):
                        bmp.put(ex + xx, ey + yy, "i" if yy > 0 else "e")
                if ew >= 2 and eh >= 3:
                    bmp.put(ex + ew - 1, ey + 1, "w")
                elif eh >= 2:
                    bmp.put(ex, ey + eh - 1, "i")
            elif style == "sharp":
                for xx in range(ew + 1):
                    bmp.put(ex + xx - (1 if not g.side and ex < t.cx else 0), ey, "e")
                for yy in range(1, eh):
                    for xx in range(ew):
                        bmp.put(ex + xx, ey + yy, "i")
                bmp.put(ex + (ew - 1 if ex < t.cx else 0), ey + eh - 1, "e")
            elif style == "sleepy":
                for xx in range(ew):
                    bmp.put(ex + xx, ey + eh - 2, "e")
                    bmp.put(ex + xx, ey + eh - 1, "i")
            elif style == "smile":
                # 웃는 눈 — 아래로 둥근 선
                if ew >= 2:
                    bmp.put(ex, ey + 1, "e")
                    bmp.put(ex + ew - 1, ey + 1, "e")
                    for xx in range(1, ew - 1):
                        bmp.put(ex + xx, ey, "e")
                    if ew == 2:
                        bmp.put(ex, ey, "e")
                        bmp.put(ex + 1, ey, "e")
                else:
                    bmp.put(ex, ey + 1, "e")
            elif style == "big":
                for yy in range(eh + 1):
                    for xx in range(ew):
                        bmp.put(ex + xx, ey - 1 + yy, "i" if yy > 0 else "e")
                bmp.put(ex + ew - 1, ey, "w")
                if ew >= 2:
                    bmp.put(ex, ey + eh - 1, "w")

    return f


def draw_face(style):
    """입·볼 — 색 그대로 (tinted 아님)."""

    def f(bmp, g):
        if g.back:
            return
        t = g.t
        x, y, w, h = g.head
        ey = g.eye_row()
        my = ey + t.eye[1] + max(1, t.head_h // 8)
        my = min(my, y + h - 2)
        if g.side:
            mx = head_feature_x(g) - 2
            if style != "plain":
                bmp.put(mx, my, "m")
            if t.head_h >= 12:
                bmp.put(mx - 2 - t.eye[0], my - 1, "b")
        else:
            cx = t.cx
            if style == "smile":
                bmp.put(cx - 1, my, "m")
                bmp.put(cx, my, "m")
            elif style == "open":
                bmp.put(cx - 1, my, "m")
                bmp.put(cx, my, "m")
                if t.head_h >= 12:
                    bmp.put(cx - 1, my + 1, "r")
                    bmp.put(cx, my + 1, "r")
            elif style == "cat":
                bmp.put(cx - 2, my, "m")
                bmp.put(cx - 1, my + 1, "m")
                bmp.put(cx, my + 1, "m")
                bmp.put(cx + 1, my, "m")
            # 볼
            if t.head_h >= 12:
                bx = cx - t.eye_gap - t.eye[0] - 1
                bmp.put(bx, my - 1, "b")
                bmp.put(mirror_x(t, bx), my - 1, "b")

    return f


FACE_COLORS = {
    "e": (43, 29, 31, 255),  # 눈꺼풀·눈동자 테두리
    "w": (255, 255, 255, 255),
    "m": (120, 52, 52, 255),  # 입
    "r": (205, 90, 90, 255),  # 입 안
    "b": (240, 150, 140, 255),  # 볼
    "t": (236, 228, 200, 255),  # 엄니
    "f": (196, 120, 90, 255),  # 주근깨
}


# ── 머리카락 ────────────────────────────────────────────────────────────────


def hair_cap(g, fringe=0.42, sides=0.7, side_w=2):
    """머리를 덮는 기본 모양 — 정수리에서 앞머리 선까지 + 옆머리. 방향마다 다르다."""
    x, y, w, h = g.head
    cap = g.head_mask(grow=1)
    if g.back:
        # 뒷모습은 목덜미만 남기고 전부
        return {(px, py) for (px, py) in cap if py <= y + h - 2}
    if g.side:
        face_x = x + int(w * 0.5)
        face_y = y + int(h * fringe)
        return {
            (px, py)
            for (px, py) in cap
            if py < face_y or px < face_x or (py <= y + h * sides and px < face_x + 1)
        }
    line = y + int(h * fringe)
    out = {(px, py) for (px, py) in cap if py < line}
    low = y + int(h * sides)
    for (px, py) in cap:
        if py <= low and (px < x + side_w or px > x + w - 1 - side_w):
            out.add((px, py))
    return out


def notch_fringe(mask, g, every=3):
    """앞머리 끝을 들쭉날쭉하게 — 아래 끝 줄에서 몇 칸마다 한 칸 내린다."""
    if g.back:
        return mask
    out = set(mask)
    bottom = {}
    for (px, py) in mask:
        bottom[px] = max(bottom.get(px, -1), py)
    x0 = g.head[0]
    for px, py in bottom.items():
        if (px - x0) % every == 1 and g.head[1] < py < g.head[1] + g.head[3] * 0.6:
            out.add((px, py + 1))
    return out


def shade_hair(bmp, mask, g):
    blob(bmp, mask, g.t.flat, light=0.55, hl=True)
    if g.t.flat:
        return
    # 머리결 — 정수리에서 내려오는 짙은 줄 두어 개
    x, y, w, h = g.head
    for sx in (x + w // 3, x + 2 * w // 3):
        for yy in range(y + 1, y + h // 3):
            if (sx, yy) in mask and bmp.get(sx, yy) == "M":
                bmp.put(sx, yy, "D")


def hair_style(style):
    def f(bmp, g):
        x, y, w, h = g.head
        t = g.t
        if style == "bald":
            return
        if style == "short":
            m = notch_fringe(hair_cap(g, 0.38, 0.5, 2), g)
        elif style == "spiky":
            m = hair_cap(g, 0.35, 0.5, 2)
            n = max(3, w // 4)
            for i in range(n):
                sx = x + 1 + i * (w - 2) // n
                m |= poly_mask([(sx, y + 2), (sx + 2, y - 3), (sx + 4, y + 2)])
            m = notch_fringe(m, g, 2)
        elif style == "bob":
            m = hair_cap(g, 0.4, 0.95, 3)
        elif style == "long":
            m = hair_cap(g, 0.4, 1.0, 3)
        elif style == "parted":
            m = hair_cap(g, 0.3, 0.75, 2)
            if not g.back and not g.side:
                # 한쪽으로 넘긴 앞머리
                m |= poly_mask([(x + 1, y + h * 0.3), (x + w * 0.6, y + h * 0.3), (x + 2, y + h * 0.62)])
        elif style == "mohawk":
            m = set()
            if g.side:
                m = {(px, py) for (px, py) in g.head_mask(1) if py < y + h * 0.3}
                m |= poly_mask([(x + 1, y + 1), (x + w * 0.5, y - 4), (x + w - 1, y + 1)])
            else:
                c = t.cx
                m = rect_mask(c - 2, y - 3, 4, h // 2 + 3) & (g.head_mask(1) | rect_mask(c - 2, y - 4, 4, 5))
        elif style in ("ponytail", "twintail", "bun"):
            m = notch_fringe(hair_cap(g, 0.38, 0.55, 2), g)
            if style == "bun":
                if g.side:
                    m |= ellipse_mask(x + 1, y - 3, 6, 6)
                else:
                    m |= ellipse_mask(t.cx - 3, y - 4, 6, 6)
        else:
            raise ValueError(style)
        shade_hair(bmp, m, g)

    return f


def hair_back_style(style):
    """몸 뒤로 늘어지는 머리 — 긴 머리·포니테일·트윈테일. 뒷모습에서는 몸 앞에 온다."""

    def f(bmp, g):
        x, y, w, h = g.head
        t = g.t
        m = set()
        if style == "long":
            bottom = min(g.hip_y - 1, g.ground - 4)
            if g.side:
                m = trap_mask(y + h // 3, bottom, (x - 1, x + w // 2), (x - 1, x + w // 2 - 2))
            else:
                m = trap_mask(y + h // 2, bottom, (x, x + w - 1), (x + 1, x + w - 2))
        elif style == "ponytail":
            if g.side:
                m = poly_mask([(x + 1, y + h * 0.35), (x - 4, y + h * 0.6), (x - 3, y + h + 4), (x + 2, y + h * 0.7)])
            elif g.back:
                m = trap_mask(y + h // 3, y + h + 4, (t.cx - 2, t.cx + 1), (t.cx - 1, t.cx))
                m |= ellipse_mask(t.cx - 2, y + h // 3 - 1, 4, 3)
            else:
                m = poly_mask([(t.cx - 3, y + 1), (t.cx + 3, y + 1), (t.cx + 2, y - 2), (t.cx - 2, y - 2)])
        elif style == "twintail":
            for sign in (-1, 1):
                if g.side and sign < 0:
                    continue
                bx = x - 2 if sign < 0 else x + w - 2
                if g.side:
                    bx = x - 1
                m |= poly_mask([(bx, y + h * 0.3), (bx + 4, y + h * 0.3), (bx + 3, y + h + 5), (bx + 1, y + h + 5)])
        if m:
            blob(bmp, m, t.flat, light=0.5)

    return f


# ── 옷 ──────────────────────────────────────────────────────────────────────


def sleeves(g, upto):
    """소매 — 팔 위쪽 `upto` 비율. 1.0 이면 손목까지(손 2px 남김)."""
    out = []
    for far, x, y0, y1 in g.arms:
        end = y0 + max(1, round((y1 - y0 + 1) * upto)) - 1
        if upto >= 1.0:
            end = y1 - 2
        out.append((far, x, y0 - 1, end))
    return out


def draw_top(style):
    def f(bmp, g):
        t = g.t
        flat = t.flat
        arms = []
        if style == "tunic":
            body = g.torso_mask(y1=g.hip_y + 1, flare=1)
            arms = sleeves(g, 0.45)
        elif style == "shirt":
            body = g.torso_mask()
            arms = sleeves(g, 1.0)
        elif style == "robe":
            body = g.torso_mask()
            body |= trap_mask(g.hip_y, g.foot_top - 1, (g.torso_x[2] - 1, g.torso_x[3] + 1), (g.torso_x[2] - 3, g.torso_x[3] + 3))
            arms = sleeves(g, 1.0)
        elif style == "dress":
            body = g.torso_mask()
            hem = g.hip_y + max(2, (g.foot_top - g.hip_y) * 2 // 3)
            body |= trap_mask(g.hip_y, hem, (g.torso_x[2], g.torso_x[3]), (g.torso_x[2] - 3, g.torso_x[3] + 3))
        elif style == "armor":
            body = g.torso_mask(grow=1, y1=g.hip_y + 1)
            arms = sleeves(g, 0.6)
        elif style == "coat":
            body = g.torso_mask()
            body |= trap_mask(g.hip_y, g.hip_y + max(2, (g.foot_top - g.hip_y) // 2), (g.torso_x[2], g.torso_x[3]), (g.torso_x[2] - 1, g.torso_x[3] + 1))
            arms = sleeves(g, 1.0)
        else:
            raise ValueError(style)
        for far, x, y0, y1 in arms:
            if far:
                column(bmp, x, y0, y1, t.arm_w, flat)
        blob(bmp, body, flat, light=0.6, hl=style == "armor")
        if not flat:
            # 허리띠·앞섶 — 옷 위의 짙은 선
            if style in ("tunic", "coat", "robe"):
                for px in range(g.torso_x[2], g.torso_x[3] + 1):
                    if (px, g.hip_y - 1) in body and bmp.get(px, g.hip_y - 1) != "O":
                        bmp.put(px, g.hip_y - 1, "D")
            if style == "coat" and g.d == "S":
                for py in range(g.shoulder_y + 1, g.hip_y + (g.foot_top - g.hip_y) // 2):
                    if bmp.get(t.cx, py) not in (None, "O"):
                        bmp.put(t.cx, py, "D")
            if style == "armor":
                # 판금 줄
                for py in (g.shoulder_y + 2, g.shoulder_y + g.t.torso_h // 2 + 1):
                    for px in range(g.torso_x[0], g.torso_x[1] + 1):
                        if bmp.get(px, py) in ("M", "L"):
                            bmp.put(px, py, "D")
        for far, x, y0, y1 in arms:
            if not far:
                column(bmp, x, y0, y1, t.arm_w, flat)
        if style == "armor" and not g.side:
            # 견갑
            sx = g.torso_x[0] - t.arm_w + 1
            pad = ellipse_mask(sx - 1, g.shoulder_y - 1, t.arm_w + 3, 4)
            blob(bmp, sym(pad, t), flat, hl=True)
        elif style == "armor":
            blob(bmp, ellipse_mask(g.arms[1][1] - 1, g.shoulder_y - 1, t.arm_w + 2, 4), flat, hl=True)

    return f


def draw_bottom(style):
    def f(bmp, g):
        t = g.t
        flat = t.flat
        if style == "pants":
            stop = None
        elif style == "shorts":
            stop = 0.45
        elif style == "skirt":
            hem = g.hip_y + max(2, (g.foot_top - g.hip_y) // 2)
            m = trap_mask(g.hip_y - 1, hem, (g.torso_x[2], g.torso_x[3]), (g.torso_x[2] - 2, g.torso_x[3] + 2))
            blob(bmp, m, flat)
            return
        else:
            raise ValueError(style)
        for far, x, y0, y1 in sorted(g.legs, key=lambda l: not l[0]):
            end = y1 if stop is None else y0 + max(1, round((y1 - y0 + 1) * stop))
            column(bmp, x, y0 - 1, end, t.leg_w, flat)
        if not g.side:
            # 가랑이 — 두 다리 사이 위쪽
            a = g.legs[0][1] + t.leg_w - 1
            b = g.legs[1][1]
            blob(bmp, rect_mask(a, g.hip_y - 1, b - a + 1, 2), flat, outline=False)
            for px in range(a, b + 1):
                bmp.put(px, g.hip_y, "O")

    return f


def draw_shoes(style):
    def f(bmp, g):
        t = g.t
        for far, x, y, w, h in sorted(g.feet, key=lambda p: not p[0]):
            fw = w + (1 if g.side else 0)
            if style == "boots":
                leg = next(l for l in g.legs if l[1] == x or g.side)
                top = max(leg[2], leg[3] - max(1, (leg[3] - leg[2]) // 2))
                blob(bmp, rect_mask(x, top, t.leg_w, y - top) | rect_mask(x, y, fw, h), t.flat)
            elif style == "shoes":
                blob(bmp, rect_mask(x, y, fw, h), t.flat)
            else:
                raise ValueError(style)

    return f


# ── 머리에 쓰는 것 ───────────────────────────────────────────────────────────


def draw_hat(style):
    def f(bmp, g):
        x, y, w, h = g.head
        t = g.t
        flat = t.flat
        if style == "cap":
            dome = {(px, py) for (px, py) in g.head_mask(1) if py < y + h * 0.36}
            blob(bmp, dome, flat, hl=True)
            by = y + int(h * 0.36)
            if g.side:
                brim = rect_mask(x + w // 2, by - 1, w // 2 + 3, 2)
            else:
                brim = rect_mask(x - 1, by - 1, w + 2, 2)
            blob(bmp, brim, flat)
        elif style == "wizard":
            by = y + int(h * 0.3)
            tip = (x + w // 2 - (3 if g.side else 0), y - max(8, h * 0.8))
            cone = poly_mask([(x + 1, by), tip, (x + w - 1, by)])
            blob(bmp, cone, flat, hl=True)
            blob(bmp, rect_mask(x - 2, by - 1, w + 4, 2), flat)
        elif style == "helmet":
            m = {(px, py) for (px, py) in g.head_mask(1) if py < y + h * 0.62}
            if not g.back:
                if g.side:
                    m -= {(px, py) for (px, py) in m if px > x + w * 0.65 and py > y + h * 0.42}
                else:
                    m -= rect_mask(x + 3, y + int(h * 0.45), w - 6, h)
            blob(bmp, m, flat, hl=True)
            if not flat and not g.back:
                # 이마 띠
                for (px, py) in m:
                    if py == y + int(h * 0.38) and bmp.get(px, py) != "O":
                        bmp.put(px, py, "D")
        elif style == "hood":
            m = g.head_mask(2)
            m |= g.torso_mask(y1=g.shoulder_y + 2, grow=1)
            if not g.back:
                if g.side:
                    face = ellipse_mask(x + w // 2, y + h // 4, w // 2 + 2, h - 2)
                else:
                    face = ellipse_mask(x + 2, y + h // 4, w - 4, h * 3 // 4 + 1)
                m -= face
            blob(bmp, m, flat, light=0.5)
        elif style == "headband":
            by = y + int(h * 0.28)
            m = {(px, py) for (px, py) in g.head_mask(1) if by <= py <= by + 1}
            blob(bmp, m, flat, outline=False)
            for (px, py) in m:
                if py == by + 1:
                    bmp.put(px, py, "D")
            if g.back:
                # 매듭 끈
                blob(bmp, poly_mask([(t.cx, by), (t.cx + 4, by + 4), (t.cx + 2, by + 5)]), flat)
        elif style == "crown":
            by = y + 1
            cw = max(6, w // 2)
            cx0 = (x + w // 2 - cw // 2 + (1 if g.side else 0))
            m = rect_mask(cx0, by - 1, cw, 2)
            for i in range(3):
                px = cx0 + i * (cw - 1) // 2
                m |= rect_mask(px, by - 3, 1, 2)
            blob(bmp, m, flat, outline=False)
            for (px, py) in m:
                if py == by:
                    bmp.put(px, py, "D")
        elif style == "ribbon":
            if g.side:
                bx = x + 1
            else:
                bx = x + w - 6
            m = poly_mask([(bx, y - 1), (bx + 2.5, y + 1.5), (bx, y + 4)]) | poly_mask([(bx + 5, y - 1), (bx + 2.5, y + 1.5), (bx + 5, y + 4)])
            blob(bmp, m, flat)
        else:
            raise ValueError(style)

    return f


def draw_cape(bmp, g):
    """망토 — 어깨에서 무릎 아래까지. 앞모습은 몸 뒤, 뒷모습은 몸 앞."""
    t = g.t
    bottom = g.foot_top - 2
    if g.side:
        m = trap_mask(g.shoulder_y, bottom, (g.torso_x[0] - 1, g.torso_x[0] + 1), (g.torso_x[0] - 4, g.torso_x[0] + 1))
    else:
        m = trap_mask(g.shoulder_y, bottom, (g.torso_x[0] - 1, g.torso_x[1] + 1), (g.torso_x[0] - 3, g.torso_x[1] + 3))
    blob(bmp, m, t.flat, light=0.5)


# ── 종족 특징 ───────────────────────────────────────────────────────────────


def draw_cat_ears(bmp, g):
    x, y, w, h = g.head
    t = g.t
    if g.side:
        m = poly_mask([(x + w * 0.25, y + 3), (x + w * 0.35, y - 3), (x + w * 0.6, y + 2)])
    else:
        m = sym(poly_mask([(x + 1, y + 4), (x + 2, y - 3), (x + 6, y + 1)]), t)
    blob(bmp, m, t.flat)
    if not g.back and not t.flat:
        for (px, py) in m:
            if bmp.get(px, py) == "M" and py > y - 1:
                bmp.put(px, py, "L")


def draw_horns(bmp, g):
    x, y, w, h = g.head
    t = g.t
    if g.side:
        m = poly_mask([(x + w * 0.4, y + 3), (x + w * 0.3, y - 4), (x + w * 0.62, y + 2)])
    else:
        m = sym(poly_mask([(x + 3, y + 3), (x + 1, y - 4), (x + 6, y + 1)]), t)
    blob(bmp, m, t.flat, hl=False)


def draw_tail(kind):
    def f(bmp, g):
        t = g.t
        hip = g.hip_y - 1
        cx = t.cx
        if kind == "cat":
            if g.side:
                pts = [(cx - 2, hip), (cx - 7, hip - 4), (cx - 8, hip - 9), (cx - 6, hip - 9), (cx - 5, hip - 4), (cx - 1, hip - 1)]
            else:
                pts = [(cx, hip), (cx + 5, hip - 3), (cx + 7, hip - 8), (cx + 9, hip - 8), (cx + 7, hip - 2), (cx + 1, hip + 2)]
            m = poly_mask(pts)
        else:  # devil — 가는 꼬리 + 화살촉
            if g.side:
                m = poly_mask([(cx - 2, hip), (cx - 8, hip + 3), (cx - 8, hip + 4), (cx - 2, hip + 1)])
                m |= poly_mask([(cx - 8, hip + 1), (cx - 11, hip + 3.5), (cx - 8, hip + 6)])
            else:
                m = poly_mask([(cx, hip), (cx + 7, hip + 1), (cx + 7, hip + 2), (cx, hip + 1.5)])
                m |= poly_mask([(cx + 6, hip - 1), (cx + 10, hip + 1.5), (cx + 6, hip + 4)])
        blob(bmp, m, t.flat, light=0.5)

    return f


# ── 얼굴 꾸미기 — 수염·안경·안대·주근깨 ───────────────────────────────────────


def draw_beard(kind):
    def f(bmp, g):
        if g.back:
            return
        x, y, w, h = g.head
        t = g.t
        ey = g.eye_row()
        my = ey + t.eye[1] + max(1, t.head_h // 8)
        if kind == "full":
            if g.side:
                m = {(px, py) for (px, py) in g.head_mask(1) if py >= my - 1 and px >= x + w // 3}
                m |= ellipse_mask(x + w // 2, y + h - 3, w // 2 + 1, 5)
            else:
                m = {(px, py) for (px, py) in g.head_mask(1) if py >= my - 1}
                m |= ellipse_mask(x + 2, y + h - 4, w - 4, 6)
                # 입 자리는 비운다
                m -= {(t.cx - 1, my), (t.cx, my)}
            blob(bmp, m, t.flat, light=0.5)
        else:  # mustache
            if g.side:
                fx = x + w - 3
                for dx in range(3):
                    bmp.put(fx - dx, my - 1, "M")
                bmp.put(fx - 2, my, "O")
            else:
                for dx in range(-3, 3):
                    bmp.put(t.cx + dx, my - 1, "O" if dx in (-3, 2) else "M")
                bmp.put(t.cx - 3, my, "O")
                bmp.put(t.cx + 2, my, "O")

    return f


def draw_glasses(bmp, g):
    if g.back:
        return
    t = g.t
    ew, eh = t.eye
    ey = g.eye_row()
    if g.side:
        ex = head_feature_x(g) - 2 - ew
        m = {(px, py) for px in range(ex - 1, ex + ew + 1) for py in range(ey - 1, ey + eh + 1)}
        ring = {(px, py) for (px, py) in m if px in (ex - 1, ex + ew) or py in (ey - 1, ey + eh)}
        for (px, py) in ring:
            bmp.put(px, py, "O")
        for px in range(g.head[0] + 2, ex - 1):
            bmp.put(px, ey, "O")
        return
    for ex in (t.cx - t.eye_gap - ew, t.cx + t.eye_gap):
        for px in range(ex - 1, ex + ew + 1):
            for py in range(ey - 1, ey + eh + 1):
                if px in (ex - 1, ex + ew) or py in (ey - 1, ey + eh):
                    bmp.put(px, py, "O")
    for px in range(t.cx - t.eye_gap, t.cx + t.eye_gap):
        bmp.put(px, ey, "O")


def draw_eyepatch(bmp, g):
    t = g.t
    x, y, w, h = g.head
    ew, eh = t.eye
    ey = g.eye_row()
    # 끈 — 머리를 비스듬히 두른다
    for px in range(x, x + w):
        py = ey - 2 + round((px - x) * 4 / max(1, w))
        if (px, py) in g.head_mask(1):
            bmp.put(px, py, "O")
    if g.back:
        return
    if g.side:
        ex = head_feature_x(g) - 2 - ew
    else:
        ex = t.cx + t.eye_gap
    blob(bmp, rect_mask(ex - 1, ey - 1, ew + 2, eh + 2), t.flat, outline=False)


def draw_freckles(bmp, g):
    if g.back or g.t.head_h < 11:
        return
    t = g.t
    ey = g.eye_row() + t.eye[1] + 1
    if g.side:
        fx = head_feature_x(g) - 4
        for dx, dy in ((0, 0), (2, 0), (1, 1)):
            bmp.put(fx - dx, ey + dy, "f")
        return
    for sign in (-1, 1):
        bx = t.cx - t.eye_gap - t.eye[0] + (0 if sign < 0 else 0)
        pts = ((0, 0), (2, 0), (1, 1))
        for dx, dy in pts:
            px = bx + dx
            bmp.put(px if sign < 0 else mirror_x(t, px), ey + dy, "f")

# ─────────────────────────────────────────────────────────────────────────────
# 파츠 목록 — 카탈로그의 원본
# ─────────────────────────────────────────────────────────────────────────────


def layers(front, back=None, side=None):
    """방향별 앞뒤 `[동, 북, 서, 남]`. 몸 = 0, 큰 쪽이 앞. 뒷모습만 다르면 `back`."""
    side = front if side is None else side
    back = front if back is None else back
    return [side, back, side, front]


class Part:
    def __init__(self, key, name, category, draw, layer, tinted=True, slot=None, palette="cloth", when=None):
        self.key = key
        self.name = name
        self.category = category
        self.draw = draw
        self.layer = layer  # 방향별 목록
        self.tinted = tinted
        self.slot = slot  # 이 자리에 외형 장비를 입으면 빠진다 (WorldEditor `EquipSlot`)
        self.palette = palette  # 견본·커스터마이징이 고를 색 묶음
        self.when = when or (lambda t: True)


# 앞뒤 번호 — 망토(뒤) < 뒷머리 < 꼬리 < 몸 < 눈·얼굴 < 하의 < 신발 < 상의 < 머리카락 < 귀 < 모자 < 뿔
L_CAPE = layers(-3, back=5)
L_HAIR_BACK = layers(-2, back=7)
L_TAIL = layers(-1, back=6)
L_FACE = layers(1)
L_BOTTOM = layers(2)
L_SHOES = layers(3)
L_TOP = layers(4)
L_HAIR = layers(6)
L_EARS = layers(8)
L_HAT = layers(9)
L_HORNS = layers(10)

PARTS = [
    Part("eyes_round", "눈 — 동그란", "eyes", draw_eyes("round"), L_FACE, palette="eyes"),
    Part("eyes_sharp", "눈 — 날카로운", "eyes", draw_eyes("sharp"), L_FACE, palette="eyes"),
    Part("eyes_sleepy", "눈 — 졸린", "eyes", draw_eyes("sleepy"), L_FACE, palette="eyes"),
    Part("eyes_smile", "눈 — 웃는", "eyes", draw_eyes("smile"), L_FACE, palette="eyes"),
    Part("eyes_big", "눈 — 큰 반짝", "eyes", draw_eyes("big"), L_FACE, palette="eyes"),
    Part("face_smile", "입 — 미소", "face", draw_face("smile"), L_FACE, tinted=False),
    Part("face_open", "입 — 벌린", "face", draw_face("open"), L_FACE, tinted=False),
    Part("face_cat", "입 — 고양이", "face", draw_face("cat"), L_FACE, tinted=False),
    Part("face_plain", "입 — 없음", "face", draw_face("plain"), L_FACE, tinted=False),
]
for key, name in [
    ("short", "짧은 머리"), ("spiky", "삐죽 머리"), ("bob", "단발"), ("long", "긴 생머리"),
    ("parted", "옆가르마"), ("mohawk", "모히칸"), ("ponytail", "포니테일"), ("twintail", "트윈테일"),
    ("bun", "올림머리"),
]:
    PARTS.append(Part(f"hair_{key}", name, "hair", hair_style(key), L_HAIR, palette="hair"))
for key, name in [("long", "긴 머리 (뒤)"), ("ponytail", "포니테일 (뒤)"), ("twintail", "트윈테일 (뒤)")]:
    PARTS.append(Part(f"hairback_{key}", name, "hair_back", hair_back_style(key), L_HAIR_BACK, palette="hair"))
for key, name in [
    ("tunic", "튜닉"), ("shirt", "긴소매 셔츠"), ("robe", "로브"), ("dress", "원피스"),
    ("armor", "갑옷 (판금)"), ("coat", "코트"),
]:
    PARTS.append(Part(f"top_{key}", name, "top", draw_top(key), L_TOP, slot="Body"))
for key, name in [("pants", "긴바지"), ("shorts", "반바지"), ("skirt", "치마")]:
    PARTS.append(Part(f"bottom_{key}", name, "bottom", draw_bottom(key), L_BOTTOM))
for key, name in [("boots", "부츠"), ("shoes", "단화")]:
    PARTS.append(Part(f"shoes_{key}", name, "shoes", draw_shoes(key), L_SHOES, slot="Shoes"))
for key, name in [
    ("cap", "모자 (챙)"), ("wizard", "마법사 모자"), ("helmet", "투구"), ("hood", "두건"),
    ("headband", "머리띠"), ("crown", "왕관"), ("ribbon", "리본"),
]:
    PARTS.append(Part(f"hat_{key}", name, "hat", draw_hat(key), L_HAT, slot="Head"))
PARTS += [
    Part("cape", "망토", "cape", draw_cape, L_CAPE),
    Part("ears_cat", "고양이 귀", "feature", draw_cat_ears, L_EARS, palette="hair", when=lambda t: t.ears == "cat"),
    Part("horns", "뿔", "feature", draw_horns, L_HORNS, palette="horn", when=lambda t: t.ears == "horns"),
    Part("tail_cat", "고양이 꼬리", "feature", draw_tail("cat"), L_TAIL, palette="hair", when=lambda t: t.tail == "cat"),
    Part("tail_devil", "마족 꼬리", "feature", draw_tail("devil"), L_TAIL, palette="horn", when=lambda t: t.tail == "devil"),
    Part("tusks", "엄니", "feature", draw_tusks, L_FACE, tinted=False, when=lambda t: t.tusks),
    Part("ears_elf", "엘프 귀", "feature", draw_side_ears, L_EARS, palette="skin", when=lambda t: t.ears == "elf"),
    Part("ears_round", "둥근 귀", "feature", draw_side_ears, L_EARS, palette="skin", when=lambda t: t.ears == "round"),
    Part("beard_full", "턱수염", "facial", draw_beard("full"), layers(5), palette="hair"),
    Part("beard_mustache", "콧수염", "facial", draw_beard("mustache"), layers(5), palette="hair"),
    Part("glasses", "안경", "facial", draw_glasses, layers(7), palette="cloth"),
    Part("eyepatch", "안대", "facial", draw_eyepatch, layers(7), palette="cloth"),
    Part("freckles", "주근깨", "facial", draw_freckles, L_FACE, tinted=False),
]

# 색 묶음 — sRGB. 커스터마이징 화면이 이 중에서 고른다 (자유 색도 된다).
PALETTES = {
    "skin": [
        ("밝은 살구", (255, 228, 205)), ("살구", (247, 205, 170)), ("황갈", (226, 172, 128)),
        ("구릿빛", (196, 134, 92)), ("갈색", (150, 98, 66)), ("짙은 갈색", (105, 68, 48)),
        ("창백", (232, 232, 240)), ("초록 (오크)", (150, 190, 110)), ("푸른 (마족)", (150, 170, 225)),
        ("잿빛 (언데드)", (175, 180, 170)),
    ],
    "hair": [
        ("검정", (60, 56, 64)), ("짙은 갈색", (110, 72, 48)), ("갈색", (160, 104, 62)),
        ("금발", (250, 214, 120)), ("백금", (245, 240, 220)), ("빨강", (210, 72, 58)),
        ("주황", (240, 140, 60)), ("분홍", (245, 150, 190)), ("보라", (150, 110, 210)),
        ("파랑", (90, 140, 230)), ("민트", (120, 210, 190)), ("은발", (200, 205, 215)),
    ],
    "eyes": [
        ("갈색", (150, 90, 50)), ("검정", (70, 64, 72)), ("파랑", (70, 130, 230)),
        ("초록", (80, 170, 90)), ("빨강", (220, 60, 60)), ("금색", (240, 190, 60)),
        ("보라", (160, 100, 220)), ("회색", (150, 160, 170)),
    ],
    "cloth": [
        ("흰색", (245, 245, 245)), ("검정", (70, 70, 80)), ("빨강", (210, 70, 60)),
        ("파랑", (70, 110, 200)), ("남색", (60, 70, 130)), ("초록", (80, 160, 90)),
        ("갈색", (150, 105, 70)), ("베이지", (225, 205, 165)), ("보라", (140, 90, 180)),
        ("노랑", (240, 200, 70)), ("분홍", (240, 150, 180)), ("철", (190, 200, 215)),
        ("황금", (240, 200, 90)),
    ],
    "horn": [("상아", (240, 232, 210)), ("흑요석", (80, 72, 90)), ("핏빛", (190, 60, 60))],
}


# ─────────────────────────────────────────────────────────────────────────────
# 출력
# ─────────────────────────────────────────────────────────────────────────────


def frames(t, draw):
    """`[클립][방향][프레임] -> Bitmap`. 서는 동의 좌우 반전."""
    out = {}
    for clip, n, _ in CLIPS:
        for d in DIRS:
            row = []
            for i in range(n):
                pose = POSES[clip][i]
                bmp = Bitmap(t.w, t.h)
                draw(bmp, Geo(t, "E" if d == "W" else d, pose))
                row.append(bmp.flip() if d == "W" else bmp)
            out[(clip, d)] = row
    return out


def colour(c, tinted, body):
    if c in FACE_COLORS and not tinted:
        return FACE_COLORS[c]
    if c in ("e", "i", "w") and tinted:
        v = {"e": 40, "i": 196, "w": 255}[c]
        return (v, v, v, 255)
    if c == "t":
        return FACE_COLORS["t"]
    ramp = SKIN if body else GRAY
    v = ramp.get(c, 200)
    return (v, v, v, 255)


def sheet_pixels(t, draw, tinted, body=False):
    cols = max(n for _, n, _ in CLIPS)
    rows = len(CLIPS) * len(DIRS)
    W, H = cols * t.w, rows * t.h
    img = [[(0, 0, 0, 0)] * W for _ in range(H)]
    fr = frames(t, draw)
    for ci, (clip, n, _) in enumerate(CLIPS):
        for di, d in enumerate(DIRS):
            for fi, bmp in enumerate(fr[(clip, d)]):
                ox, oy = fi * t.w, (ci * len(DIRS) + di) * t.h
                for y in range(t.h):
                    for x in range(t.w):
                        c = bmp.px[y][x]
                        if c is not None:
                            img[oy + y][ox + x] = colour(c, tinted, body)
    return img, fr


def write_png(path, img):
    h = len(img)
    w = len(img[0])
    raw = b"".join(b"\x00" + bytes(v for px in row for v in px) for row in img)

    def chunk(tag, data):
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")
    path.write_bytes(png)


def sheet_ron(t, image, tinted, layer=None):
    lines = [
        "// tools/paperdoll.py 가 만든 파일 — 직접 고치지 말고 생성기를 고친 뒤 다시 만든다.",
        "//",
        "// 배치: 행 = 클립 시작 행 + 방향, 열 = 프레임. 방향 0 은 +X(동)에서 반시계로 증가.",
        "(",
        "    version: 1,",
        f'    image: "{image}",',
        f"    cell: ({t.w}, {t.h}),",
        f"    directions: {len(DIRS)},",
        f"    pixels_per_meter: {float(t.ppm)},",
        f"    tinted: {'true' if tinted else 'false'},",
    ]
    if layer is not None:
        if len(set(layer)) == 1:
            lines.append(f"    layer: {layer[0]},")
        else:
            lines.append(f"    layer_by_direction: [{', '.join(map(str, layer))}],")
    lines.append("    clips: [")
    row = 0
    for clip, n, ms in CLIPS:
        lines.append(f"        (state: {clip}, row: {row}, frames: {n}, frame_ms: {ms}, looping: true),")
        row += len(DIRS)
    lines += ["    ],", ")", ""]
    return "\n".join(lines)


def ron_str(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def build():
    OUT.mkdir(parents=True, exist_ok=True)
    built = {}
    for t in TEMPLATES:
        folder = OUT / t.key
        folder.mkdir(exist_ok=True)
        img, _ = sheet_pixels(t, draw_body, tinted=True, body=True)
        write_png(folder / "body.png", img)
        (folder / "body.sheet.ron").write_text(sheet_ron(t, "body.png", True))
        parts = [p for p in PARTS if p.when(t)]
        for p in parts:
            img, _ = sheet_pixels(t, p.draw, p.tinted)
            write_png(folder / f"{p.key}.png", img)
            (folder / f"{p.key}.sheet.ron").write_text(sheet_ron(t, f"{p.key}.png", p.tinted, p.layer))
        built[t.key] = parts
        cat = [
            "// tools/paperdoll.py 가 만든 템플릿 카탈로그 — 이 체형에 맞는 파츠 목록.",
            "// sheet 경로는 이 파일 폴더 기준. slot 은 그 자리에 외형이 있는 장비를 입으면 빠지는 파츠.",
            "(",
            "    version: 1,",
            f"    key: {ron_str(t.key)},",
            f"    name: {ron_str(t.name)},",
            f"    reference: {ron_str(t.ref)},",
            '    body: "body.sheet.ron",',
            "    parts: [",
        ]
        for p in parts:
            slot = f"Some({p.slot})" if p.slot else "None"
            cat.append(
                f"        (key: {ron_str(p.key)}, name: {ron_str(p.name)}, category: {ron_str(p.category)}, "
                f"palette: {ron_str(p.palette)}, tinted: {'true' if p.tinted else 'false'}, slot: {slot}),"
            )
        cat += ["    ],", ")", ""]
        (folder / "catalog.ron").write_text("\n".join(cat))
    # 공용 카탈로그 — 템플릿 목록과 색 묶음
    lines = [
        "// tools/paperdoll.py 가 만든 공용 카탈로그 — 템플릿 목록과 커스터마이징 색 묶음 (sRGB).",
        "(",
        "    version: 1,",
        "    templates: [",
    ]
    for t in TEMPLATES:
        lines.append(f"        {ron_str(t.key + '/catalog.ron')},")
    # tools/sd_paperdoll.py 가 만든 SD2 템플릿 — 다시 만들어도 목록에서 빠지지 않게
    for extra in sorted(OUT.glob("sd2_*/catalog.ron")):
        lines.append(f"        {ron_str(extra.parent.name + '/catalog.ron')},")
    lines += ["    ],", "    palettes: {"]
    for name, cols in PALETTES.items():
        lines.append(f"        {ron_str(name)}: [")
        for label, (r, g, b) in cols:
            lines.append(f"            ({ron_str(label)}, ({r / 255:.3f}, {g / 255:.3f}, {b / 255:.3f})),")
        lines.append("        ],")
    lines += ["    },", ")", ""]
    (OUT / "catalog.ron").write_text("\n".join(lines))
    return built


# ─────────────────────────────────────────────────────────────────────────────
# 견본 그림 — 에디터와 같은 규칙으로 합성 (선형 공간에서 색 곱하기, 앞뒤 정렬)
# ─────────────────────────────────────────────────────────────────────────────


def to_lin(v):
    v /= 255
    return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4


def to_srgb(v):
    v = 12.92 * v if v <= 0.0031308 else 1.055 * v ** (1 / 2.4) - 0.055
    return max(0, min(255, round(v * 255)))


def tinted(px, tint):
    r, g, b, a = px
    return (to_srgb(to_lin(r) * to_lin(tint[0])), to_srgb(to_lin(g) * to_lin(tint[1])), to_srgb(to_lin(b) * to_lin(tint[2])), a)


def pal(name, label):
    return dict(PALETTES[name])[label]


# 견본 차림 — (파츠, 색 묶음 이름의 색) 목록. 템플릿에 없는 파츠는 건너뛴다.
LOOKS = [
    ("살구", [("eyes_round", "갈색"), ("face_smile", None), ("hair_short", "갈색"), ("beard_mustache", "갈색"), ("top_tunic", "파랑"),
             ("bottom_pants", "갈색"), ("shoes_boots", "갈색")]),
    ("밝은 살구", [("eyes_big", "파랑"), ("face_open", None), ("hairback_long", "금발"), ("hair_long", "금발"),
                 ("top_dress", "분홍"), ("shoes_shoes", "흰색"), ("hat_ribbon", "빨강")]),
    ("구릿빛", [("eyes_sharp", "초록"), ("face_plain", None), ("hair_spiky", "검정"), ("cape", "빨강"), ("eyepatch", "검정"),
              ("top_armor", "철"), ("bottom_pants", "남색"), ("shoes_boots", "철"), ("hat_headband", "빨강")]),
    ("창백", [("eyes_sleepy", "보라"), ("face_smile", None), ("hairback_twintail", "보라"), ("hair_twintail", "보라"),
            ("top_robe", "보라"), ("shoes_shoes", "검정"), ("hat_wizard", "보라")]),
    ("갈색", [("eyes_smile", "검정"), ("face_smile", None), ("hairback_ponytail", "빨강"), ("hair_ponytail", "빨강"),
            ("top_coat", "초록"), ("bottom_skirt", "베이지"), ("shoes_boots", "갈색")]),
    ("황갈", [("eyes_round", "금색"), ("face_open", None), ("hair_bun", "백금"), ("glasses", "검정"), ("freckles", None), ("top_shirt", "흰색"),
            ("bottom_shorts", "파랑"), ("shoes_shoes", "빨강"), ("hat_crown", "황금")]),
]
# 종족 특징 색 — 귀·꼬리는 머리색, 뿔은 뿔 색
FEATURE_TINT = {
    "ears_cat": "hair", "tail_cat": "hair", "horns": "horn", "tail_devil": "horn",
    "ears_elf": "skin", "ears_round": "skin",
}
RACE_SKIN = {"sd_orc": "초록 (오크)", "sd_demon": "푸른 (마족)"}


def compose(t, look, built, frame_key=("Idle", 0)):
    """견본 하나를 방향 넷으로 합성 — `[(방향, 비트맵 RGBA)]`."""
    skin_label, parts = look
    skin = pal("skin", RACE_SKIN.get(t.key, skin_label))
    have = {p.key: p for p in built[t.key]}
    hair_tint = None
    layers_list = [(0, -1, draw_body, True, True, skin)]
    for i, (key, colour_label) in enumerate(parts):
        p = have.get(key)
        if not p:
            continue
        tint = pal(p.palette, colour_label) if colour_label else None
        if p.category == "hair":
            hair_tint = tint
        layers_list.append((p.layer, i, p.draw, p.tinted, False, tint))
    for p in built[t.key]:
        if p.category == "feature":
            if p.key == "tusks":
                tint = None
            elif FEATURE_TINT[p.key] == "skin":
                tint = skin
            elif FEATURE_TINT[p.key] == "hair":
                tint = hair_tint or pal("hair", "갈색")
            else:
                tint = pal("horn", "흑요석")
            layers_list.append((p.layer, 99, p.draw, p.tinted, False, tint))
    clip, fi = frame_key
    out = []
    for di, d in enumerate(DIRS):
        canvas = [[(0, 0, 0, 0)] * t.w for _ in range(t.h)]
        ordered = sorted(layers_list, key=lambda l: ((l[0] if isinstance(l[0], int) else l[0][di]), l[1]))
        for _, _, draw, tnt, body, tint in ordered:
            bmp = Bitmap(t.w, t.h)
            draw(bmp, Geo(t, "E" if d == "W" else d, POSES[clip][fi]))
            if d == "W":
                bmp = bmp.flip()
            for y in range(t.h):
                for x in range(t.w):
                    c = bmp.px[y][x]
                    if c is None:
                        continue
                    px = colour(c, tnt, body)
                    if tnt and tint:
                        px = tinted(px, tint)
                    canvas[y][x] = px
        out.append((d, canvas))
    return out


def label_font(size):
    """견본 그림 글자 — PIL 이 있으면 한글 폰트로, 없으면 글자 없이."""
    try:
        from PIL import ImageFont
    except ImportError:
        return None
    for path in (
        "/usr/share/fonts/google-noto-sans-cjk-fonts/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "C:/Windows/Fonts/malgun.ttf",
    ):
        if Path(path).exists():
            return ImageFont.truetype(path, size)
    return None


def preview(built):
    """템플릿마다 한 줄 — 이름 + 견본 차림 여섯 × 남·동·북. 4배 확대."""
    PREVIEW.mkdir(exist_ok=True)
    scale = 4
    order = [3, 0, 1]  # 남·동·북
    label_w = 300
    max_w = max(t.w for t in TEMPLATES)
    cell_w = (max_w * len(order) + 4) * scale
    W = label_w + cell_w * len(LOOKS) + 8 * scale
    rows = [(t.h + 3) * scale for t in TEMPLATES]
    H = sum(rows) + 6 * scale
    bg = (46, 48, 56, 255)
    img = [[bg] * W for _ in range(H)]
    y0 = 3 * scale
    tops = []
    for t, rh in zip(TEMPLATES, rows):
        tops.append(y0)
        for li, look in enumerate(LOOKS):
            views = compose(t, look, built)
            x0 = label_w + li * cell_w
            for k, di in enumerate(order):
                _, can = views[di]
                ox = x0 + k * max_w * scale + (max_w - t.w) * scale // 2
                for y in range(t.h):
                    for x in range(t.w):
                        px = can[y][x]
                        if px[3] == 0:
                            continue
                        for sy in range(scale):
                            row = img[y0 + y * scale + sy]
                            for sx in range(scale):
                                row[ox + x * scale + sx] = px
        y0 += rh
    path = PREVIEW / "paperdoll_templates.png"
    write_png(path, img)
    font = label_font(22)
    small = label_font(15)
    if font:
        from PIL import Image, ImageDraw

        im = Image.open(path)
        dr = ImageDraw.Draw(im)
        for t, top, rh in zip(TEMPLATES, tops, rows):
            mid = top + rh // 2
            dr.text((16, mid - 34), t.name, font=font, fill=(235, 235, 240))
            dr.text((16, mid - 4), f"{t.w}×{t.h - t.pad}px · {t.key}", font=small, fill=(160, 165, 180))
            words, line, lines = t.ref.split(" "), "", []
            for wd in words:
                if small.getlength(line + " " + wd) > label_w - 30:
                    lines.append(line)
                    line = wd
                else:
                    line = (line + " " + wd).strip()
            lines.append(line)
            for i, ln in enumerate(lines[:3]):
                dr.text((16, mid + 18 + i * 18), ln, font=small, fill=(140, 145, 160))
        im.save(path)
    return path


def preview_parts(built, key="sd_human"):
    """한 템플릿의 파츠 전부를 낱장으로 — 남쪽 정지 프레임, 기본 몸 위에."""
    t = next(t for t in TEMPLATES if t.key == key)
    scale = 4
    parts = [p for p in built[key] if p.category not in ("feature",)]
    cols = 10
    rows = (len(parts) + cols - 1) // cols
    W = (t.w + 4) * scale * cols
    H = (t.h + 4) * scale * rows
    img = [[(46, 48, 56, 255)] * W for _ in range(H)]
    skin = pal("skin", "살구")
    for i, p in enumerate(parts):
        tint = PALETTES[p.palette][(i * 3) % len(PALETTES[p.palette])][1] if p.tinted else None
        look = [(p.layer, 1, p.draw, p.tinted, False, tint), (0, 0, draw_body, True, True, skin)]
        if p.category == "hair_back":
            look.append((L_HAIR, 2, hair_style("long"), True, False, tint))
        canvas = [[(0, 0, 0, 0)] * t.w for _ in range(t.h)]
        for _, _, draw, tnt, body, tn in sorted(look, key=lambda l: (l[0] if isinstance(l[0], int) else l[0][3], l[1])):
            bmp = Bitmap(t.w, t.h)
            draw(bmp, Geo(t, "S", POSES["Idle"][0]))
            for y in range(t.h):
                for x in range(t.w):
                    c = bmp.px[y][x]
                    if c is None:
                        continue
                    px = colour(c, tnt, body)
                    if tnt and tn:
                        px = tinted(px, tn)
                    canvas[y][x] = px
        ox = (i % cols) * (t.w + 4) * scale + 2 * scale
        oy = (i // cols) * (t.h + 4) * scale + 2 * scale
        for y in range(t.h):
            for x in range(t.w):
                px = canvas[y][x]
                if px[3]:
                    for sy in range(scale):
                        for sx in range(scale):
                            img[oy + y * scale + sy][ox + x * scale + sx] = px
    write_png(PREVIEW / f"paperdoll_parts_{key}.png", img)
    return PREVIEW / f"paperdoll_parts_{key}.png"


if __name__ == "__main__":
    built = build()
    total = sum(len(v) for v in built.values()) + len(built)
    print(f"템플릿 {len(built)}개, 시트 {total}장 → {OUT}")
    if "--preview" in sys.argv:
        print(preview(built))
        print(preview_parts(built))

