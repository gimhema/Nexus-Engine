"""플레이어 템플릿(여) 머리 — 허리까지 오는 긴 머리(살짝 물결), 큰 눈. 칠하는 규칙은 tools/sd_head.py.

    python3 tools/template_f_head.py --write
"""
import math
import sys

from sd_head import View, main

# 머리 끝 높이 (머리 좌표). 체형마다 `--hair-end=N` 으로 줄인다 — 꼬마는 어깨 단발.
HAIR_END = next((int(a.split("=")[1]) for a in sys.argv if a.startswith("--hair-end=")), 46)


def wave(y):
    """긴 머리의 물결 — 아래로 갈수록 크게 출렁인다."""
    return math.sin((y - 30) / 3.2) * max(0.0, (y - 30) / 14)


def side_s(v, x, y):
    """정면 — 얼굴 양옆으로 길게, 끝은 물결치며 뾰족해진다."""
    if not 18 <= y <= HAIR_END:
        return False
    k = max(0.0, (y - (HAIR_END - 10)) / 10)
    w = wave(y)
    l0, l1 = 11 + 2.5 * k + w, 17.5 - 1.0 * k + w
    r0, r1 = 38.5 + 1.0 * k - w, 45 - 2.5 * k - w
    return l0 <= x + 0.5 <= l1 or r0 <= x + 0.5 <= r1


def face_s(x, y):
    dx, dy = (x + 0.5 - 27.5) / 10.5, (y + 0.5 - 25.5) / 8.0
    if dx * dx + dy * dy > 1.0:
        return False
    if y >= 29:
        return abs(x + 0.5 - 27.3) <= (33.6 - y) * 1.9
    return True


def back_n(v, x, y):
    """뒷모습 — 등을 덮는 긴 머리. 어깨 아래로 등 가운데로 모이고 끝은 물결."""
    if not 18 <= y <= HAIR_END + 1:
        return False
    k = max(0.0, (y - 27) / 8)
    l, r = 12.5 + min(k, 1.0) * 4.0, 43.5 - min(k, 1.0) * 4.0
    if not l + wave(y) <= x + 0.5 <= r + wave(y):
        return False
    i, u = v.clump(x, min(y, 26))
    return y <= HAIR_END - 4 + 4 * (1 - abs(u - 0.5) * 2) + (i % 2)


def side_e(v, x, y):
    if not 18 <= y <= HAIR_END:
        return False
    k = max(0.0, (y - (HAIR_END - 12)) / 12)
    w = wave(y)
    l, r = 10.5 + 2.5 * k + w, 25.5 - 3.0 * k + w
    if not l <= x + 0.5 <= r:
        return False
    i, u = v.clump(x, min(y, 26))
    return y <= HAIR_END - 5 + 4 * (1 - abs(u - 0.5) * 2) + (i % 2)


def face_e(x, y):
    if x < 27:
        return False
    dx, dy = (x + 0.5 - 31.5) / 7.8, (y + 0.5 - 25.5) / 8.0
    if dx * dx + dy * dy > 1.0:
        return False
    if y >= 29:
        return 28 <= x + 0.5 <= 39.5 - (y - 29) * 1.3
    return True


# 큰 눈 — 속눈썹 꼬리가 바깥으로, 아래쪽이 밝은 홍채
EYES_S = (
    ([".KKKKK.", "KKQQqKK", "KQYYqrK", "KqYqrR.", ".qrRRR.", "..RRR.."], 18, 22),
    (["KKKKKK", "KQQqqK", "QYYqrK", "qYqrR.", "qrRRR.", ".RRR.."], 30, 22),
)
EYE_E = (["KKKKKK", ".KQqqK", ".QYYqr", ".qYqrR", ".qrRRR", "..RRK."], 30, 22)
# 작은 머리(--scale < 0.9)용 — 5×5, 하이라이트 하나
SMALL = dict(
    eyes_s=(([".KKKK", "KKQqK", "KQYqr", "KqrRR", ".RRR."], 18, 22),
            (["KKKK.", "KQqKK", "QYqrK", "qrRR.", ".RR.."], 30, 22)),
    eye_e=(["KKKKK", ".KQqK", ".QYqr", ".qrRR", "..RR."], 30, 22),
)

VIEWS = [
    View("s", 28.0, 20.0, 15.5, 14.8,
         [17.5, 21.5, 25.5, 29.5, 33.5, 37.5], [10.0, 17.5, 24.0, 31.0, 37.5, 45.0],
         [22, 21, 24, 21, 22], face_s, side_s),
    View("n", 28.0, 20.0, 15.5, 14.8,
         [16.5, 20.5, 24.5, 28.0, 31.5, 35.5, 39.5], [9.0, 15.0, 21.0, 28.0, 35.0, 41.0, 47.0],
         None, None, back_n),
    View("e", 26.5, 20.0, 14.5, 14.8,
         [17.0, 21.0, 25.0, 29.0, 33.0, 37.0], [6.0, 12.0, 18.0, 25.0, 33.0, 41.0],
         [21, 22, 20, 22, 21], face_e, side_e),
]

# 고해상도(--res=2)용 — 머리 배율에 맞춰 그리는 눈 (tools/sd_head.py make_eye)
HIRES = "f"

if __name__ == "__main__":
    # `--style=ponytail|twintail|bun|spiky` — 머리 모양 라이브러리(tools/hair_styles.py)로 바꿔 칠한다
    _style = next((a.split("=", 1)[1] for a in sys.argv if a.startswith("--style=")), None)
    if _style:
        import hair_styles
        VIEWS = hair_styles.views(_style, "f", HAIR_END)
    main("tools/template_f_head.py", "sprites/template_f.canvas", VIEWS, small=SMALL, hires=HIRES, strands=True,
         ahoge_on=False, eyes_s=EYES_S, eye_e=EYE_E)
