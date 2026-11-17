"""머리 모양 라이브러리 — 포니테일·트윈테일·올림머리·삐죽 머리. 칠하는 규칙은 tools/sd_head.py.

모양마다 세 방향(정면 s · 뒷면 n · 옆 e)의 `View` 를 만든다. 짧은 머리·긴 머리는 각각
tools/template_m_head.py · template_f_head.py 의 VIEWS 그대로다 (이 파일에는 새 모양만).

    import hair_styles
    views = hair_styles.views("ponytail", sex="f", end=46)

- 얼굴 모양은 성별 머리 스크립트의 `face_s`·`face_e` 를 빌린다 (앞머리 아래 얼굴 구멍).
- `end` 는 늘어지는 머리(꼬리) 끝 높이 — 체형의 `hair_end` (꼬마는 짧게).
- 정면·옆에서 `split`(31) 아래로 내려온 머리는 몸 뒤 겹(l_hair_behind_*)이 된다 → 종이 인형 `hairback_<모양>`.
- 묶은 자리(고무줄)·올림머리 테두리는 `mark` 로 짙은 머리색 — 머리 파츠 하나로 칠해지도록 색을 따로 쓰지 않는다.
"""
import math

import sd_head
import template_f_head as HF
import template_m_head as HM
from sd_head import View

STYLES = {"ponytail": "포니테일", "twintail": "트윈테일", "bun": "올림머리", "spiky": "삐죽 머리"}

# 다발 경계·정수리 — 긴 머리(여)와 같은 틀
_S = dict(cx=28.0, cy=20.0, rx=15.5, ry=14.8,
          sep_top=[17.5, 21.5, 25.5, 29.5, 33.5, 37.5], sep_bot=[10.0, 17.5, 24.0, 31.0, 37.5, 45.0])
_N = dict(cx=28.0, cy=20.0, rx=15.5, ry=14.8,
          sep_top=[16.5, 20.5, 24.5, 28.0, 31.5, 35.5, 39.5], sep_bot=[9.0, 15.0, 21.0, 28.0, 35.0, 41.0, 47.0])
_E = dict(cx=26.5, cy=20.0, rx=14.5, ry=14.8,
          sep_top=[17.0, 21.0, 25.0, 29.0, 33.0, 37.0], sep_bot=[6.0, 12.0, 18.0, 25.0, 33.0, 41.0])


def _px():
    """출력 1px 의 설계 좌표 길이 — 테두리 선을 1px 로 남긴다."""
    return 1.0 / (sd_head.SCALE * sd_head.RES)


def _view(name, geo, tips, face, side, mark=None):
    return View(name, geo["cx"], geo["cy"], geo["rx"], geo["ry"], geo["sep_top"], geo["sep_bot"], tips, face, side,
                mark=mark)


# ── 공통 조각 ──

def short_side_s(v, x, y, bottom=29):
    """정면 귀밑 옆머리 — 아래로 좁아진다."""
    if not 18 <= y <= bottom:
        return False
    k = max(0.0, (y - (bottom - 5)) / 5)
    return 12.5 + k <= x + 0.5 <= 17.0 or 39.0 <= x + 0.5 <= 43.5 - k


def nape_n(v, x, y, base=28):
    """뒷면 목덜미 머리 — 다발마다 끝이 뾰족."""
    if not 18 <= y <= base + 4 or not 13.5 <= x + 0.5 <= 42.5:
        return False
    i, u = v.clump(x, min(y, 26))
    return y <= base + 3 * (1 - abs(u - 0.5) * 2)


def nape_e(v, x, y, bottom=30):
    """옆 뒤통수에서 목덜미로."""
    if not 18 <= y <= bottom:
        return False
    return 13.0 + max(0, y - 24) * 0.6 <= x + 0.5 <= 26.5 - max(0, y - 26) * 1.2


def tail_shape(y, cx, top, bottom, swell=3.4, curl=2.0):
    """늘어진 머리 한 갈래의 (가운데, 반폭) — 묶은 자리는 가늘고, 부풀었다가 끝이 뾰족. 끝은 바깥으로 휜다."""
    t = (y - top) / max(1, bottom - top)
    half = 1.3 + swell * math.sin(math.pi * min(1.0, t * 1.15))
    return cx + (curl * t * t if cx > 28 else -curl * t * t), half


def tail(x, y, cx, top, bottom, swell=3.4, curl=2.0):
    if not top <= y <= bottom:
        return False
    c, half = tail_shape(y, cx, top, bottom, swell, curl)
    return abs(x + 0.5 - c) <= half


def edge_of(x, c, half):
    """갈래 가장자리 안쪽 1px 인가 — 정수리 위에 겹친 갈래를 짙은 선으로 갈라 보이게."""
    d = abs(x + 0.5 - c)
    return half - _px() < d <= half


def in_crown(geo, x, y):
    dx, dy = (x + 0.5 - geo["cx"]) / geo["rx"], (y + 0.5 - geo["cy"]) / geo["ry"]
    return dx * dx + dy * dy <= 1.0


def tie(x, y, cx, cy, half=2.6):
    """묶은 자리 — 짙은 한 줄 (고무줄 그늘)."""
    return abs(y + 0.5 - cy) <= _px() * 0.9 and abs(x + 0.5 - cx) <= half


def ring(x, y, cx, cy, r):
    """원 테두리 1px — 올림머리 경계."""
    return abs(math.hypot(x + 0.5 - cx, y + 0.5 - cy) - r) <= _px() * 0.55


def disc(x, y, cx, cy, r):
    return math.hypot(x + 0.5 - cx, y + 0.5 - cy) <= r


# ── 모양 ──

def ponytail(face_s, face_e, end):
    """뒤로 묶은 한 갈래 — 정면은 짧은 옆머리, 뒷면·옆은 꼬리. 묶은 자리는 귀 높이."""
    n_top, e_top = 20, 19

    def e_shape(y):
        t = (y - e_top) / max(1, end - 1 - e_top)
        c = 12.0 - 3.4 * t + 2.0 * t * t  # 뒤로 뻗었다가 아래로 떨어진다
        return c, 1.4 + 2.8 * math.sin(math.pi * min(1.0, t * 1.15))

    def side_s(v, x, y):
        return short_side_s(v, x, y, bottom=30)

    def back_n(v, x, y):
        return nape_n(v, x, y, base=26) or tail(x, y, 28.0, n_top, end, swell=3.0, curl=0)

    def side_e(v, x, y):
        if nape_e(v, x, y, bottom=27):
            return True
        if not e_top <= y <= end - 1:
            return False
        c, half = e_shape(y)
        return abs(x + 0.5 - c) <= half

    def mark_n(x, y):
        if tie(x, y, 28.0, n_top + 1.5, 2.0):
            return "X"
        if n_top + 2 <= y <= end and in_crown(_N, x, y):
            c, half = tail_shape(y, 28.0, n_top, end, 3.0, 0)
            if edge_of(x, c, half):
                return "X"
        return None

    def mark_e(x, y):
        if not e_top <= y <= end - 1:
            return None
        c, half = e_shape(y)
        if y + 0.5 - (e_top + 1.5) <= _px() * 0.9 and y + 0.5 >= e_top + 1.5 - _px() * 0.9 and abs(x + 0.5 - c) <= half:
            return "X"  # 묶은 자리
        if y > e_top + 2 and in_crown(_E, x, y) and edge_of(x, c, half) and x + 0.5 > c:
            return "X"  # 뒤통수 위로 겹친 꼬리의 앞 가장자리
        return None

    return [
        _view("s", _S, [22, 21, 24, 21, 22], face_s, side_s),
        _view("n", _N, None, None, back_n, mark=mark_n),
        _view("e", _E, [21, 22, 20, 22, 21], face_e, side_e, mark=mark_e),
    ]


def twintail(face_s, face_e, end):
    """양옆으로 묶은 두 갈래 — 정면·뒷면 모두 보인다."""
    top = 20

    def tails(x, y):
        return tail(x, y, 9.5, top, end) or tail(x, y, 46.5, top, end)

    def side_s(v, x, y):
        return short_side_s(v, x, y, bottom=30) or tails(x, y)

    def back_n(v, x, y):
        return nape_n(v, x, y, base=29) or tails(x, y)

    def side_e(v, x, y):
        return nape_e(v, x, y, bottom=30) or tail(x, y, 12.5, top + 1, end - 1)

    def ties(x, y):
        return "X" if tie(x, y, 9.5, top + 1.5, 2.2) or tie(x, y, 46.5, top + 1.5, 2.2) else None

    return [
        _view("s", _S, [22, 21, 24, 21, 22], face_s, side_s, mark=ties),
        _view("n", _N, None, None, back_n, mark=ties),
        _view("e", _E, [21, 22, 20, 22, 21], face_e, side_e,
              mark=lambda x, y: "X" if tie(x, y, 12.5, top + 2.5, 2.2) else None),
    ]


def bun(face_s, face_e, end):
    """정수리 뒤로 틀어 올린 머리 — 목덜미가 드러나고 둥근 쪽이 위로 솟는다."""
    buns = {"s": (28.0, 5.0, 5.2), "n": (28.0, 8.0, 6.4), "e": (16.5, 7.5, 5.8)}

    def side_s(v, x, y):
        return short_side_s(v, x, y, bottom=27) or disc(x, y, *buns["s"])

    def back_n(v, x, y):
        return nape_n(v, x, y, base=24) or disc(x, y, *buns["n"])

    def side_e(v, x, y):
        return nape_e(v, x, y, bottom=25) or disc(x, y, *buns["e"])

    def edge(view):
        cx, cy, r = buns[view]

        geo = {"s": _S, "n": _N, "e": _E}[view]

        def mark(x, y):
            if not disc(x, y, cx, cy, r):
                return None
            if ring(x, y, cx, cy, r):
                # 정면은 정수리 밖으로 솟은 위쪽 테두리만, 뒷면·옆은 둘레 전부 (아래 반은 그늘색)
                if view == "s" and in_crown(geo, x, y):
                    return None
                return "x" if y + 0.5 < cy else "X"
            if view == "s" and in_crown(geo, x, y):
                return None
            # 둥근 쪽 명암 — 왼쪽 위가 밝고 오른쪽 아래로 그늘 (정수리 하이라이트 규칙 대신)
            lit = ((x + 0.5 - cx) + (y + 0.5 - cy)) / r
            if math.hypot(x + 0.5 - cx + r * 0.35, y + 0.5 - cy + r * 0.4) < r * 0.28:
                return "W"
            return "w" if lit < -0.55 else ("v" if lit < 0.15 else ("u" if lit < 0.75 else "x"))
        return mark

    return [
        _view("s", _S, [22, 21, 24, 21, 22], face_s, side_s, mark=edge("s")),
        _view("n", _N, None, None, back_n, mark=edge("n")),
        _view("e", _E, [21, 22, 20, 22, 21], face_e, side_e, mark=edge("e")),
    ]


def spiky(face_s, face_e, end):
    """정수리 둘레로 뻗친 삐죽 머리 — 앞머리 끝도 들쭉날쭉."""

    def spikes(v, x, y, a0, a1, n, length):
        dx, dy = (x + 0.5 - v.cx) / v.rx, (y + 0.5 - v.cy) / v.ry
        a = math.degrees(math.atan2(dy, dx))
        if not a0 <= a <= a1:
            return False
        phase = (a - a0) / (a1 - a0) * n
        tri = 1 - abs((phase % 1) * 2 - 1)
        return math.hypot(dx, dy) <= 1 + length * tri ** 1.6

    def side_s(v, x, y):
        return short_side_s(v, x, y, bottom=28) or spikes(v, x, y, -175, -5, 7, 0.24)

    def back_n(v, x, y):
        if spikes(v, x, y, -175, -5, 7, 0.24):
            return True
        if not 18 <= y <= 33 or not 13.5 <= x + 0.5 <= 42.5:
            return False
        i, u = v.clump(x, min(y, 26))
        return y <= 26 + 6 * (1 - abs(u - 0.5) * 2)  # 목덜미 끝도 다발마다 길게 뾰족

    def side_e(v, x, y):
        return nape_e(v, x, y, bottom=29) or spikes(v, x, y, -180, -40, 6, 0.26)

    return [
        _view("s", _S, [24, 18, 25, 18, 24], face_s, side_s),
        _view("n", _N, None, None, back_n),
        _view("e", _E, [23, 19, 23, 19, 22], face_e, side_e),
    ]


BUILD = {"ponytail": ponytail, "twintail": twintail, "bun": bun, "spiky": spiky}


def views(style, sex="f", end=46):
    """머리 모양 `style` 의 세 방향 View. 얼굴은 `sex` 의 머리 스크립트에서."""
    mod = HF if sex == "f" else HM
    return BUILD[style](mod.face_s, mod.face_e, end)
