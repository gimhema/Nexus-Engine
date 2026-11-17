"""플레이어 템플릿(남) 머리 — 짧게 뻗친 머리, 가는 눈, 홍조 없음. 칠하는 규칙은 tools/sd_head.py.

    python3 tools/template_m_head.py --write
"""
import sys

from sd_head import View, main


def side_s(v, x, y):
    """정면 — 귀밑까지 오는 짧은 옆머리. 아래로 갈수록 좁아진다."""
    if not 18 <= y <= 29:
        return False
    k = max(0.0, (y - 24) / 5)
    return 12.5 + k <= x + 0.5 <= 17.0 or 39.0 <= x + 0.5 <= 43.5 - k


def face_s(x, y):
    dx, dy = (x + 0.5 - 27.5) / 10.5, (y + 0.5 - 25.5) / 8.0
    if dx * dx + dy * dy > 1.0:
        return False
    if y >= 29:
        # 남성 턱 — 조금 더 각지고 넓다
        return abs(x + 0.5 - 27.3) <= (34.2 - y) * 2.0
    return True


def back_n(v, x, y):
    """뒷모습 — 목덜미에서 끝나는 짧은 머리, 끝은 다발마다 뾰족."""
    if not 18 <= y <= 32 or not 13.5 <= x + 0.5 <= 42.5:
        return False
    i, u = v.clump(x, min(y, 26))
    return y <= 28 + 3 * (1 - abs(u - 0.5) * 2)


def side_e(v, x, y):
    if not 18 <= y <= 30:
        return False
    return 13.0 + max(0, y - 24) * 0.6 <= x + 0.5 <= 26.5 - max(0, y - 26) * 1.2


def face_e(x, y):
    if x < 27:
        return False
    dx, dy = (x + 0.5 - 31.5) / 7.8, (y + 0.5 - 25.5) / 8.0
    if dx * dx + dy * dy > 1.0:
        return False
    if y >= 29:
        return 28 <= x + 0.5 <= 39.8 - (y - 29) * 1.1
    return True


# 가는 눈 — 윗속눈썹 한 줄, 홍채가 작다
EYES_S = (
    ([".KKKKK", "KQYqrK", "KqqrR.", ".rRR.."], 18, 23),
    (["KKKKK", "KYqrK", "qqrR.", ".RR.."], 30, 23),
)
EYE_E = (["KKKKK", ".KYqK", ".qqrR", "..RR."], 31, 23)
# 작은 머리(--scale < 0.9)용 — 5×3
SMALL = dict(
    eyes_s=((".KKKK KQYrK .rRR.".split(), 18, 23), ("KKKK KYrK rRR.".split(), 30, 23)),
    eye_e=("KKKK .KYq .rRR".split(), 31, 23),
)

VIEWS = [
    View("s", 28.0, 20.0, 15.0, 14.2,
         [17.0, 21.5, 25.5, 29.5, 33.5, 38.0], [10.0, 17.5, 24.0, 31.0, 37.5, 45.0],
         [21, 19, 22, 19, 21], face_s, side_s),
    View("n", 28.0, 20.0, 15.0, 14.2,
         [16.5, 20.5, 24.5, 28.0, 31.5, 35.5, 39.5], [10.0, 15.0, 21.0, 28.0, 35.0, 41.0, 46.0],
         None, None, back_n),
    View("e", 27.0, 20.0, 14.0, 14.2,
         [17.0, 21.0, 25.0, 29.0, 33.0, 37.0], [7.0, 12.0, 18.0, 25.0, 33.0, 41.0],
         [20, 21, 19, 21, 20], face_e, side_e),
]

# 고해상도(--res=2)용 — 머리 배율에 맞춰 그리는 눈 (tools/sd_head.py make_eye)
HIRES = "m"

if __name__ == "__main__":
    # `--style=ponytail|twintail|bun|spiky` — 머리 모양 라이브러리(tools/hair_styles.py)로 바꿔 칠한다
    _style = next((a.split("=", 1)[1] for a in sys.argv if a.startswith("--style=")), None)
    if _style:
        import hair_styles
        VIEWS = hair_styles.views(_style, "m", 46)
    main("tools/template_m_head.py", "sprites/template_m.canvas", VIEWS, small=SMALL, hires=HIRES, strands=True,
         ahoge_on=False, eyes_s=EYES_S, eye_e=EYE_E, blush=False)
