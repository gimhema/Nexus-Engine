"""암흑기사 v2 머리 — 흰 장발, 아호게. 칠하는 규칙은 tools/sd_head.py.

    python3 tools/dark_knight_v2_head.py --write
"""
from sd_head import View, main
from template_f_head import HIRES  # 고해상도 눈 — 문자가 같아 이 원본의 붉은 홍채로 칠해진다


def side_s(v, x, y):
    """정면 — 얼굴 양옆으로 길게 내려오는 머리, 끝은 좁아진다."""
    if not 18 <= y <= 45:
        return False
    k = max(0.0, (y - 34) / 11)
    l0, l1 = 11 + 2.5 * k, 17.5 - 1.5 * k
    r0, r1 = 38.5 + 1.5 * k, 45 - 2.5 * k
    if y >= 44 and x in (13, 42):
        return False
    return l0 <= x + 0.5 <= l1 or r0 <= x + 0.5 <= r1


def face_s(x, y):
    dx, dy = (x + 0.5 - 27.0) / 10.5, (y + 0.5 - 25.5) / 8.0
    if dx * dx + dy * dy > 1.0:
        return False
    if y >= 29:
        return abs(x + 0.5 - 26.8) <= (33.6 - y) * 1.9
    return True


def back_n(v, x, y):
    """뒷모습 — 정수리 아래 등을 덮는 머리. 아랫단은 다발마다 뾰족한 끝."""
    if not 18 <= y <= 47:
        return False
    # 어깨 아래로는 등 가운데로 모여 견갑이 드러난다
    k = max(0.0, (y - 27) / 8)
    l, r = 12.5 + min(k, 1.0) * 4.0, 43.5 - min(k, 1.0) * 4.0
    if not l <= x + 0.5 <= r:
        return False
    i, u = v.clump(x, min(y, 26))
    # 다발 끝: 가운데(u=0.5)가 가장 길다
    bottom = 41 + 5 * (1 - abs(u - 0.5) * 2) + (i % 2)
    return y <= bottom


def side_e(v, x, y):
    """옆모습(동쪽) — 뒤통수에서 등으로 흘러내리는 머리 (왼쪽)."""
    if not 18 <= y <= 46:
        return False
    k = max(0.0, (y - 34) / 12)
    l, r = 10.5 + 2.5 * k, 25.5 - 3.0 * k
    if not l <= x + 0.5 <= r:
        return False
    i, u = v.clump(x, min(y, 26))
    bottom = 41 + 4 * (1 - abs(u - 0.5) * 2) + (i % 2)
    return y <= bottom


def face_e(x, y):
    if x < 27:
        return False  # 그 앞은 귀를 덮은 옆머리
    dx, dy = (x + 0.5 - 31.5) / 7.8, (y + 0.5 - 25.5) / 8.0
    if dx * dx + dy * dy > 1.0:
        return False
    if y >= 29:
        return 28 <= x + 0.5 <= 39.5 - (y - 29) * 1.3
    return True


VIEWS = [
    View("s", 28.0, 20.0, 15.5, 14.8,
         [17.5, 21.5, 25.5, 29.5, 33.5, 37.5], [10.0, 17.5, 24.0, 31.0, 37.5, 45.0],
         [23, 21, 25, 21, 23], face_s, side_s),
    View("n", 28.0, 20.0, 15.5, 14.8,
         [16.5, 20.5, 24.5, 28.0, 31.5, 35.5, 39.5], [9.0, 15.0, 21.0, 28.0, 35.0, 41.0, 47.0],
         None, None, back_n),
    View("e", 26.5, 20.0, 14.5, 14.8,
         [17.0, 21.0, 25.0, 29.0, 33.0, 37.0], [6.0, 12.0, 18.0, 25.0, 33.0, 41.0],
         [22, 23, 21, 23, 21], face_e, side_e),
]

if __name__ == "__main__":
    main("tools/dark_knight_v2_head.py", "sprites/dark_knight_v2.canvas", VIEWS, hires=HIRES, strands=True)
