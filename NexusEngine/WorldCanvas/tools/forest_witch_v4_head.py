"""숲의 마녀 v4 머리 — 청록 양갈래, 모자를 쓰므로 아호게 없음. 칠하는 규칙은 tools/sd_head.py.

    python3 tools/forest_witch_v4_head.py --write

정수리 위쪽은 모자(원본의 l_hat_*)가 덮는다. 머리는 얼굴 옆 짧은 옆머리 + 그 바깥으로 길게
늘어지는 두 갈래(묶은 자리는 원본에서 놋쇠 고리로 찍는다).
"""
import math

from sd_head import View, main
from template_f_head import HIRES  # 고해상도 눈 — 문자가 같아 이 원본의 호박색 홍채로 칠해진다


def tail(x, y, cx, top=22, bottom=46):
    """묶은 갈래 하나 — 위에서 볼록하게 부풀었다가 끝이 뾰족해진다."""
    if not top <= y <= bottom:
        return False
    t = (y - top) / (bottom - top)
    half = 1.2 + 3.6 * math.sin(math.pi * min(1.0, t * 1.15))
    # 끝은 바깥으로 살짝 휜다
    c = cx + (2.0 * t * t if cx > 28 else -2.0 * t * t)
    return abs(x + 0.5 - c) <= half


def side_s(v, x, y):
    """정면 — 얼굴 옆 짧은 옆머리(턱까지) + 양갈래."""
    if 18 <= y <= 31 and (12.5 <= x + 0.5 <= 17.5 or 38.5 <= x + 0.5 <= 43.5):
        return True
    return tail(x, y, 9.5) or tail(x, y, 46.5)


def face_s(x, y):
    dx, dy = (x + 0.5 - 27.5) / 10.5, (y + 0.5 - 25.5) / 8.0
    if dx * dx + dy * dy > 1.0:
        return False
    if y >= 29:
        return abs(x + 0.5 - 27.3) <= (33.6 - y) * 1.9
    return True


def back_n(v, x, y):
    """뒷모습 — 목덜미까지 오는 뒷머리 + 양갈래."""
    if 18 <= y <= 35 and 13.5 <= x + 0.5 <= 42.5:
        i, u = v.clump(x, min(y, 26))
        return y <= 32 + 3 * (1 - abs(u - 0.5) * 2)
    return tail(x, y, 9.5) or tail(x, y, 46.5)


def side_e(v, x, y):
    """옆모습 — 뒤통수에서 목덜미로 내려오는 머리 + 뒤로 늘어지는 갈래 하나."""
    if 18 <= y <= 34 and 11.5 <= x + 0.5 <= 26.5 - max(0, y - 28) * 0.8:
        return True
    return tail(x, y, 12.5, top=21, bottom=45)


def face_e(x, y):
    if x < 27:
        return False
    dx, dy = (x + 0.5 - 31.5) / 7.8, (y + 0.5 - 25.5) / 8.0
    if dx * dx + dy * dy > 1.0:
        return False
    if y >= 29:
        return 28 <= x + 0.5 <= 39.5 - (y - 29) * 1.3
    return True


# 눈 — 둥글고 큰 눈, 아래쪽이 밝다 (호박색은 팔레트가 정한다)
EYES_S = (
    ([".KKKKK.", "KKQQqKK", "KQYYqrK", "KqYqrR.", ".qrRRR.", "..RRR.."], 18, 22),
    (["KKKKKK", "KQQqqK", "QYYqrK", "qYqrR.", "qrRRR.", ".RRR.."], 30, 22),
)
EYE_E = (["KKKKKK", ".KQqqK", ".QYYqr", ".qYqrR", ".qrRRR", "..RRK."], 30, 22)

VIEWS = [
    View("s", 28.0, 20.0, 15.0, 14.5,
         [17.5, 21.5, 25.5, 29.5, 33.5, 37.5], [11.0, 17.5, 24.0, 31.0, 37.5, 44.0],
         [22, 21, 24, 20, 22], face_s, side_s),
    View("n", 28.0, 20.0, 15.0, 14.5,
         [16.5, 20.5, 24.5, 28.0, 31.5, 35.5, 39.5], [10.0, 15.0, 21.0, 28.0, 35.0, 41.0, 46.0],
         None, None, back_n),
    View("e", 26.5, 20.0, 14.5, 14.5,
         [17.0, 21.0, 25.0, 29.0, 33.0, 37.0], [7.0, 12.0, 18.0, 25.0, 33.0, 41.0],
         [21, 22, 20, 22, 21], face_e, side_e),
]

if __name__ == "__main__":
    main("tools/forest_witch_v4_head.py", "sprites/forest_witch_v4.canvas", VIEWS, hires=HIRES, strands=True,
         ahoge_on=False, eyes_s=EYES_S, eye_e=EYE_E)
