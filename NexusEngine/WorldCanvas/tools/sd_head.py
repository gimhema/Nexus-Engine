"""SD 캐릭터 머리 화가 — 규칙으로 칠해 `.canvas` 의 grid 프레임으로 내보내는 공용 엔진.

캐릭터마다 작은 스크립트(`tools/<캐릭터>_head.py`)가 방향별 모양(정수리 타원, 다발 경계,
앞머리 끝, 옆·뒷머리, 얼굴)을 정하고 이 엔진의 `main()` 을 부른다:

    python3 tools/dark_knight_v2_head.py            # 표준 출력으로
    python3 tools/dark_knight_v2_head.py --write    # 원본의 「머리」 절을 바꾼다
    … --scale=0.62 --res=2                          # 머리 배율, 출력 해상도(112×112)

방향마다 두 장:
  l_hair_behind_<d>  어깨 아래로 내려오는 옆·뒷머리 — 몸 뒤에 깐다
  l_head_<d>         정수리·앞머리·얼굴·어깨 위 머리 — 위에 얹는다
(뒷모습 n 은 머리가 몸을 덮으므로 l_head_n 한 장)

팔레트 문자는 캐릭터와 상관없이 같다 — 색은 원본의 팔레트가 정한다:
  머리 WwvuxX (밝음→어두움) · 피부 sSTt · 눈 K(속눈썹) QqrR(홍채 어두움→밝음) Y(하이라이트) · 볼 b · 외곽선 o

칠하는 규칙:
- 다발 경계: 정수리에서 부채꼴로 퍼지며 바깥으로 살짝 휜 선. 고리 바로 아래는 흐리게(x), 아래로 짙게(X)
- 다발 안: 왼쪽 밝게(w), 오른쪽 그늘(u). 왼쪽 가장자리에 고리 아래 몇 칸 하이라이트 결(W)
- 전체 빛: 왼쪽 위. 오른쪽 끝·아래로 갈수록 한 단계씩 어둡게
- 천사 고리: 정수리 아래 호, 왼쪽 W·오른쪽 w
- 앞머리 끝: 다발 가운데서 길고 경계에서 짧은 지그재그. 그 아래 피부는 그림자(T)
- 외곽선은 앞뒤 두 장을 합친 그림에 한 번 — 자른 경계에 선이 생기지 않게
"""
import math
import sys

W_, H_ = 56, 56
HAIR = "WwvuxX"  # 0 밝음 .. 5 가장 어두움

# 머리 배율 — 모양은 56×56 「설계 좌표」로 정하고, 출력 픽셀을 목(ANCHOR) 기준으로 설계 좌표로 되돌려
# 계산한다. 1 이면 출력 = 설계. 작게 하면 머리가 목 쪽으로 오그라든다 (몸에 붙는 자리는 그대로).
# 1px 이어야 하는 선(다발 경계·고리)은 판정 폭을 배율로 나눠 출력에서 1px 로 남긴다.
# 눈·아호게 같은 픽셀 그림은 줄이지 않고 자리만 옮긴다 — 작은 머리에는 작은 눈 그림을 따로 준다.
SCALE = 1.0
ANCHOR = (28.0, 31.0)
# 출력 해상도 — 2 면 같은 머리를 112×112 격자에 칠한다 (시트 축척 32px/m). 모양은 그대로 56 설계 좌표로 정하고,
# 1px 선은 출력에서 1px 로 남으니 더 촘촘해진다. 눈·입은 픽셀 그림이라 고해상도용(`hires`)을 따로 준다.
RES = 1
# 다발 가운데 가는 결 — 다발 하나를 두세 가닥으로 쪼갠다 (디테일 밀도). main(strands=True) 로 켠다.
STRANDS = False


def to_design(x, y):
    """출력 픽셀 → 설계 좌표 (배율·해상도 1 이면 그대로)."""
    ax, ay = ANCHOR
    k = SCALE * RES
    return ax + (x + 0.5 - ax * RES) / k - 0.5, ay + (y + 0.5 - ay * RES) / k - 0.5


def to_out(x, y):
    """설계 좌표 → 출력 픽셀."""
    ax, ay = ANCHOR
    k = SCALE * RES
    return round(ax * RES + (x + 0.5 - ax) * k - 0.5), round(ay * RES + (y + 0.5 - ay) * k - 0.5)

EYES_S = (
    (["KKKKKK.", "KQQqqKK", "KQYYqr.", "KqYqrR.", ".qrrRR.", "..RRR.."], 18, 22),
    (["KKKKK", "KQqqK", "QYYqr", "qYqrR", "qrrRR", ".RRR."], 30, 22),
)
EYE_E = (["KKKKKK", ".KQqqK", ".QYYqr", ".qYqrR", ".qrRRR", "..RRK."], 30, 22)


# 볼 홍조·입 자리 — 종이 인형은 이것만 따로 뽑아 「입」 파츠로 쓴다
BLUSH_S = [(18, 28), (19, 28), (31, 28), (32, 28)]
BLUSH_E = [(33, 28), (34, 28)]
MOUTH_S = [((26, 30), "t"), ((27, 30), "t"), ((26, 31), "T")]
MOUTH_E = [((37, 31), "t")]


class View:
    """한 방향의 머리 모양.

    side(view, x, y) — 정수리 밖으로 내려오는 옆·뒷머리인가
    face(x, y)       — 얼굴인가 (뒷모습은 None)
    tips             — 다발마다 앞머리 끝 높이 (None = 앞머리 없음)
    mark(x, y)       — 그 자리 머리색을 바꿀 문자 (묶은 자리 그늘·올림머리 경계), 없으면 None
    """

    def __init__(self, name, cx, cy, rx, ry, sep_top, sep_bot, tips, face, side, split=31, mark=None):
        self.name = name
        self.cx, self.cy, self.rx, self.ry = cx, cy, rx, ry
        self.sep_top, self.sep_bot = sep_top, sep_bot
        self.tips = tips
        self.face = face
        self.side = side
        self.split = split
        self.mark = mark

    # 다발 경계 — 정수리(y=8)에서 아래(y=26)로, 바깥으로 살짝 휜다
    def sep(self, i, y):
        t = (y - 8) / 18
        x = self.sep_top[i] + (self.sep_bot[i] - self.sep_top[i]) * t
        bulge = (self.sep_bot[i] - self.cx) * 0.18 * math.sin(math.pi * max(0.0, min(1.0, t)))
        return x + bulge

    def clump(self, x, y):
        xs = [self.sep(i, y) for i in range(len(self.sep_top))]
        for i in range(len(xs) - 1):
            if xs[i] <= x + 0.5 < xs[i + 1]:
                return i, (x + 0.5 - xs[i]) / (xs[i + 1] - xs[i])
        if x + 0.5 < xs[0]:
            return -1, (x + 0.5 - (xs[0] - 4)) / 4
        return len(xs) - 1, (x + 0.5 - xs[-1]) / 4

    def near_sep(self, x, y):
        half = 0.5 / (SCALE * RES)
        return any(abs(x + 0.5 - self.sep(i, y)) < half for i in range(len(self.sep_top)))

    def tip_line(self, y_ref=18):
        xs = [self.sep(i, y_ref) for i in range(len(self.sep_top))]
        pts = [(xs[0] - 3, 25)]
        for i in range(len(xs) - 1):
            pts.append((xs[i], 18 if i else 19))
            pts.append(((xs[i] + xs[i + 1]) / 2, self.tips[i]))
        pts.append((xs[-1], 17))
        pts.append((xs[-1] + 3, 25))
        return pts

    def in_crown(self, x, y):
        dx, dy = (x + 0.5 - self.cx) / self.rx, (y + 0.5 - self.cy) / self.ry
        return dx * dx + dy * dy <= 1.0


def interp(pts, x):
    for (x0, y0), (x1, y1) in zip(pts, pts[1:]):
        if x0 <= x <= x1:
            t = 0 if x1 == x0 else (x - x0) / (x1 - x0)
            return y0 + (y1 - y0) * t
    return pts[0][1] if x < pts[0][0] else pts[-1][1]


def nose_e(x, y):
    """옆얼굴 윤곽 — 코끝이 한 칸 튀어나온다 (설계 좌표)."""
    return (round(x), round(y)) in ((39, 26), (39, 27))


def hair_shade(v, x, y, tips):
    i, u = v.clump(x, y)
    idx = 2
    if u < 0.22:
        idx -= 1
    elif u > 0.72:
        idx += 1
    ring0 = 11 + ((x + 0.5 - v.cx) / 14) ** 2 * 3
    if STRANDS and 0 <= i < len(v.sep_top) - 1 and y > ring0 + 3:
        # 다발 폭에서 출력 1px 만큼의 가는 결 — 가운데보다 조금 오른쪽(그늘 쪽), 결 왼쪽은 한 단계 밝게
        width = (v.sep(i + 1, y) - v.sep(i, y)) * SCALE * RES
        if width >= 4:
            off = (u - 0.58) * width
            if abs(off) < 0.5:
                idx += 1
            elif -1.5 < off < -0.5:
                idx -= 1
    if 0.08 < u < 0.3 and ring0 + 1.5 < y < ring0 + 6 and x < v.cx + 6:
        idx = 0
    if x >= v.cx + 9:
        idx += 1
    if x <= v.cx - 13 and y < 30:
        idx -= 1
    if tips and y >= interp(tips, x) - 2.5 and v.in_crown(x, y):
        idx += 1
    if y >= 31:
        idx += 1
    if y >= 39:
        idx += 1
    if y <= 9:
        idx = 1 if x < v.cx + 3 else (2 if x < v.cx + 7 else 3)
        if y <= 7 and x < v.cx - 1:
            idx = 0
    ring = 11 + ((x + 0.5 - v.cx) / 14) ** 2 * 3
    tol, row = 0.6 / (SCALE * RES), 1 / (SCALE * RES)
    if abs(y - ring) < tol and v.cx - 14 <= x <= v.cx + 13:
        idx = 0 if x < v.cx + 3 else 1
    elif abs(y - (ring + row)) < tol and v.cx - 13 <= x <= v.cx + 5:
        idx = 1
    return HAIR[max(0, min(5, idx))]


def paint(v, ahoge_on=True, eyes_s=EYES_S, eye_e=EYE_E, blush=True, mouth=True):
    g = [["." for _ in range(W_)] for _ in range(H_)]
    behind = [["." for _ in range(W_)] for _ in range(H_)]
    tips = v.tip_line() if v.tips else None
    for oy in range(H_):
        for ox in range(W_):
            x, y = to_design(ox, oy)
            crown = v.in_crown(x, y)
            sd = v.side(v, x, y)
            face = v.face(x, y) if v.face else False
            if v.name == "e" and nose_e(x, y):
                face = True
            if not (crown or sd or face):
                continue
            under = tips is not None and y >= interp(tips, x)
            if face and under:
                g[oy][ox] = "S"
                continue
            # 옆모습: 앞머리 끝 아래, 얼굴 앞쪽에는 머리가 없다 (정수리 타원이 얼굴을 덮지 않게)
            if v.name == "e" and under and x >= 30 and not face:
                continue
            c = hair_shade(v, x, y, tips)
            # 다발 경계: 고리 바로 아래는 흐리게(x) 시작해 아래로 짙어진다(X) — 줄무늬처럼 안 보이게
            ring = 11 + ((x + 0.5 - v.cx) / 14) ** 2 * 3
            if y >= 10 and v.near_sep(x, y):
                c = "X" if y >= ring + 4 else ("x" if y >= ring + 1.5 else c)
            if v.mark:
                c = v.mark(x, y) or c
            if v.name != "n" and y >= v.split and not crown:
                behind[oy][ox] = c
            else:
                g[oy][ox] = c
    if v.name == "s":
        paint_face_s(g, eyes_s, blush, mouth)
    elif v.name == "e":
        paint_face_e(g, eye_e, blush, mouth)
    if ahoge_on:
        ahoge(g, v)
    both = [[g[y][x] if g[y][x] != "." else behind[y][x] for x in range(W_)] for y in range(H_)]
    outline(both)
    for y in range(H_):
        for x in range(W_):
            if both[y][x] == "o" and g[y][x] == "." and behind[y][x] == ".":
                (behind if (to_design(x, y)[1] >= v.split and v.name != "n") else g)[y][x] = "o"
    return behind, g


def shadow_under_hair(g):
    for y in range(1, H_):
        for x in range(W_):
            if g[y][x] in "sST" and g[y - 1][x] in HAIR:
                g[y][x] = "T" if g[y][x] != "T" else "t"
    for y in range(H_ - 1):
        for x in range(W_):
            if g[y][x] in "Wwvu" and g[y + 1][x] in "sSTt":
                g[y][x] = "x"


def paint_face_s(g, eyes, blush=True, mouth=True):
    for oy in range(H_):
        for ox in range(W_):
            if g[oy][ox] != "S":
                continue
            x, y = to_design(ox, oy)
            if x >= 33:
                g[oy][ox] = "T"
            if x >= 35:
                g[oy][ox] = "t"
            if x <= 19 and 25 <= y <= 28:
                g[oy][ox] = "s"
            if STRANDS and y >= 31.5:
                g[oy][ox] = "T"  # 턱 아래 그늘 — 얼굴의 큰 피부 면을 가른다
    shadow_under_hair(g)
    for rows, x0, y0 in eyes:
        stamp(g, rows, *to_out(x0, y0))
    face_marks(g, BLUSH_S if blush else [], MOUTH_S if mouth else [])


def face_marks(g, blush, mouth):
    """볼 홍조·입 — 설계 좌표 목록을 출력 자리에 찍는다 (같은 자리에 겹치면 한 번).

    고해상도(RES ≥ 2)에서는 점 하나가 너무 작다 — 입은 같은 줄의 점들을 이은 가로선에 아래 그늘 한 점,
    볼 홍조는 점마다 가로 두 칸.
    """
    for x, y in blush:
        ox, oy = to_out(x, y)
        for dx in range(2 if RES >= 2 else 1):
            g[oy][ox + dx] = "b"
    if RES >= 2 and mouth:
        line = [to_out(x, y) for (x, y), c in mouth if c == "t"]
        ys = {oy for _, oy in line}
        for oy in ys:
            xs = [ox for ox, y in line if y == oy]
            lo, hi = min(xs), max(xs) + 1
            for ox in range(lo, hi + 1):
                g[oy][ox] = "t"
            g[oy + 1][(lo + hi) // 2] = "T"
        return
    for (x, y), c in mouth:
        ox, oy = to_out(x, y)
        g[oy][ox] = c


def paint_face_e(g, eye, blush=True, mouth=True):
    for oy in range(H_):
        for ox in range(W_):
            if g[oy][ox] != "S":
                continue
            x, y = to_design(ox, oy)
            if x <= 29 and y >= 27:
                g[oy][ox] = "T"  # 턱 아래·뒤 그늘
            if x >= 37 and y <= 27:
                g[oy][ox] = "s"
    shadow_under_hair(g)
    # 옆모습 눈 — 좁고, 속눈썹 꼬리가 뒤(왼쪽)로
    if eye:
        rows, x0, y0 = eye
        stamp(g, rows, *to_out(x0, y0))
    face_marks(g, BLUSH_E if blush else [], MOUTH_E if mouth else [])


def blank():
    return [["." for _ in range(W_)] for _ in range(H_)]


def hair_only(g):
    """머리카락과 그 외곽선만 — 얼굴·눈·입을 뺀다 (종이 인형의 머리 파츠)."""
    out = blank()
    for y in range(H_):
        for x in range(W_):
            c = g[y][x]
            if c in HAIR:
                out[y][x] = c
            elif c == "o":
                near = [g[y + dy][x + dx] for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1))
                        if 0 <= x + dx < W_ and 0 <= y + dy < H_]
                if any(n in HAIR for n in near):
                    out[y][x] = "o"
    return out


def head_skin(v):
    """두피와 얼굴 피부 (외곽선 없음) — 어떤 머리 파츠를 얹어도 구멍이 나지 않게 정수리 전체를 피부로.

    빛은 왼쪽 위 — 정면은 오른쪽 볼, 옆면은 턱 뒤가 그늘.
    """
    out = blank()
    for oy in range(H_):
        for ox in range(W_):
            x, y = to_design(ox, oy)
            face = v.face(x, y) if v.face else False
            if v.name == "e" and nose_e(x, y):
                face = True
            if not (v.in_crown(x, y) or face):
                continue
            c = "S"
            if v.name == "s" and x >= 33:
                c = "t" if x >= 35 else "T"
            if v.name == "e" and x <= 29 and y >= 27:
                c = "T"
            if v.name == "e" and x >= 37 and y <= 27:
                c = "s"
            out[oy][ox] = c
    return out


def stamp(g, rows, x0, y0):
    for dy, r in enumerate(rows):
        for dx, c in enumerate(r):
            if c != ".":
                g[y0 + dy][x0 + dx] = c


def ahoge(g, v):
    rows = ["..ooo", ".oWWw", ".oWoo", "oWo..", "oWo.."]
    x0, y0 = to_out(int(v.cx) - 2, 1)
    for dy, r in enumerate(rows):
        for dx, c in enumerate(r):
            if c != "." and g[y0 + dy][x0 + dx] == ".":
                g[y0 + dy][x0 + dx] = c
    g[y0 + 4][x0 + 1] = "W"


def outline(g):
    src = [r[:] for r in g]
    for y in range(H_):
        for x in range(W_):
            if src[y][x] != ".":
                continue
            for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                xx, yy = x + dx, y + dy
                if 0 <= xx < W_ and 0 <= yy < H_ and src[yy][xx] not in ".o":
                    g[y][x] = "o"
                    break


def emit(name, g, out, part=False):
    # 칸이 56×56 이 아닌 원본(장신·거구)은 부품으로 내보내고 머리 높이만큼 내려 stamp 한다
    out.append(f"part {name} {W_} {H_}" if part else f"frame {name}")
    out.append("    grid")
    for r in g:
        out.append("        " + "".join(r))
    out.append("    end")
    out.append("end")


# ── 고해상도 눈 — 머리 배율(SCALE × RES)에 맞춰 크기를 정해 그린다. 픽셀 그림을 늘리면 뭉개진다 ──
# (설계 너비, 설계 높이, 왼눈 설계 자리, 오른눈 설계 자리, 옆눈 설계 자리)
EYE_DESIGN = {"f": (7, 6, (18, 22), (30, 22), (30, 22)), "m": (7, 4.2, (18, 23), (30, 23), (31, 23))}


def make_eye(w, h, style, outer):
    """눈 한 짝 — w×h. outer = 눈꼬리 쪽(-1 왼쪽, 1 오른쪽). 하이라이트는 늘 왼쪽 위(빛 방향)."""
    g = [["." for _ in range(w)] for _ in range(h)]
    lash = max(1, round(h * (0.2 if style == "f" else 0.28)))
    body_h = h - lash
    cx, cy = (w - 1) / 2, lash + (body_h - 1) / 2
    ix, iw = cx - outer * w * 0.06, w * (0.62 if style == "f" else 0.5)
    for y in range(lash, h):
        for x in range(w):
            ex, ey = (x - cx) / (w / 2), (y - cy) / (body_h / 2 + 0.4)
            if ex * ex + ey * ey > 1.05:
                continue
            if abs(x - ix) <= iw / 2:
                t = (y - lash) / max(1, body_h - 1)  # 홍채 — 위가 짙고 아래가 밝다
                g[y][x] = "Q" if t < 0.3 else ("q" if t < 0.55 else ("r" if t < 0.8 else "R"))
            else:
                g[y][x] = "Y"  # 흰자
    # 동공
    for y in range(lash, lash + max(1, round(body_h * 0.55))):
        for x in range(round(ix - iw * 0.15), round(ix + iw * 0.15) + 1):
            if 0 <= x < w and g[y][x] != ".":
                g[y][x] = "Q"
    # 윗속눈썹 — 눈꼬리 쪽으로 한 칸 길다(여성형)
    for y in range(lash):
        for x in range(w):
            g[y][x] = "K"
    if style == "f" and h >= 6:
        tail = w - 1 if outer > 0 else 0
        g[lash][tail] = "K"
    # 눈 양옆 테두리
    for y in range(lash, h - 1):
        row = [x for x in range(w) if g[y][x] != "."]
        if row:
            g[y][row[0]] = g[y][row[0]] if g[y][row[0]] in "QqrR" else "K"
            g[y][row[-1]] = g[y][row[-1]] if g[y][row[-1]] in "QqrR" else "K"
    # 하이라이트 — 홍채 왼쪽 위에 크게, 오른쪽 아래에 작게
    hs = max(1, round(w * 0.16))
    hx, hy = round(ix - iw * 0.32), lash + max(0, round(body_h * 0.12))
    for y in range(hy, min(h, hy + hs)):
        for x in range(hx, min(w, hx + hs)):
            if g[y][x] in "QqrR":
                g[y][x] = "Y"
    lx, ly = round(ix + iw * 0.25), lash + round(body_h * 0.65)
    if 0 <= lx < w and ly < h and g[ly][lx] in "QqrR":
        g[ly][lx] = "Y"
    # 아래 속눈썹 — 가운데 짧게 (여성형)
    if style == "f":
        for x in range(round(cx - w * 0.15), round(cx + w * 0.15) + 1):
            if g[h - 1][x] == ".":
                g[h - 1][x] = "K"
    return ["".join(r) for r in g]


def scaled_eyes(style):
    """고해상도 눈 — (정면 두 눈, 옆눈). 크기는 설계 크기 × 머리 배율 × 해상도."""
    dw, dh, left, right, side = EYE_DESIGN[style]
    k = SCALE * RES
    w, h = max(5, round(dw * k)), max(4, round(dh * k))
    front = ((make_eye(w, h, style, -1), *left), (make_eye(w, h, style, 1), *right))
    sw = max(4, round(w * 0.8))
    return front, (make_eye(sw, h, style, -1), *side)


def main(tool, canvas, views, small=None, hires=None, strands=False, **paint_opts):
    """방향마다 칠해 「머리」 절을 만든다. `--write` 면 `canvas` 의 그 절을 바꾼다.

    `--canvas=경로` 로 다른 원본에 쓰고(같은 머리를 여러 체형이 쓴다), `--part` 면 frame 대신
    56×56 part 로 내보낸다.
    """
    global SCALE, STRANDS, RES, W_, H_
    STRANDS = strands
    for a in sys.argv:
        if a.startswith("--canvas="):
            canvas = a.split("=", 1)[1]
        if a.startswith("--scale="):
            SCALE = float(a.split("=", 1)[1])
        if a.startswith("--res="):
            RES = int(a.split("=", 1)[1])
            W_ = H_ = 56 * RES
    # 고해상도에는 고해상도 눈, 작은 머리에는 작은 눈 — 픽셀 그림은 늘리거나 줄이지 않는다
    if RES >= 2 and hires:
        # 문자열이면 배율에 맞춰 그리는 눈("f" 둥근 큰 눈 · "m" 가는 눈), 사전이면 그 그림 그대로
        if isinstance(hires, str):
            front, side = scaled_eyes(hires)
            paint_opts.update(eyes_s=front, eye_e=side)
        else:
            paint_opts.update(hires)
    elif SCALE < 0.9 and small:
        paint_opts.update(small)
    part = "--part" in sys.argv
    head = f"// ── 머리 — {tool} 로 생성 (규칙은 tools/sd_head.py 머리말) ──"
    end = "// ── 머리 끝 ──"
    out = [head]
    for v in views:
        behind, g = paint(v, **paint_opts)
        if v.name != "n":
            emit(f"l_hair_behind_{v.name}", behind, out, part)
        emit(f"l_head_{v.name}", g, out, part)
    out.append(end)
    text = "\n".join(out) + "\n"
    if "--write" in sys.argv:
        src = open(canvas).read()
        a = src.index(head)
        b = src.index(end + "\n") + len(end) + 1
        open(canvas, "w").write(src[:a] + text + src[b:])
    else:
        sys.stdout.write(text)
