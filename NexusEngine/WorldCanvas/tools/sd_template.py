"""SD 플레이어 체형 생성기 — 체형 수치로 몸 겹·걷기 표·시트를 만든 `.canvas` 를 쓴다.

    python3 tools/sd_template.py              # BODIES 전부: sprites/template_<이름>.canvas + 머리 생성
    python3 tools/sd_template.py tall_m elf_f

머리는 tools/template_{m,f}_head.py (tools/sd_head.py 엔진)가 칠한다. 칸이 56×56 이 아니면 머리를
part 로 받아 `head_y` 만큼 내려 stamp 한다. 생성한 원본은 평범한 `.canvas` 다 — 손으로 고쳐도 되지만
다시 생성하면 덮인다. 체형을 고치려면 BODIES 의 수치를 고친다. 종이 인형(에디터 캐릭터 만들기)은
같은 겹 정의를 tools/sd_paperdoll.py 가 파츠별로 나눠 내보낸다.

겹은 재질별로 나뉜다 — 피부(몸통·팔·다리·손), 윗옷(튜닉/원피스)·소매, 바지, 장화, 머리, 종족 특징.
움직임은 겹의 **종류**(머리·몸·왼팔·오른다리·꼬리 …)가 정한다 → MOTION.

체형 수치 — 좌표는 정면 기준. 몸 가운데 x=28, 발바닥은 칸 바닥에서 3칸 위.
  h    칸 높이              head_y  머리를 내려 얹는 높이
  sw   어깨 반폭            ww      허리(옷단) 반폭
  tl   어깨→옷단 길이       ll      (참고) 옷단→발바닥
  lw   다리 폭              gap     두 다리 사이
  aw   팔 폭                al      어깨→손목 길이
  bh   장화 높이            lift    걷기에서 드는 발 높이     stride  옆모습 보폭
  skirt  여성 원피스 치마 퍼짐 (없으면 튜닉)
  beard  수염 · hair_end 여성 머리 끝 높이(머리 좌표, 기본 46) · skin 피부 기준색
  hair   머리 모양 (tools/hair_styles.py: ponytail · twintail · bun · spiky). 생략하면 남 짧은 머리 · 여 긴 머리
  ears   elf(뾰족 귀, 피부) · cat(고양이 귀, 머리색)    horns  뿔    tail  cat    tusks  엄니
  res    해상도 배율 (기본 1). 2 면 칸·좌표를 모두 두 배로, 시트 축척 32px/m — 게임 화면(1m = 32px)에 1:1.
         도형은 두 배 격자에서 다시 그려져 매끈해지고, 외곽선·주름 같은 1px 선은 그대로라 더 가늘다.
         머리는 머리 엔진이 `--res` 로 직접 112×112 에 칠한다 (눈·입은 고해상도 그림).
  head_scale  머리 배율 (기본 HEAD_SCALE). 머리는 목 기준으로 오그라들고 몸은 그대로 —
              참고 화풍은 머리 폭 24px 안팎이라 0.8 (설계 머리 폭 31px × 0.8)
"""
import math
import subprocess
import sys

CX = 28
HEAD_SCALE = 0.8

# 프레임 마무리 — 외곽선 계단 안쪽 모서리를 한 단계 어둡게(aa) 다듬은 뒤 재질별 색 외곽선(selout)
SEL = ("    aa o Ww wv vu sS ST ED DC CB Ll lj NM Mm Gg gy cd de FH HO\n"
       "    selout o WX wX vX uX YX sT ST TT tT EA DA CA BA LA lA jA "
       "Nn Mn mn Gh gh yh cf df IZ iZ JZ FU HU OU\n")


def fold(x0, y0, x1, y1, lit=True):
    """천 주름 한 줄 — 빛 쪽(왼쪽)은 밝은 줄 + 오른쪽에 짙은 줄, 그늘 쪽은 짙은 줄 + 중간 줄."""
    a, b = ("E", "B") if lit else ("B", "A")
    return [f"    line {x0} {y0} {x1} {y1} {a}", f"    line {x0 + 1} {y0} {x1 + 1} {y1} {b}"]


def trim(x0, x1, y, light="c", dark="d"):
    """크림 테·레이스 — 한 줄을 두 색 번갈아."""
    return [f"    line {x0} {y} {x1} {y} {dark}", f"    dither {dark} {light} {x0} {y} {x1 - x0 + 1} 1"]

# 성별마다 기본 색 (템플릿 m/f 와 같다)
COLORS = {
    "m": dict(hair="#6e4c3c", eye="#4a8ad8", cloth="#3e6aa8", pants="#4d4438"),
    "f": dict(hair="#e0a868", eye="#48b070", cloth="#b84c60", pants="#3a3550"),
}
# 아이소 화풍 — 흙빛·숲빛 바탕에 채도 높은 강조색 (참고: reference/isolmetricRef*.jpg 의 색감까지만)
COLORS_ISO = {
    "m": dict(hair="#7a4a2e", eye="#3a8a5a", cloth="#4f8f4a", pants="#6b4a33", accent="#c83c34"),
    "f": dict(hair="#e2702e", eye="#3a7ad0", cloth="#3d8fb8", pants="#4a3a58", accent="#d8a030"),
}

# 사실 등신 화풍 — 채도를 낮춘 흙빛·핏빛 (망토 깃은 강조색 자리에 올리브 울)
COLORS_REAL = {
    "m": dict(hair="#4a3426", eye="#5a6a78", cloth="#7a3430", pants="#4a4640", accent="#5a5c44"),
    "f": dict(hair="#5a3a2a", eye="#5a6a78", cloth="#3a4a6a", pants="#4a4640", accent="#6a4a3a"),
}

_STD_M = dict(sex="m", h=56, head_y=0, sw=9, ww=10, tl=11, ll=11, lw=5, gap=3, aw=5, al=7, bh=5, lift=2, stride=3)
_STD_F = dict(sex="f", h=56, head_y=0, sw=7, ww=6, tl=15, ll=7, lw=4, gap=2, aw=3, al=6, bh=5, lift=2, stride=3,
              skirt=12)
_TALL_M = dict(sex="m", h=72, head_y=2, sw=10, ww=9, tl=17, ll=18, lw=5, gap=3, aw=5, al=13, bh=6, lift=3, stride=4)
_TALL_F = dict(sex="f", h=72, head_y=2, sw=8, ww=6, tl=20, ll=15, lw=4, gap=2, aw=4, al=12, bh=6, lift=3, stride=4,
               skirt=10)
_BIG_M = dict(sex="m", h=72, head_y=4, sw=14, ww=12, tl=18, ll=14, lw=7, gap=3, aw=7, al=15, bh=6, lift=3, stride=4)

BODIES = {
    # 기본 — 손으로 쓴 template_m/f 와 같은 비율 (종족 템플릿의 바탕)
    "std_m": dict(_STD_M, label="표준 남"),
    "std_f": dict(_STD_F, label="표준 여"),
    # 장신 — 3등신. 칸 56×72
    "tall_m": dict(_TALL_M, label="장신 남"),
    "tall_f": dict(_TALL_F, label="장신 여"),
    # 꼬마·하플링 — 몸이 짧고 가늘다
    "small_m": dict(sex="m", label="꼬마 남", h=56, head_y=6, sw=7, ww=7, tl=9, ll=7, lw=4, gap=2,
                    aw=3, al=7, bh=3, lift=2, stride=2),
    "small_f": dict(sex="f", label="꼬마 여", h=56, head_y=6, sw=6, ww=5, tl=10, ll=6, lw=3, gap=2,
                    aw=3, al=7, bh=3, lift=2, stride=2, skirt=7, hair_end=36),
    # 드워프 — 짧고 아주 넓다
    "dwarf_m": dict(sex="m", label="드워프 남", h=56, head_y=4, sw=13, ww=12, tl=11, ll=7, lw=6, gap=2,
                    aw=6, al=9, bh=4, lift=2, stride=2, beard=True),
    "dwarf_f": dict(sex="f", label="드워프 여", h=56, head_y=4, sw=11, ww=9, tl=12, ll=6, lw=5, gap=2,
                    aw=5, al=9, bh=4, lift=2, stride=2, skirt=7, hair_end=40),
    # 거구 — 어깨가 아주 넓고 두껍다. 칸 56×72
    "big_m": dict(_BIG_M, label="거구 남"),
    # ── 종족 ──
    "elf_m": dict(_TALL_M, label="엘프 남", sw=9, ww=8, ears="elf", skin="#ffe6d8"),
    "elf_f": dict(_TALL_F, label="엘프 여", ears="elf", skin="#ffe6d8"),
    "beast_m": dict(_STD_M, label="수인 남", ears="cat", tail="cat"),
    "beast_f": dict(_STD_F, label="수인 여", ears="cat", tail="cat"),
    "demon_m": dict(_STD_M, label="마족 남", horns=True, skin="#b8c0ec"),
    "demon_f": dict(_STD_F, label="마족 여", horns=True, skin="#b8c0ec"),
    "orc_m": dict(_BIG_M, label="오크 남", tusks=True, skin="#9cc070"),
    # ── 아이소 화풍 — 약 3.6등신(칸 56×80, 머리 0.62), 셀 셰이딩, 모험가 장비 (iso_dress) ──
    "iso_m": dict(sex="m", label="아이소 남", style="iso", res=2, h=80, head_y=0, head_scale=0.62, sw=10, ww=8, tl=18,
                  ll=28, lw=5, gap=3, aw=5, al=17, bh=8, lift=3, stride=4),
    "iso_f": dict(sex="f", label="아이소 여", style="iso", res=2, h=80, head_y=0, head_scale=0.62, sw=8, ww=5, tl=19,
                  ll=26, lw=4, gap=3, aw=4, al=16, bh=8, lift=3, stride=4, skirt=9, hair_end=40),
    # ── 사실 등신 화풍 — 약 7등신(칸 56×80), 작은 사실적 머리(real_head), 망토 깃·가죽 장갑 (real_dress) ──
    "real_m": dict(sex="m", label="사실 등신 남", style="real", res=2, h=80, head_y=-13, head_scale=0.42, skin="#e2b898",
                   sw=9, ww=7, tl=26, ll=31, lw=6, gap=2, aw=5, al=24, bh=10, lift=3, stride=3),
}
# 모든 체형의 기본 해상도 — 32px/m (게임 화면 1:1). 종이 인형은 sd_paperdoll.py 가 1배로 따로 만든다.
for _b in BODIES.values():
    _b.setdefault("res", 2)


# ─────────────────────────────────────────────────────────────────────────────
# 겹 — 이름 → 연산 줄. 몸 겹은 칸 크기 frame(좌표에 head_y 포함), 머리 좌표 겹은 56×56 part.
# ─────────────────────────────────────────────────────────────────────────────

def _y(b):
    """(어깨, 옷단, 허리띠, 발바닥, 장화 위, 다리 위)"""
    sy = 31 + b["head_y"]
    hem = sy + b["tl"]
    belt = sy + round(b["tl"] * (0.45 if b.get("skirt") else 0.55))
    foot = b["h"] - 3
    bt = foot - b["bh"] + 1
    return sy, hem, belt, foot, bt, hem - 2


def top_front(b, back=False, dress=None):
    """정면(back=True 면 뒷면) 윗옷 — 튜닉 또는 원피스(몸판+치마)."""
    sy, hem, belt, *_ = _y(b)
    sw, ww = b["sw"], b["ww"]
    fl = dress if dress is not None else 0
    out = []
    if fl:
        waist = sy + round(b["tl"] * 0.45)
        out += [f"    poly D {CX - sw} {sy} {CX + sw} {sy} {CX + ww} {waist} {CX - ww} {waist}",
                "    light cloth 0 2 1",
                f"    poly C {CX - ww} {waist + 2} {CX + ww} {waist + 2} {CX + fl} {hem} {CX - fl} {hem}",
                "    light cloth 0 2 1",
                *fold(CX - ww + 1, waist + 3, CX - fl + 2, hem - 1),
                *fold(CX - 2, waist + 3, CX - 3, hem - 1),
                *fold(CX + 2, waist + 3, CX + 3, hem - 1, lit=False),
                *fold(CX + ww - 2, waist + 3, CX + fl - 3, hem - 1, lit=False),
                *trim(CX - fl, CX + fl, hem),
                *[f"    px {x} {hem + 1} d" for x in range(CX - fl + 1, CX + fl, 3)],
                f"    rect {CX - ww - 1} {waist} {2 * ww + 3} 2 m",
                f"    dither m n {CX - ww - 1} {waist + 1} {2 * ww + 3} 1",
                # 몸판 — 가슴 아래 가는 주름과 앞 여밈 끈
                *fold(CX - sw + 2, sy + 2, CX - ww + 1, waist - 1),
                *[f"    px {CX} {y} g" for y in range(sy + 5, waist, 2)]]
        if back:
            out += [f"    line {CX - 2} {waist} {CX + 2} {waist + 1} g", f"    line {CX + 2} {waist} {CX - 2} {waist + 1} g"]
        else:
            out += [f"    px {CX - 2} {waist + 1} g", f"    px {CX} {waist + 1} g", f"    px {CX + 2} {waist + 1} g"]
    else:
        out += [f"    poly D {CX - sw} {sy} {CX + sw} {sy} {CX + sw + 1} {sy + 6} {CX + ww + 1} {hem} "
                f"{CX - ww - 1} {hem} {CX - sw - 1} {sy + 6}",
                "    light cloth 0 2 1",
                *fold(CX - sw + 3, sy + 3, CX - sw + 2, belt - 1),
                *fold(CX + sw - 4, sy + 3, CX + sw - 3, belt - 1, lit=False),
                # 넓은 몸통은 가슴에 한 쌍 더 + 가로 가슴 주름 — 큰 면이 비지 않게
                *(fold(CX - ww // 2, sy + 5, CX - ww // 2 + 1, belt - 1) +
                  fold(CX + ww // 2 - 1, sy + 5, CX + ww // 2 - 2, belt - 1, lit=False) +
                  [f"    line {CX - ww // 2 + 2} {sy + 5} {CX - 2} {sy + 6} B",
                   f"    line {CX + 2} {sy + 6} {CX + ww // 2 - 2} {sy + 5} B",
                   f"    line {CX - ww // 2 + 2} {sy + 4} {CX - 3} {sy + 4} E"] if ww >= 9 else []),
                # 허리띠 아래 짧은 주름 — 옷단까지
                *fold(CX - ww + 2, belt + 2, CX - ww + 1, hem - 1),
                *fold(CX + ww - 3, belt + 2, CX + ww - 2, hem - 1, lit=False),
                f"    line {CX} {belt + 2} {CX} {hem - 1} B",
                f"    rect {CX - ww - 1} {belt} {2 * ww + 3} 2 m",
                f"    dither m n {CX - ww - 1} {belt + 1} {2 * ww + 3} 1",
                *trim(CX - ww - 1, CX + ww + 1, hem)]
        if back:
            out += [f"    line {CX} {sy + 1} {CX} {belt - 1} B"]
        else:
            out += [f"    rect {CX - 2} {belt} 4 2 g", f"    px {CX - 1} {belt} G", f"    px {CX + 1} {belt + 1} h",
                    # 앞 여밈 단추
                    *[f"    px {CX} {y} g" for y in range(sy + 6, belt - 1, 3)]]
    if not back:
        # 목 트임 — 옷에 V 구멍(아래 맨몸이 비친다), 크림 깃 테
        out += [f"    poly . {CX - 3} {sy} {CX + 3} {sy} {CX} {sy + 4}",
                f"    line {CX - 4} {sy} {CX} {sy + 4} d",
                f"    line {CX + 4} {sy} {CX} {sy + 4} e"]
    return out + ["    outline o"]


def top_side(b, dress=None):
    sy, hem, belt, *_ = _y(b)
    d = max(5, round(b["sw"] * 0.62))
    out = []
    if dress:
        fl = round(dress * 0.8)
        waist = sy + round(b["tl"] * 0.45)
        out += [f"    poly D {CX - d} {sy} {CX + d} {sy} {CX + d - 1} {waist} {CX - d + 1} {waist}",
                "    light cloth 0 2 1",
                f"    poly C {CX - d} {waist + 2} {CX + d} {waist + 2} {CX + fl + 1} {hem} {CX - fl - 1} {hem}",
                "    light cloth 0 2 1",
                *fold(CX - d + 2, waist + 3, CX - fl + 1, hem - 1, lit=False),
                *fold(CX, waist + 3, CX + 1, hem - 1),
                *fold(CX + d - 2, waist + 3, CX + fl - 2, hem - 1),
                *trim(CX - fl - 1, CX + fl + 1, hem),
                *[f"    px {x} {hem + 1} d" for x in range(CX - fl, CX + fl + 1, 3)],
                f"    rect {CX - d - 1} {waist} {2 * d + 2} 2 m",
                f"    dither m n {CX - d - 1} {waist + 1} {2 * d + 2} 1",
                f"    px {CX + d - 2} {waist + 1} g"]
    else:
        out += [f"    poly D {CX - d} {sy} {CX + d} {sy} {CX + d + 1} {sy + 6} {CX + d + 1} {hem} "
                f"{CX - d - 1} {hem} {CX - d - 1} {sy + 6}",
                "    light cloth 0 2 1",
                *fold(CX - d + 2, sy + 3, CX - d + 1, belt - 1, lit=False),
                *fold(CX + 1, sy + 4, CX + 2, belt - 1),
                *fold(CX - 1, belt + 2, CX - 2, hem - 1),
                f"    rect {CX - d - 1} {belt} {2 * d + 3} 2 m",
                f"    dither m n {CX - d - 1} {belt + 1} {2 * d + 3} 1",
                f"    rect {CX + d - 2} {belt} 3 2 g",
                f"    px {CX + d - 1} {belt} G",
                *trim(CX - d - 1, CX + d + 1, hem)]
    out += [f"    poly . {CX + d - 3} {sy} {CX + d} {sy} {CX + d - 1} {sy + 3}"]
    return out + ["    outline o"]


def skin_torso(b, view):
    """맨몸 몸통 — 옷 아래. 옷보다 한 칸 좁다 (옷을 입으면 안 보인다)."""
    sy, hem, *_ = _y(b)
    if view == "e":
        d = max(5, round(b["sw"] * 0.62)) - 1
        pts = f"{CX - d} {sy} {CX + d} {sy} {CX + d} {hem - 1} {CX - d} {hem - 1}"
    else:
        sw, ww = b["sw"] - 1, min(b["ww"], b["sw"]) - 1
        pts = f"{CX - sw} {sy} {CX + sw} {sy} {CX + ww} {hem - 1} {CX - ww} {hem - 1}"
    return [f"    poly S {pts}", "    light skin 0 1 0", "    outline o"]


def _arm_pts(b, sign):
    sy = 31 + b["head_y"]
    x_in = CX + sign * (b["sw"] - 2)
    x_out = CX + sign * (b["sw"] + b["aw"] - 2)
    lo, hi = sorted((x_in, x_out))
    return lo, hi, sy + 1, sy + b["al"]


def arm_front(b, sign, cloth):
    if b.get("res", 1) > 1:
        return arm_front_fine(b, sign, cloth)
    lo, hi, top, wrist = _arm_pts(b, sign)
    if cloth:
        mid = (top + wrist) // 2
        return [f"    poly D {lo} {top} {hi} {top} {hi} {wrist} {lo} {wrist}", "    light cloth 0 2 1",
                # 팔꿈치 주름과 소매 끝단(두 색 테). 굵은 팔은 위팔에 밝은 결·그늘 결
                f"    line {lo + 1} {mid} {hi - 1} {mid + 1} B", f"    px {lo + 1} {mid - 1} E",
                *([f"    line {lo + 1} {top + 2} {lo + 1} {mid - 2} E", f"    line {hi - 1} {top + 2} {hi - 1} {mid - 2} B"]
                  if hi - lo >= 5 else []),
                *trim(lo, hi, wrist - 1), "    outline o"]
    return [f"    poly S {lo} {top} {hi} {top} {hi} {wrist} {lo} {wrist}", "    light skin 0 1 0", "    outline o"]


# ── 고해상도 정면 도형 — 팔다리가 가늘어지고(어깨→팔목, 허벅지→무릎→종아리→발목) 마디가 보인다 ──

def arm_front_fine(b, sign, cloth):
    lo, hi, top, wrist = _arm_pts(b, sign)
    c, aw = (lo + hi) / 2, b["aw"]
    mid = (top + wrist) / 2
    # 바깥쪽(sign)으로 팔꿈치가 살짝 나온다
    pts = [(c - aw / 2, top), (c + aw / 2, top), (c + sign * 0.3 + aw * 0.45, mid),
           (c + aw * 0.36, wrist), (c - aw * 0.36, wrist), (c + sign * 0.3 - aw * 0.45, mid)]
    poly = " ".join(f"{_n(x)} {_n(y)}" for x, y in pts)
    cap = _ellipse_at((c, top + 1), aw + 1, 3.5, "D" if cloth else "S")
    if cloth:
        return [cap, f"    poly D {poly}", "    light cloth 0 1 1",
                # 팔꿈치 주름 두 겹과 위팔 밝은 결
                f"    line {_n(c - aw * 0.35)} {_n(mid)} {_n(c + aw * 0.3)} {_n(mid + 0.5)} B",
                f"    line {_n(c - aw * 0.2)} {_n(mid + 1.5)} {_n(c + aw * 0.25)} {_n(mid + 1.5)} B",
                f"    line {_n(c - aw * 0.3)} {_n(top + 2)} {_n(c - aw * 0.3)} {_n(mid - 1.5)} E",
                *trim(round(c - aw * 0.36), round(c + aw * 0.36), round(wrist - 1)), "    outline o"]
    return [cap, f"    poly S {poly}", "    light skin 0 1 0", "    outline o"]


def hand_fine(b, wrist_c, sign):
    """손 — 손바닥, 몸 쪽 엄지, 손가락 마디 그늘, 손등 하이라이트."""
    aw = b["aw"]
    # 팔이 굵어도 손은 완만하게 커진다 — 비례로 키우면 굵은 체형의 손이 공처럼 된다
    w, h = 3.2 + aw * 0.45, 4.0 + aw * 0.45
    cx, cy = wrist_c[0], wrist_c[1] + h / 2
    return [_ellipse_at((cx, cy), w, h, "S"), _ellipse_at((cx - sign * w * 0.45, cy - h * 0.15), 1.6, 2.6, "S"),
            "    light skin 0 1 0",
            f"    line {_n(cx - w * 0.3)} {_n(cy + h * 0.25)} {_n(cx + w * 0.3)} {_n(cy + h * 0.25)} T",
            f"    px {_n(cx - w * 0.15)} {_n(cy - h * 0.2)} s", "    outline o"]


def leg_fine(b, sign, what):
    """정면·뒷면 다리 한 짝 (고해상도) — 허벅지가 넓고 무릎에서 좁아졌다 종아리에서 볼록, 발목에서 가늘다."""
    _, _, _, foot, bt, top = _y(b)
    x0, x1 = _leg_x(b, sign)
    c, lw = (x0 + x1 + 1) / 2, b["lw"]
    knee = top + (foot - top) * 0.45
    calf = knee + (foot - knee) * 0.3
    if what == "boot":
        toe = sign * 0.6
        return [f"    poly M {_n(c - lw * 0.55)} {bt} {_n(c + lw * 0.55)} {bt} {_n(c + lw * 0.5)} {_n(foot - 2)} "
                f"{_n(c - lw * 0.5)} {_n(foot - 2)}",
                _ellipse_at((c + toe, foot - 1.2), lw + 1.6, 3.2, "M"), "    light leather 0 1 1",
                # 접은 목 · 앞코 광택 · 밑창 · 굽 · 버클
                f"    rect {_n(c - lw * 0.6)} {bt} {_n(lw * 1.2)} 1.5 m",
                f"    line {_n(c - lw * 0.55)} {bt} {_n(c + lw * 0.55)} {bt} N",
                f"    px {_n(c + toe - 1)} {_n(foot - 2)} N",
                f"    line {_n(c - lw * 0.6 + toe)} {foot} {_n(c + lw * 0.6 + toe)} {foot} n",
                f"    px {_n(c + lw * 0.4)} {_n(bt + 3)} g", "    outline o"]
    end = foot if what == "skin" else bt + 1
    pts = [(c - lw / 2, top), (c + lw / 2, top), (c + lw * 0.42, knee), (c + lw * 0.48, calf),
           (c + lw * 0.34, end), (c - lw * 0.34, end), (c - lw * 0.48, calf), (c - lw * 0.42, knee)]
    poly = " ".join(f"{_n(x)} {_n(y)}" for x, y in pts)
    if what == "skin":
        return [f"    poly S {poly}", "    light skin 0 1 0", "    outline o"]
    return [f"    poly L {poly}", "    light pants 0 1 1",
            # 무릎 — 짙은 주름 위 밝은 턱, 바깥쪽 솔기, 정강이 밝은 결
            f"    line {_n(c - lw * 0.3)} {_n(knee)} {_n(c + lw * 0.3)} {_n(knee + 0.5)} j",
            f"    line {_n(c - lw * 0.2)} {_n(knee - 1)} {_n(c + lw * 0.2)} {_n(knee - 1)} L",
            f"    line {_n(c + sign * lw * 0.3)} {top + 1} {_n(c + sign * lw * 0.3)} {_n(knee - 2)} l",
            f"    line {_n(c - lw * 0.2)} {_n(calf)} {_n(c - lw * 0.15)} {bt - 1} L",
            "    outline o"]


def hand_front(b, sign):
    lo, hi, _, wrist = _arm_pts(b, sign)
    if b.get("res", 1) > 1:
        return hand_fine(b, ((lo + hi) / 2, wrist), sign)
    x0, w, h = lo - (1 if sign < 0 else 0), b["aw"] + 1, max(4, b["aw"])
    out = [f"    ellipse {x0} {wrist} {w} {h} S", "    light skin 0 1 0"]
    if w >= 6:
        # 큰 손 — 손가락 마디 그늘과 손등 하이라이트
        out += [f"    line {x0 + 1} {wrist + h - 2} {x0 + w - 2} {wrist + h - 2} T",
                *[f"    px {x} {wrist + h - 3} T" for x in range(x0 + 2, x0 + w - 1, 2)],
                f"    px {x0 + 1} {wrist + 1} s"]
    return out + ["    outline o"]


def _leg_x(b, sign):
    if sign < 0:
        x0 = CX - b["gap"] // 2 - b["lw"]
    elif sign > 0:
        x0 = CX + (b["gap"] + 1) // 2
    else:
        x0 = CX - b["lw"] // 2
    return x0, x0 + b["lw"] - 1


def leg(b, sign, what):
    """정면·뒷면 다리 한 짝 — what: skin(맨다리) · pants · boot. sign: -1 왼쪽 / 1 오른쪽."""
    if b.get("res", 1) > 1:
        return leg_fine(b, sign, what)
    _, _, _, foot, bt, top = _y(b)
    x0, x1 = _leg_x(b, sign)
    if what == "skin":
        return [f"    rect {x0} {top} {b['lw']} {foot - top + 1} S", "    light skin 0 1 0", "    outline o"]
    if what == "pants":
        knee = (top + bt) // 2
        return [f"    rect {x0} {top} {b['lw']} {bt - top + 1} L", "    light pants 0 1 1",
                # 무릎 — 짙은 주름 위 밝은 턱, 안쪽 솔기
                f"    line {x0 + 1} {knee} {x1} {knee} j", f"    px {x0 + 1} {knee - 1} L",
                f"    line {x1} {top + 1} {x1} {knee - 2} l",
                # 긴 다리는 정강이 주름과 앞쪽 밝은 결
                *([f"    line {x0 + 1} {(knee + bt) // 2} {x1 - 1} {(knee + bt) // 2 + 1} j",
                   f"    line {x0} {knee + 2} {x0} {bt - 2} L"] if bt - top > 10 else []),
                # 굵은 다리는 가운데 다림 주름
                *([f"    line {(x0 + x1) // 2} {top + 1} {(x0 + x1) // 2} {bt - 1} l"] if b["lw"] >= 6 else []),
                "    outline o"]
    lo = x0 - (1 if sign < 0 else 0)
    hi = x1 + (1 if sign > 0 else 0)
    pts = f"{lo} {bt} {hi} {bt} {hi} {foot} {lo} {foot}"
    return [f"    poly M {pts}", "    light leather 0 1 1",
            # 접은 목 · 밑창 · 앞코 광택 · 버클
            f"    line {x0} {bt + 1} {x1} {bt + 1} n",
            f"    line {x0 - 1} {foot} {x1 + 1} {foot} m",
            f"    px {x0} {foot - 2} N", f"    px {x1} {bt + 3} g", "    outline o"]


# ── 옆모습 골격 — 엉덩이→무릎→발목, 어깨→팔꿈치→손목. 자세마다 다각형을 직접 계산한다 ──
# stamp_rot 으로 조각을 돌리면 무릎·팔꿈치에 이음선이 생긴다 — 대신 마디 둘을 한 겹 안에서 같은 색으로
# 칠하고 외곽선을 한 번 두른다. 각도는 도, 양수 = 앞(동쪽)으로. 굽힘(bend)은 아랫마디가 뒤로 꺾이는 양.

# 자세 — (가까운 다리, 먼 다리, 가까운 팔, 먼 팔). 다리 (허벅지 각, 무릎 굽힘), 팔 (위팔 각, 팔꿈치 굽힘).
POSES = {
    "i": ((5, 0), (-5, 0), (2, 8), (-2, 8)),
    # 0 가까운 다리 딛음·먼 다리 밂 · 1 가까운 다리 받침·먼 다리 굽혀 지나감 · 2·3 반대
    "w0": ((24, 4), (-22, 10), (-18, 8), (16, 22)),
    "w1": ((0, 0), (14, 48), (0, 10), (0, 10)),
    "w2": ((-22, 10), (24, 4), (16, 22), (-18, 8)),
    "w3": ((14, 48), (0, 0), (0, 10), (0, 10)),
    # 공격 — 가까운 팔이 무기 팔: 뒤로 들어 올림 → 내리침 → 따라감 → 돌아옴. 다리는 앞뒤로 벌린 자세
    "a0": ((14, 6), (-14, 6), (200, 20), (20, 30)),
    "a1": ((16, 6), (-14, 6), (60, 10), (-10, 20)),
    "a2": ((16, 6), (-14, 6), (35, 0), (-15, 15)),
    "a3": ((12, 6), (-12, 6), (15, 15), (0, 10)),
    # 시전 — 두 팔을 앞으로 모아 올린다
    "c0": ((6, 0), (-6, 0), (40, 40), (30, 40)),
    "c1": ((6, 0), (-6, 0), (80, 30), (70, 30)),
    "c2": ((6, 0), (-6, 0), (95, 20), (85, 20)),
    "c3": ((6, 0), (-6, 0), (60, 30), (50, 30)),
    # 피격 — 팔이 뒤로 젖혀진다
    "h0": ((4, 4), (-10, 4), (-35, 20), (-25, 20)),
    "h1": ((4, 2), (-8, 2), (-20, 15), (-15, 15)),
}
# 정면 팔 자세 (동작 클립만) — (오른팔 = 화면 오른쪽 · 무기 팔, 왼팔). 각도 양수 = 몸 바깥쪽, 음수 = 몸 앞을 가로질러.
FRONT_POSES = {
    "a0": ((165, 25), (25, 20)),
    "a1": ((-30, 25), (30, 20)),
    "a2": ((-45, 30), (25, 15)),
    "a3": ((10, 15), (12, 10)),
    # 시전 — 정면은 팔을 들면 큰 머리 뒤로 숨는다. 두 손을 가슴 앞으로 모은다
    "c0": ((-20, 40), (-20, 40)),
    "c1": ((-45, 60), (-45, 60)),
    "c2": ((-55, 70), (-55, 70)),
    "c3": ((-30, 50), (-30, 50)),
    "h0": ((60, 20), (60, 20)),
    "h1": ((35, 15), (35, 15)),
}
# 동작마다 몸 전체가 밀리는 양(옆모습 x) — 공격은 앞으로 내딛고 피격은 뒤로 밀린다
POSE_DX = {"a1": 1, "a2": 1, "h0": -2, "h1": -1}
# (키, 상태, 프레임 수, 프레임 ms, loop|once, 자세 글자). 시트는 이 순서로 방향 4행씩 쌓인다.
CLIPS = [("idle", "Idle", 2, 450, "loop", "i"), ("walk", "Walk", 4, 160, "loop", "w"),
         ("attack", "Attack", 4, 110, "once", "a"), ("cast", "Cast", 4, 140, "once", "c"),
         ("hit", "Hit", 2, 130, "once", "h"), ("die", "Die", 4, 180, "once", None)]
# 지나가는 다리 — 이 자세에서 땅을 딛지 않는 다리 (가까운 다리인가). 몸 높이는 딛는 다리로만 정하고,
# 지나가는 다리가 땅에 닿으면 그 다리만 들어 올린다 (짧은 다리는 무릎을 굽혀도 발이 덜 들린다).
SWING = {"w1": False, "w3": True}
# 발 — 발목 기준 (앞 = +x, 아래 = +y). 바닥은 발목 +2.
FOOT = [(-2, -1), (2, -1), (4, 0), (5, 1), (5, 2), (-2, 2)]


def _rot(pts, deg):
    """화면 좌표에서 시계 방향(양수)으로 돌린다."""
    r = math.radians(deg)
    c, s_ = math.cos(r), math.sin(r)
    return [(x * c - y * s_, x * s_ + y * c) for x, y in pts]


def _dir(deg):
    """각도 → 아래를 향한 단위 벡터 (양수 = 앞으로)."""
    r = math.radians(deg)
    return math.sin(r), math.cos(r)


# 고해상도(res ≥ 2) 원본을 만드는 중인가 — 그때는 좌표를 실수로 내보내고 upscale() 이 배율을 곱한 뒤 반올림한다.
# 1배 원본은 지금처럼 정수 (출력이 바뀌지 않게). layers() 가 체형마다 정한다.
FINE = False


def _n(v):
    return f"{v:.2f}".rstrip("0").rstrip(".") if FINE else str(round(v))


def _quad(p0, p1, w, w1=None):
    """선분 p0→p1 을 사각형 꼭짓점 문자열로 — 폭은 p0 쪽 w, p1 쪽 w1 (없으면 같은 폭)."""
    w1 = w if w1 is None else w1
    dx, dy = p1[0] - p0[0], p1[1] - p0[1]
    n = math.hypot(dx, dy) or 1.0
    ux, uy = -dy / n / 2, dx / n / 2
    pts = [(p0[0] + ux * w, p0[1] + uy * w), (p1[0] + ux * w1, p1[1] + uy * w1),
           (p1[0] - ux * w1, p1[1] - uy * w1), (p0[0] - ux * w, p0[1] - uy * w)]
    return " ".join(f"{_n(x)} {_n(y)}" for x, y in pts)


def _ellipse_at(c, w, h, colour):
    return f"    ellipse {_n(c[0] - w / 2)} {_n(c[1] - h / 2)} {_n(w)} {_n(h)} {colour}"


def _leg_geo(b, pose_leg, hip_x, dy):
    """(엉덩이, 무릎, 발목, 장화 위, 정강이 각, 발 꼭짓점들) — 엉덩이는 dy 만큼 내려간 자리."""
    _, _, _, foot, _, top = _y(b)
    hip_y = top + 1
    total = (foot - 2) - hip_y
    lt = round(total * 0.5)
    ls = total - lt
    th, bend = pose_leg
    # 짧은 다리는 같은 각도면 장화끼리 뭉친다 — 다리 길이에 비례해 줄인다 (16px 이상이면 그대로)
    reach = min(1.0, total / 16) * b["stride"] / 3
    th, bend = th * reach, bend * min(1.0, total / 12)
    hip = (hip_x, hip_y + dy)
    kx, ky = _dir(th)
    knee = (hip[0] + lt * kx, hip[1] + lt * ky)
    shin = th - bend
    sx, sy_ = _dir(shin)
    ankle = (knee[0] + ls * sx, knee[1] + ls * sy_)
    boot_top = (ankle[0] - (b["bh"] - 2) * sx, ankle[1] - (b["bh"] - 2) * sy_)
    foot_pts = [(ankle[0] + x, ankle[1] + y) for x, y in _rot(FOOT, -shin * 0.8)]
    return hip, knee, ankle, boot_top, shin, foot_pts


def side_offsets(b):
    """자세마다 엉덩이를 얼마나 내려야 가장 낮은 발이 땅에 닿는가 — 몸 오르내림이 다리에서 나온다."""
    _, _, _, foot, _, _ = _y(b)
    out = {}
    for name, (near, far, *_) in POSES.items():
        legs = [(near, CX, True), (far, CX - 2, False)]
        low = max(y for leg_pose, hx, is_near in legs if SWING.get(name) != is_near
                  for _, y in _leg_geo(b, leg_pose, hx, 0)[5])
        out[name] = round(foot - low)
    return out


# 다리 재질 — 템플릿 기본. 다른 원본(암흑기사·마녀)은 tools/skeleton_legs.py 가 자기 문자로 넘긴다.
#   pants: (색, 램프, 무릎 짙은 점, 무릎 밝은 점, 줄무늬 색 또는 None) · boot: (색, 램프, 목 테 색)
LEG_MATS = {"pants": ("L", "pants", "j", "L", None), "boot": ("M", "leather", "n")}


def leg_side_pose(b, pose, near, what, mats=LEG_MATS):
    """옆모습 다리 한 짝, 한 자세 — what: skin · pants · boot. 먼 다리는 한 단계 어둡게."""
    dy = side_offsets(b)[pose]
    leg_pose = POSES[pose][0 if near else 1]
    hx = CX if near else CX - 2
    geo = _leg_geo(b, leg_pose, hx, dy)
    if SWING.get(pose) == near:
        # 지나가는 다리 — 발이 땅에 닿거나 파고들면 한 칸 떠 있게 들어 올린다
        low = max(y for _, y in geo[5])
        foot = b["h"] - 3
        if low > foot - 1:
            geo = _leg_geo(b, leg_pose, hx, dy - (low - foot) - 1)
    hip, knee, ankle, boot_top, shin, foot_pts = geo
    w = b["lw"] - 1
    foot_poly = " ".join(f"{round(x)} {round(y)}" for x, y in foot_pts)
    if what == "skin":
        taper = 0.75 if FINE else 1.0
        out = [f"    poly S {_quad(hip, knee, w, w * 0.85 if FINE else None)}",
               f"    poly S {_quad(knee, ankle, w * 0.85 if FINE else w, w * taper * 0.85 if FINE else None)}",
               _ellipse_at(knee, b["lw"], b["lw"], "S"), f"    poly S {foot_poly}", "    light skin 0 1 0"]
        ramp = "skin"
    elif what == "pants":
        c, ramp, dark, lit, stripe = mats["pants"]
        out = [f"    poly {c} {_quad(hip, knee, w, w * 0.85 if FINE else None)}",
               f"    poly {c} {_quad(knee, boot_top, w * 0.85 if FINE else w, w * 0.7 if FINE else None)}",
               _ellipse_at(knee, b["lw"], b["lw"], c), f"    light {ramp} 0 1 1"]
        if stripe:
            # 줄무늬 — 다리 방향에 가로로 두 칸마다 (허벅지·정강이 각각의 각도로)
            for p0, p1 in ((hip, knee), (knee, boot_top)):
                ln = math.hypot(p1[0] - p0[0], p1[1] - p0[1]) or 1.0
                ux, uy = (p1[0] - p0[0]) / ln, (p1[1] - p0[1]) / ln
                nx, ny = -uy * w / 2, ux * w / 2
                for t in range(1, int(ln), 2):
                    cx_, cy_ = p0[0] + ux * t, p0[1] + uy * t
                    out.append(f"    line {round(cx_ - nx)} {round(cy_ - ny)} {round(cx_ + nx)} {round(cy_ + ny)} {stripe}")
        else:
            # 무릎 주름 — 무릎 앞쪽 짙은 점과 위쪽 밝은 점
            out += [f"    px {round(knee[0] + 1)} {round(knee[1])} {dark}",
                    f"    px {round(knee[0])} {round(knee[1]) - 2} {lit}"]
    else:
        c, ramp, cuff = mats["boot"]
        out = [f"    poly {c} {_quad(boot_top, ankle, b['lw'] + 1)}", f"    poly {c} {foot_poly}",
               f"    light {ramp} 0 1 1"]
        # 장화 목 — 정강이에 가로로 짙은 줄
        sx, sy_ = _dir(shin)
        nx, ny = sy_ * (b["lw"] + 1) / 2, -sx * (b["lw"] + 1) / 2
        out.append(f"    line {round(boot_top[0] + nx)} {round(boot_top[1] + ny)} "
                   f"{round(boot_top[0] - nx)} {round(boot_top[1] - ny)} {cuff}")
    if not near:
        out.append(f"    shade {ramp} 1")
    return out + ["    outline o"]


def _arm_geo(b, shoulder, a, bend, sign=1):
    """(팔꿈치, 손목, 아래팔 방향) — 각도 양수 = sign 쪽(앞·바깥), 굽힘은 같은 쪽으로 더 접힌다."""
    lu = round(b["al"] * 0.5)
    lf = b["al"] - lu
    ux, uy = _dir(a)
    elbow = (shoulder[0] + sign * lu * ux, shoulder[1] + lu * uy)
    fx, fy = _dir(a + bend)
    fx *= sign
    wrist = (elbow[0] + lf * fx, elbow[1] + lf * fy)
    return elbow, wrist, (fx, fy)


def side_shoulder(b, pose):
    return CX - 1, 31 + b["head_y"] + 2 + side_offsets(b)[pose]


def front_shoulder(b, sign):
    return CX + sign * (b["sw"] - 1), 31 + b["head_y"] + 2


def arm_front_pose(b, pose, sign, what):
    """정면 팔 한 짝, 동작 자세 — sign 1 = 화면 오른쪽(무기 팔), -1 = 왼쪽."""
    a, bend = FRONT_POSES[pose][0 if sign > 0 else 1]
    return _arm_lines(b, front_shoulder(b, sign), a, bend, what, sign=sign)


def arm_side_pose(b, pose, near, what):
    """옆모습 팔 한 짝, 한 자세 — what: skin · sleeve · hand. 다리와 반대로 흔든다."""
    a, bend = POSES[pose][2 if near else 3]
    return _arm_lines(b, side_shoulder(b, pose), a, bend, what, far=not near)


def _arm_lines(b, shoulder, a, bend, what, sign=1, far=False):
    elbow, wrist, (fx, fy) = _arm_geo(b, shoulder, a, bend, sign)
    near = not far
    w = b["aw"] - 1
    if what == "hand":
        centre = (wrist[0] + 1.5 * fx, wrist[1] + 1.5 * fy)
        out = [_ellipse_at(centre, b["aw"] + 1, max(4, b["aw"]), "S"), "    light skin 0 1 0"]
        ramp = "skin"
    else:
        c, ramp, lit = ("D", "cloth", "light cloth 0 2 1") if what == "sleeve" else ("S", "skin", "light skin 0 1 0")
        out = [f"    poly {c} {_quad(shoulder, elbow, w, w * 0.85 if FINE else None)}",
               f"    poly {c} {_quad(elbow, wrist, w * 0.85 if FINE else w, w * 0.7 if FINE else None)}",
               _ellipse_at(elbow, b["aw"], b["aw"], c), f"    {lit}"]
        if what == "sleeve":
            # 소매 끝단 — 손목에 크림 테
            nx, ny = fy * b["aw"] / 2, -fx * b["aw"] / 2
            out.append(f"    line {round(wrist[0] + nx)} {round(wrist[1] + ny)} "
                       f"{round(wrist[0] - nx)} {round(wrist[1] - ny)} d")
    if not near:
        out.append(f"    shade {ramp} 1")
    return out + ["    outline o"]


# ── 견본 스프라이트 전용 — 공격 클립의 칼, 시전 클립의 빛. 종이 인형 파츠에는 넣지 않는다(무기는 장비 몫) ──
SWORD = """part l_sword_up 7 22
    poly F 3 0 5 3 5 15 1 15 1 3
    light steel 0 1 1
    line 3 2 3 14 H
    rect 0 16 7 2 g
    rect 2 18 3 3 M
    px 3 21 g
    outline o
end
"""
SWORD_GRIP = (3, 18)


def weapon_pose(b, view, pose):
    """무기 팔 손목에 칼 — 아래팔 방향을 따라 칼날이 뻗는다 (stamp_rot)."""
    if view == "e":
        a, bend = POSES[pose][2]
        shoulder = side_shoulder(b, pose)
    else:
        a, bend = FRONT_POSES[pose][0]
        shoulder = front_shoulder(b, 1)
    _, wrist, _ = _arm_geo(b, shoulder, a, bend)
    # 위를 향한 칼을 아래팔 방향(a + bend)으로 — 시계 방향 180 - φ
    deg = round(180 - (a + bend))
    gx, gy = SWORD_GRIP
    return [f"    stamp_rot l_sword_up {round(wrist[0]) - gx} {round(wrist[1]) - gy} {deg} {gx} {gy}"]


def cast_fx(b, view, pose):
    """시전 빛 — 손끝 앞에 반투명 빛과 밝은 심, 반짝이. c1·c2 에만."""
    if pose not in ("c1", "c2"):
        return None
    if view == "e":
        a, bend = POSES[pose][2]
        _, wrist, (fx, fy) = _arm_geo(b, side_shoulder(b, pose), a, bend)
        c = (wrist[0] + 4 * fx, wrist[1] + 4 * fy)
    else:
        # 정면 — 두 손목 사이에서 조금 위 (몸 위에 두면 반투명 빛이 가려진다)
        wrists = [_arm_geo(b, front_shoulder(b, sign), *FRONT_POSES[pose][0 if sign > 0 else 1], sign)[1]
                  for sign in (1, -1)]
        c = ((wrists[0][0] + wrists[1][0]) / 2, min(w[1] for w in wrists) - 1)
    r = 5 if pose == "c2" else 4
    # 반투명 테(몸 밖에서만 보인다) + 불투명한 금빛 구슬과 밝은 심 + 반짝이
    return [_ellipse_at(c, 2 * r + 1, 2 * r + 1, "V"), _ellipse_at(c, 5, 5, "G"), _ellipse_at(c, 3, 3, "P"),
            f"    px {round(c[0]) - r} {round(c[1]) - 2} P", f"    px {round(c[0]) + r - 1} {round(c[1]) + 2} P",
            f"    px {round(c[0]) + 1} {round(c[1]) - r} P"]


# ── 머리 좌표(56×56) 겹 — 종족 특징·수염. head_y 만큼 내려 stamp ──
# 좌표는 머리 설계 좌표로 적고 hp() 가 머리 배율에 맞춰 옮긴다 (tools/sd_head.py to_out 과 같은 식).

def hp(b, *xy):
    s = b.get("head_scale", HEAD_SCALE)
    out = []
    for x, y in zip(xy[::2], xy[1::2]):
        out += [28 + (x + 0.5 - 28) * s - 0.5, 31 + (y + 0.5 - 31) * s - 0.5]
    return " ".join(_n(v) for v in out)


def beard(b, view):
    # 고해상도면 가닥 — 수염 끝 갈래를 향해 가는 짙은 결과 밝은 결
    strands = ([f"    line {hp(b, x0, 30, x1, 38)} {c}" for x0, x1, c in
                ((21, 23, "x"), (23.5, 25.5, "w"), (29.5, 29, "x"), (33, 31.5, "x"), (35, 33.5, "w"))]
               if b.get("res", 1) > 1 else [])
    if view == "s":
        return [f"    poly v {hp(b, 19, 28, 37, 28, 37, 33, 33, 40, 30, 37, 28, 42, 26, 37, 23, 40, 19, 33)}",
                "    light hair 0 2 1", f"    line {hp(b, 24, 31, 25, 37)} x", f"    line {hp(b, 32, 31, 31, 37)} x",
                *strands, f"    px {hp(b, 27, 30)} t", f"    px {hp(b, 28, 30)} t", "    outline o"]
    if view == "e":
        return [f"    poly v {hp(b, 30, 28, 39, 28, 40, 33, 37, 40, 34, 37, 31, 40, 29, 33)}", "    light hair 0 2 1",
                f"    line {hp(b, 33, 31, 34, 37)} x", "    outline o"]
    return None


def elf_ears(b, view):
    """뾰족 귀 — 피부. 옆머리를 뚫고 바깥 위로 길게."""
    if view in ("s", "n"):
        return [f"    poly S {hp(b, 14, 22, 6, 16, 8, 19, 14, 28)}", f"    poly S {hp(b, 41, 22, 49, 16, 47, 19, 41, 28)}",
                "    light skin 0 1 0", f"    line {hp(b, 12, 22, 9, 19)} T", f"    line {hp(b, 43, 22, 46, 19)} T",
                "    outline o"]
    return [f"    poly S {hp(b, 26, 23, 18, 17, 20, 21, 26, 28)}", "    light skin 0 1 0",
            f"    line {hp(b, 24, 23, 21, 20)} T", "    outline o"]


def cat_ears(b, view):
    """고양이 귀 — 머리색, 안쪽은 분홍(정면만)."""
    ears = [f"    poly v {hp(b, 14, 13, 17, 3, 23, 9)}", f"    poly v {hp(b, 33, 9, 39, 3, 42, 13)}",
            "    light hair 0 2 1"]
    if view == "s":
        return ears + [f"    poly b {hp(b, 16, 11, 17, 6, 20, 9)}", f"    poly b {hp(b, 36, 9, 39, 6, 40, 11)}",
                       "    outline o"]
    if view == "n":
        return ears + ["    outline o"]
    return [f"    poly v {hp(b, 18, 10, 22, 2, 28, 8)}", f"    poly v {hp(b, 26, 8, 31, 2, 33, 10)}",
            "    light hair 0 2 1", f"    poly b {hp(b, 28, 8, 31, 5, 31, 9)}", "    outline o"]


def horns(b, view):
    """뿔 — 상아색, 관자놀이에서 바깥 위로 휜다."""
    if view in ("s", "n"):
        return [f"    poly J {hp(b, 18, 12, 15, 7, 12, 2, 14, 1, 18, 5, 21, 10)}",
                f"    poly J {hp(b, 38, 12, 41, 7, 44, 2, 42, 1, 38, 5, 35, 10)}", "    light horn 0 2 1", "    outline o"]
    return [f"    poly J {hp(b, 24, 11, 20, 6, 16, 2, 18, 1, 23, 5, 27, 9)}", "    light horn 0 2 1", "    outline o"]


def tusks(b, view):
    """엄니 — 입꼬리에서 위로 두 칸. 머리가 작아도 픽셀 두 칸은 그대로."""
    pts = {"s": [(24, 30), (30, 30)], "e": [(37, 30)]}.get(view)
    if not pts:
        return None
    out = []
    for x, y in pts:
        ox, oy = (float(v) for v in hp(b, x, y).split())
        if FINE:
            # 고해상도 — 아래가 넓고 위가 뾰족한 엄니, 그늘 한 줄
            out += [f"    poly I {_n(ox - 0.5)} {_n(oy + 0.5)} {_n(ox + 0.8)} {_n(oy + 0.5)} {_n(ox + 0.3)} {_n(oy - 1.6)}",
                    f"    px {_n(ox + 0.5)} {_n(oy)} J"]
        else:
            out += [f"    px {round(ox)} {round(oy)} I", f"    px {round(ox)} {round(oy) - 1} I"]
    return out


def tail(b, view):
    """고양이 꼬리 — 허리에서 나와 휘어 오른다. 정면·옆면은 몸 뒤, 뒷면은 몸 앞."""
    _, _, belt, *_ = _y(b)
    w = belt + 1
    if view == "s":
        pts = [(CX + 3, w), (CX + 10, w - 3), (CX + 12, w - 9), (CX + 14, w - 10), (CX + 15, w - 7), (CX + 13, w - 1),
               (CX + 5, w + 3)]
    elif view == "n":
        pts = [(CX - 1, w), (CX + 2, w), (CX + 5, w + 4), (CX + 9, w + 4), (CX + 11, w + 1), (CX + 13, w + 2),
               (CX + 10, w + 8), (CX + 4, w + 8), (CX - 1, w + 3)]
    else:
        d = max(5, round(b["sw"] * 0.62))
        pts = [(CX - d, w), (CX - d - 6, w - 2), (CX - d - 9, w - 8), (CX - d - 11, w - 7), (CX - d - 9, w + 1),
               (CX - d, w + 3)]
    flat = " ".join(f"{x} {y}" for x, y in pts)
    return [f"    poly v {flat}", "    light hair 0 2 1", "    outline o"]


def layers(b, dress=None):
    """이 체형의 겹 전부 — 이름 → (종류, 연산 줄, 머리 좌표 part 인가)."""
    global FINE
    FINE = b.get("res", 1) > 1
    if dress is None:
        dress = b.get("skirt")
    L = {}

    def add(name, kind, lines, head=False):
        if lines:
            L[name] = (kind, lines, head)

    for side, sign in (("l", -1), ("r", 1)):
        k = "leg_" + side
        add(f"l_skin_leg_s{side}", k, leg(b, sign, "skin"))
        add(f"l_pants_s{side}", k, leg(b, sign, "pants"))
        add(f"l_boot_s{side}", k, leg(b, sign, "boot"))
        add(f"l_skin_arm_s{side}", "arm_" + side, arm_front(b, sign, cloth=False))
        add(f"l_sleeve_s{side}", "arm_" + side, arm_front(b, sign, cloth=True))
        add(f"l_hand_s{side}", "arm_" + side, hand_front(b, sign))
    add("l_skin_torso_s", "body", skin_torso(b, "s"))
    add("l_skin_torso_e", "body", skin_torso(b, "e"))
    add("l_top_s", "body", top_front(b, dress=dress))
    add("l_top_n", "body", top_front(b, back=True, dress=dress))
    add("l_top_e", "body", top_side(b, dress=dress))
    # 정면 팔 — 동작 자세(공격·시전·피격)만 골격. 대기·걷기는 위의 고정 겹
    for pose in FRONT_POSES:
        for side, sign in (("l", -1), ("r", 1)):
            for what, name in (("skin", "skin_arm"), ("sleeve", "sleeve"), ("hand", "hand")):
                add(f"l_{name}_s{side}_{pose}", "pose_arm", arm_front_pose(b, pose, sign, what))
    for pose in ("a0", "a1", "a2", "a3"):
        for view in "se":
            add(f"l_weapon_{view}_{pose}", "pose_arm", weapon_pose(b, view, pose))
    for pose in ("c0", "c1", "c2", "c3"):
        for view in "se":
            add(f"l_fx_{view}_{pose}", "pose_arm", cast_fx(b, view, pose))
    # 옆모습 팔다리 — 자세마다 겹 하나 (l_<재질>_e<n|f>_<자세>). 몸이 오르내리는 만큼은 자세에 들어 있다.
    for pose in POSES:
        for near, nf in ((True, "n"), (False, "f")):
            for what, name in (("skin", "skin_leg"), ("pants", "pants"), ("boot", "boot")):
                add(f"l_{name}_e{nf}_{pose}", "pose_leg", leg_side_pose(b, pose, near, what))
            for what, name in (("skin", "skin_arm"), ("sleeve", "sleeve"), ("hand", "hand")):
                add(f"l_{name}_e{nf}_{pose}", "pose_arm", arm_side_pose(b, pose, near, what))
    for v in "sne":
        if b.get("ears") == "elf":
            add(f"l_ear_{v}", "head", elf_ears(b, v), head=True)
        if b.get("ears") == "cat":
            add(f"l_catear_{v}", "head", cat_ears(b, v), head=True)
        if b.get("horns"):
            add(f"l_horn_{v}", "head", horns(b, v), head=True)
        if b.get("tusks"):
            add(f"l_tusk_{v}", "head", tusks(b, v), head=True)
        if b.get("beard"):
            add(f"l_beard_{v}", "head", beard(b, v), head=True)
        if b.get("tail"):
            add(f"l_tail_{v}", "tail", tail(b, v))
    if b.get("style") == "iso":
        iso_dress(b, L)
    if b.get("style") == "real":
        real_dress(b, L)
    # 일부 자세에만 있는 겹의 빈 자리
    L["l_empty"] = ("pose_arm", [], False)
    return L


# ── 아이소 화풍 (style: "iso") ──
# 참고 그림(reference/isolmetricRef*.jpg)에서 가져온 것은 **비율·명암·장비 구성**까지 — 특정 캐릭터는 옮기지 않는다.
# 셀 셰이딩(명암 단계를 줄임), 남: 가죽 조끼·쇠 견갑·가죽 장갑, 여: 강조색 스카프, 둘 다 허리 주머니.

def iso_dress(b, L):
    sex = b["sex"]
    # 셀 셰이딩 — 옷은 기본·그늘·하이라이트 세 단계
    for name, (kind, lines, head) in list(L.items()):
        L[name] = (kind, [ln.replace("light cloth 0 2 1", "light cloth 0 1 1") for ln in lines], head)
    sy, hem, belt, *_ = _y(b)
    sw, ww = b["sw"], b["ww"]
    d = max(5, round(sw * 0.62))
    waist = sy + round(b["tl"] * 0.45) if b.get("skirt") else belt
    # 허리 주머니 — 가죽, 덮개와 금 단추
    L["l_pouch_s"] = ("body", [f"    rect {CX + ww - 3} {waist + 2} 4 4 M", "    light leather 0 1 1",
                               f"    line {CX + ww - 3} {waist + 2} {CX + ww} {waist + 2} m",
                               f"    px {CX + ww - 2} {waist + 3} g", "    outline o"], False)
    L["l_pouch_e"] = ("body", [f"    rect {CX - d - 2} {waist + 2} 3 4 M", "    light leather 0 1 1",
                               f"    px {CX - d - 1} {waist + 3} g", "    outline o"], False)
    if sex == "m":
        # 가죽 조끼 — 정면은 앞이 열려 튜닉이 보이고, 뒷면은 등 전체, 옆면은 등쪽
        L["l_vest_s"] = ("body", [
            f"    poly M {CX - sw} {sy} {CX - 3} {sy + 1} {CX - 2} {belt - 1} {CX - ww - 1} {belt - 1} {CX - sw - 1} {sy + 6}",
            f"    poly M {CX + sw} {sy} {CX + 3} {sy + 1} {CX + 2} {belt - 1} {CX + ww + 1} {belt - 1} {CX + sw + 1} {sy + 6}",
            "    light leather 0 1 1",
            f"    line {CX - 3} {sy + 2} {CX - 2} {belt - 2} N", f"    line {CX + 3} {sy + 2} {CX + 2} {belt - 2} m",
            *[f"    px {CX - 3} {y} g" for y in range(sy + 4, belt - 1, 3)], "    outline o"], False)
        L["l_vest_n"] = ("body", [
            f"    poly M {CX - sw} {sy} {CX + sw} {sy} {CX + ww + 1} {belt - 1} {CX - ww - 1} {belt - 1}",
            "    light leather 0 1 1", f"    line {CX} {sy + 1} {CX} {belt - 2} m",
            f"    line {CX - sw + 2} {sy + 2} {CX - ww} {belt - 2} N", "    outline o"], False)
        L["l_vest_e"] = ("body", [
            f"    poly M {CX - d} {sy} {CX + d - 2} {sy} {CX + d - 3} {belt - 1} {CX - d - 1} {belt - 1}",
            "    light leather 0 1 1", f"    line {CX + d - 2} {sy + 1} {CX + d - 3} {belt - 2} N", "    outline o"], False)
        # 쇠 견갑 — 정면·뒷면은 두 어깨(팔과 함께 움직인다), 옆면은 가까운 어깨(자세마다)
        for side, sign in (("l", -1), ("r", 1)):
            x = CX + sign * (sw + 1)
            L[f"l_pauldron_s{side}"] = ("arm_" + side, [
                _ellipse_at((x, sy + 2), b["aw"] + 4, 6, "O"), "    light steel 0 1 1",
                f"    line {x - 2} {sy + 4} {x + 2} {sy + 4} U", f"    px {x} {sy + 1} g", "    outline o"], False)
        for pose in POSES:
            sx, sy_ = side_shoulder(b, pose)
            L[f"l_pauldron_en_{pose}"] = ("pose_arm", [
                _ellipse_at((sx, sy_), b["aw"] + 4, 6, "O"), "    light steel 0 1 1",
                f"    line {round(sx) - 2} {round(sy_) + 2} {round(sx) + 2} {round(sy_) + 2} U", "    outline o"], False)
        # 가죽 장갑 — 손 겹의 피부를 가죽으로
        swap = {" S": " M", "light skin 0 1 0": "light leather 0 1 1", "shade skin 1": "shade leather 1",
                " T": " m", " s": " N"}
        for name, (kind, lines, head) in list(L.items()):
            if name.startswith("l_hand_"):
                new = []
                for ln in lines:
                    for a, c in swap.items():
                        if ln.endswith(a) or ln.strip() == a.strip():
                            ln = ln[: len(ln) - len(a)] + c if ln.endswith(a) else c.join(ln.rsplit(a, 1))
                    new.append(ln)
                L[name] = (kind, new, head)
    else:
        # 스카프 — 목에 두르고 한 자락이 늘어진다. 강조색 (옆면은 뒤로 흩날린다)
        band = f"    poly 1 {CX - 5} {sy - 1} {CX + 5} {sy - 1} {CX + 4} {sy + 2} {CX - 4} {sy + 2}"
        L["l_scarf_s"] = ("body", [band, f"    poly 1 {CX - 4} {sy + 1} {CX - 1} {sy + 1} {CX - 3} {sy + 9} {CX - 6} {sy + 8}",
                                   "    light accent 0 1 1", f"    line {CX - 4} {sy + 3} {CX - 4} {sy + 7} 3", "    outline o"], False)
        # 뒷모습 — 꼬리는 가늘게, 어깨 쪽으로 비스듬히 늘어진다
        L["l_scarf_n"] = ("body", [band, f"    poly 1 {CX + 1} {sy + 1} {CX + 3} {sy + 1} {CX + 6} {sy + 8} {CX + 4} {sy + 9}",
                                   "    light accent 0 1 1", "    outline o"], False)
        L["l_scarf_e"] = ("body", [f"    poly 1 {CX - 4} {sy - 1} {CX + 3} {sy - 1} {CX + 3} {sy + 2} {CX - 4} {sy + 2}",
                                   f"    poly 1 {CX - 4} {sy} {CX - 2} {sy + 1} {CX - 8} {sy + 8} {CX - 10} {sy + 6}",
                                   "    light accent 0 1 1", "    outline o"], False)


# ── 사실 등신 화풍 (style: "real") ──
# 약 7등신 — 머리가 작고 얼굴이 사실적이다(작은 눈·코 그늘·턱선). 채도를 낮춘 흙빛·핏빛, 망토 깃.
# 몸은 다른 체형과 같은 생성기(골격 걷기·동작 클립), 머리는 큰 눈 머리 엔진 대신 real_head() 가 도형으로 그린다.

def real_dress(b, L):
    sy, hem, belt, *_ = _y(b)
    sw = b["sw"]
    d = max(5, round(sw * 0.62))
    # 망토 깃(울) — 어깨를 덮고 앞은 V 로 여민다. 금 걸쇠
    L["l_scarf_s"] = ("body", [
        f"    poly 2 {CX - sw - 1} {sy - 0.5} {CX + sw + 1} {sy - 0.5} {CX + sw + 2} {sy + 4.5} {CX + 3.5} {sy + 6.5} "
        f"{CX} {sy + 3.5} {CX - 3.5} {sy + 6.5} {CX - sw - 2} {sy + 4.5}",
        "    light accent 0 2 1",
        f"    line {CX - sw + 1} {sy + 2} {CX - 4} {sy + 4.5} 3", f"    line {CX + 4} {sy + 4.5} {CX + sw - 1} {sy + 2} 3",
        f"    line {CX - sw} {sy + 0.5} {CX - 3} {sy + 1} 1",
        f"    rect {CX - 1} {sy + 2.5} 2 1.5 g", f"    px {CX - 0.5} {sy + 2.5} G", "    outline o"], False)
    L["l_scarf_n"] = ("body", [
        f"    poly 2 {CX - sw - 1} {sy - 0.5} {CX + sw + 1} {sy - 0.5} {CX + sw + 2} {sy + 5} {CX} {sy + 8} "
        f"{CX - sw - 2} {sy + 5}",
        _ellipse_at((CX, sy + 0.5), 9, 3, "2"),
        "    light accent 0 2 1",
        f"    line {CX - 4} {sy + 3} {CX - 2} {sy + 6.5} 3", f"    line {CX + 3} {sy + 3} {CX + 2} {sy + 6.5} 3",
        "    outline o"], False)
    L["l_scarf_e"] = ("body", [
        f"    poly 2 {CX - d - 1.5} {sy - 0.5} {CX + d} {sy - 0.5} {CX + d + 0.5} {sy + 4} {CX - d - 2.5} {sy + 6.5}",
        "    light accent 0 2 1", f"    line {CX - d} {sy + 2} {CX + d - 1} {sy + 2.5} 3", "    outline o"], False)
    # 가죽 장갑
    swap = {" S": " M", "light skin 0 1 0": "light leather 0 1 1", " T": " m", " s": " N"}
    for name, (kind, lines, head) in list(L.items()):
        if name.startswith("l_hand_"):
            new = []
            for ln in lines:
                for a, c in swap.items():
                    if ln.endswith(a):
                        ln = ln[: len(ln) - len(a)] + c
                    elif ln.strip() == a.strip():
                        ln = ln.replace(a.strip(), c.strip())
                new.append(ln)
            L[name] = (kind, new, head)


def real_head(b):
    """사실 등신 머리 — 출력 좌표(112×112 part, res 2)에 직접. 목 아래 끝이 몸 어깨에 닿는다."""
    k = b.get("res", 1)
    assert k == 2, "real 화풍은 res 2 전용"
    W = 56 * k
    s_ = f"""part l_hair_behind_s {W} {W}
end
part l_hair_behind_e {W} {W}
end
part l_head_s {W} {W}
    rect 52 54 8 10 S
    ellipse 47 37 18 20 S
    poly S 48 47 64 47 63 53 59 58 53 58 49 53
    ellipse 45 45 3 6 S
    ellipse 64 45 3 6 S
    light skin 0 1 0
    line 53 58 59 58 T
    rect 52 59 8 2 T
    line 59 56 62 52 T
    px 46 47 T
    px 65 47 t
    // 머리 — 짧고 뒤로 넘긴 머리, 한 가닥이 이마로
    ellipse 46 33 20 12 v
    poly v 46 40 49 40 49 47 47 47
    poly v 63 40 66 40 66 47 64 47
    poly v 48 38 62 38 60 41 55 40 51 42 48 42
    poly v 51 41 53 41 52 45
    light hair 0 2 1
    line 49 37 57 35 w
    line 50 36 54 35 W
    line 58 36 63 39 x
    // 눈썹 · 눈 · 코 · 입
    line 50 45 53 44 X
    line 58 44 61 45 X
    px 51 47 t
    px 52 47 Y
    px 53 47 Q
    px 58 47 Q
    px 59 47 Y
    px 60 47 t
    line 51 48 53 48 T
    line 58 48 60 48 T
    px 57 49 T
    px 57 50 T
    px 56 51 t
    px 55 51 T
    line 54 54 58 54 t
    px 56 55 s
    outline o
end
part l_head_n {W} {W}
    rect 52 54 8 10 S
    ellipse 45 45 3 6 S
    ellipse 64 45 3 6 S
    ellipse 47 37 18 20 S
    light skin 0 1 0
    rect 52 58 8 3 T
    ellipse 46 33 20 22 v
    poly v 47 46 65 46 62 54 56 56 50 54
    light hair 0 2 1
    line 50 37 55 35 w
    line 56 40 56 53 x
    line 52 42 51 52 x
    line 60 42 61 52 x
    outline o
end
part l_head_e {W} {W}
    rect 50 54 8 10 S
    ellipse 46 37 18 20 S
    poly S 52 46 63 46 64 49 66 51 63 53 62 56 58 59 52 58
    light skin 0 1 0
    line 52 58 58 59 T
    rect 50 59 8 2 T
    line 55 57 58 58 T
    // 귀 · 머리 · 눈 · 코 · 입
    ellipse 51 44 4 7 S
    line 52 46 52 49 T
    px 53 45 s
    ellipse 44 33 21 13 v
    poly v 44 38 53 38 53 43 50 52 46 52 44 46
    poly v 57 37 64 39 63 42 59 41
    light hair 0 2 1
    line 47 37 56 35 w
    line 48 36 52 35 W
    line 46 44 49 51 x
    line 59 44 62 44 X
    px 61 47 Q
    px 60 47 Y
    line 60 48 62 48 T
    px 64 51 T
    px 63 52 t
    line 61 54 62 54 t
    outline o
end
"""
    return s_


# 방향별 겹 순서 (뒤 → 앞). 없는 겹은 건너뛴다. 머리 겹(l_head·l_hair_behind)은 원본의 「머리」 절에 있다.
ORDER = {
    "s": ["l_tail_s", "l_hair_behind_s", "l_skin_leg_sl", "l_skin_leg_sr", "l_pants_sl", "l_pants_sr",
          "l_boot_sl", "l_boot_sr", "l_skin_torso_s", "l_top_s", "l_vest_s", "l_pouch_s", "l_skin_arm_sl",
          "l_skin_arm_sr", "l_sleeve_sl", "l_sleeve_sr", "l_pauldron_sl", "l_pauldron_sr", "l_hand_sl", "l_hand_sr",
          "l_weapon_s#", "l_scarf_s", "l_head_s", "l_beard_s", "l_tusk_s",
          "l_ear_s", "l_catear_s", "l_horn_s", "l_fx_s#"],
    "n": ["l_skin_leg_sl", "l_skin_leg_sr", "l_pants_sl", "l_pants_sr", "l_boot_sl", "l_boot_sr",
          "l_skin_torso_s", "l_top_n", "l_vest_n", "l_skin_arm_sl", "l_skin_arm_sr", "l_sleeve_sl", "l_sleeve_sr",
          "l_pauldron_sl", "l_pauldron_sr", "l_hand_sl", "l_hand_sr", "l_head_n", "l_scarf_n", "l_ear_n", "l_catear_n", "l_horn_n", "l_tail_n"],
    # # 로 끝나는 이름은 자세 묶음 — 대기는 _i, 걷기는 프레임마다 _w0 … _w3
    "e": ["l_tail_e", "l_hair_behind_e", "l_skin_arm_ef#", "l_sleeve_ef#", "l_hand_ef#",
          "l_skin_leg_ef#", "l_pants_ef#", "l_boot_ef#", "l_skin_leg_en#", "l_pants_en#", "l_boot_en#",
          "l_skin_torso_e", "l_top_e", "l_vest_e", "l_pouch_e", "l_skin_arm_en#", "l_sleeve_en#",
          "l_pauldron_en#", "l_hand_en#", "l_weapon_e#", "l_scarf_e",
          "l_head_e", "l_beard_e", "l_tusk_e", "l_ear_e", "l_catear_e", "l_horn_e", "l_fx_e#"],
}
HEAD_LAYERS = {"l_head_s", "l_head_n", "l_head_e", "l_hair_behind_s", "l_hair_behind_e"}
# 정면·뒷면 팔 고정 겹 — 동작 클립에서는 같은 이름 + _<자세> 겹으로 바꿔 끼운다
FRONT_ARMS = {f"l_{n}_s{side}" for n in ("skin_arm", "sleeve", "hand") for side in "lr"}


def motion(b, view, clip, kind):
    """(dx 목록, dy 목록) — 프레임 수만큼. 머리 종류는 head_y 를 더한다."""
    hy, lift = b["head_y"], b["lift"]
    code = next((c[5] for c in CLIPS if c[0] == clip), None)
    if clip not in ("idle", "walk"):
        # 동작 클립 — 옆모습은 다리 자세에서 나온 오르내림, 정면·뒷면은 제자리
        n = next(c[2] for c in CLIPS if c[0] == clip)
        off = side_offsets(b)
        bob = [off[f"{code}{i}"] for i in range(n)] if view == "e" else [0] * n
        table = {k: (0, bob) for k in ("head", "body", "tail")}
        table.update({k: (0, 0) for k in ("arm_l", "arm_r", "leg_l", "leg_r")})
    elif clip == "idle":
        table = {"head": (0, [0, 1]), "body": (0, [0, 1]), "arm_l": (0, [0, 1]), "arm_r": (0, [0, 1]),
                 "leg_l": (0, 0), "leg_r": (0, 0), "tail": (0, [0, 1])}
    elif view in "sn":
        al, ar = ([1, 1, -1, 1], [-1, 1, 1, 1]) if view == "s" else ([-1, 1, 1, 1], [1, 1, -1, 1])
        table = {"head": (0, [0, 1, 0, 1]), "body": (0, [0, 1, 0, 1]), "arm_l": (0, al), "arm_r": (0, ar),
                 "leg_l": (0, [-lift, 0, 0, 0]), "leg_r": (0, [0, 0, -lift, 0]),
                 "tail": ([0, 1, 0, -1], [0, 1, 0, 1])}
    else:
        # 옆모습 걷기 — 몸 오르내림은 다리 자세에서 (가장 낮은 발이 땅에 닿게)
        off = side_offsets(b)
        bob = [off[f"w{i}"] for i in range(4)]
        table = {"head": (0, bob), "body": (0, bob), "tail": ([0, -1, 0, -1], [v + d for v, d in zip(bob, [1, 1, 1, 0])])}
    if view == "e" and clip == "idle":
        base = side_offsets(b)["i"]
        for k in ("head", "body", "tail"):
            table[k] = (0, [base, base + 1])
    dx, dy = table[kind]
    if kind == "head":
        dy = [hy + v for v in dy] if isinstance(dy, list) else hy + dy
    return dx, dy


def _val(v):
    return f"[{' '.join(map(str, v))}]" if isinstance(v, list) else str(v)


def frames_text(b, L, only=None, tail_sel=SEL):
    if b.get("res", 1) > 1:
        # 바깥 윤곽 두 겹 — 안쪽 겹은 selout 이 재질 색으로, 바깥 겹은 짙은 색 그대로. 안쪽 선은 1px
        tail_sel = "    outline o\n" + tail_sel
    """대기·걷기 frames 블록 전부. `only`(겹 이름 → 참/거짓)가 있으면 그 겹만 (종이 인형 파츠)."""
    keep = only or (lambda _: True)
    out = []
    for clip, _, n, _, _, code in CLIPS:
        if clip == "die":
            continue
        for view in "sne":
            out.append(f"frames {clip}_{view}_ {n}")
            for name in ORDER[view]:
                if clip not in ("idle", "walk") and name in FRONT_ARMS:
                    name += "#"
                if name.endswith("#"):
                    base = name[:-1]
                    if clip == "idle" and f"{base}_i" in L:
                        names = [f"{base}_i"] * n
                    else:
                        names = [f"{base}_{code}{i}" for i in range(n)]
                    present = [m in L and keep(m) for m in names]
                    if not any(present):
                        continue
                    # 일부 자세에만 있는 겹(시전 빛)은 나머지 프레임을 빈 겹으로
                    names = [m if ok else "l_empty" for m, ok in zip(names, present)]
                    # 팔은 대기에서 숨쉬기만큼 오르내리고, 다리는 땅을 딛고 있다
                    dy = "[0 1]" if clip == "idle" and L[names[0]][0] == "pose_arm" else "0"
                    if all(m == "l_empty" for m in names):
                        continue
                    ref = names[0] if clip == "idle" else f"[{' '.join(names)}]"
                    out.append(f"    stamp {ref} 0 {dy}")
                    continue
                if not keep(name):
                    continue
                if name in HEAD_LAYERS:
                    kind = "head"
                elif name in L:
                    kind = L[name][0]
                else:
                    continue
                dx, dy = motion(b, view, clip, kind)
                out.append(f"    stamp {name} {_val(dx)} {_val(dy)}")
            # 공격은 앞으로 내딛고 피격은 뒤로 밀린다 — 옆모습만, 몸 전체를
            shift = [POSE_DX.get(f"{code}{i}", 0) for i in range(n)]
            if view == "e" and any(shift):
                out.append(f"    shift {_val(shift)} 0")
            out.append(tail_sel.rstrip("\n"))
            out.append("end")
    out += die_frames(b)
    for clip, _, n, *_ in CLIPS:
        names = " ".join(f"{clip}_e_{i}" for i in range(n))
        out += [f"frames {clip}_w_ {n} = [{names}]", "    flip_h", "end"]
    return "\n".join(out) + "\n"


def die_frames(b):
    """사망 — 대기 첫 프레임을 발을 축으로 뒤로 넘어뜨린다 (stamp_rot). 다 눕히면 칸을 넘으니 몸 높이에 맞춰 멈춘다."""
    foot = b["h"] - 3
    tall = foot - (31 + b["head_y"] - round(26 * b.get("head_scale", HEAD_SCALE)))
    last = min(70, round(math.degrees(math.asin(min(1.0, (CX + 6) / tall)))))
    degs = [0, -round(last * 0.3), -round(last * 0.7), -last]
    out = []
    for view in "sne":
        out += [f"frames die_{view}_ 4", f"    stamp_rot idle_{view}_0 0 0 {_val(degs)} {CX} {foot}",
                "    shift [0 1 3 6] [0 0 1 1]", "end"]
    return out


def layer_defs(b, L):
    out = []
    for name, (_, lines, head) in L.items():
        out.append(f"part {name} 56 56" if head else f"frame {name}")
        out.extend(lines)
        out.append("end")
    return "\n".join(out) + "\n"


def sheet_block(tinted):
    """시트 정의 — CLIPS 순서로 클립마다 방향 4행 (동·북·서·남)."""
    out = ["", "sheet", "    directions 4", "    pixels_per_meter 16", f"    tinted {tinted}"]
    for clip, state, n, ms, mode, _ in CLIPS:
        out.append(f"    clip {state} {ms} {mode}")
        for view in "enws":
            out.append("        row " + " ".join(f"{clip}_{view}_{i}" for i in range(n)))
        out.append("    end")
    return "\n".join(out + ["end", ""])


def palette(b, colors):
    skin = b.get("skin", "#ffd9c4")
    iso = b.get("style") == "iso"
    real = b.get("style") == "real"
    accent = (f"    ramp accent 1234 from {colors['accent']} at 1 step 0.12 hue 8\n" if iso else
              f"    ramp accent 1234 from {colors['accent']} at 1 step 0.13 hue 10\n" if real else "")
    return f"""palette
    o {"#2a1a12" if iso else "#16100e" if real else "#1c1626"}
{accent}
    ramp hair WwvuxX from {colors['hair']} at 2 step 0.12 hue 14
    ramp skin sSTt from {skin} at 1 step 0.1
    ramp eye RrqQ from {colors['eye']} at 1 step 0.15 hue 8
    K #2a1420
    b #ff9aa6
    Y #fffaf0
    ramp cloth EDCBA from {colors['cloth']} at 2 step 0.11 hue 8
    ramp pants Lljz from {colors['pants']} at 1 step 0.09 hue 6
    ramp leather NMmn from #8a5a38 at 1 step 0.12 hue 6
    ramp gold Ggyh from #e8b040 at 1 step 0.13 hue 8
    ramp cream cdef from #efe4cc at 1 step 0.1
    ramp horn IiJZ from #e8dcc4 at 1 step 0.12 hue 8
    ramp steel FHOU from #c4ccdc at 1 step 0.14 hue 6
    P #fff6c8
    V #ffe8805a
end
""".replace("\n\n", "\n")


SEL_ISO = SEL.replace(" FH HO\n", " FH HO 12 23\n").replace(" FU HU OU\n", " FU HU OU 14 24 34\n")


def canvas(name, b):
    sex = b["sex"]
    iso = b.get("style") == "iso"
    real = b.get("style") == "real"
    return (f"""canvas 1
cell 56 {b['h']}

// 플레이어 템플릿 — {b['label']}. tools/sd_template.py 로 생성 (체형 수치는 그 파일 BODIES["{name}"]).
// 머리는 tools/template_{sex}_head.py, 몸은 체형 수치로. 손으로 고치면 다시 생성할 때 덮인다.
// 팔레트 문자는 모든 템플릿이 같다 — ramp … from #색 의 기준색만 바꾸면 변형:
//   머리 WwvuxX · 피부 sSTt · 눈 RrqQ K Y · 볼 b · 윗옷 EDCBA · 아랫옷 Lljz · 가죽 NMmn · 금 Ggyh
//   크림 테 cdef · 뿔·엄니 IiJZ

""" + palette(b, (COLORS_ISO if iso else COLORS_REAL if real else COLORS)[sex]) + f"""
// ── 머리 — tools/template_{sex}_head.py 로 생성 (규칙은 tools/sd_head.py 머리말) ──
// ── 머리 끝 ──

""" + SWORD + layer_defs(b, layers(b)) + frames_text(b, layers(b), tail_sel=SEL_ISO if iso or real else SEL)
            + sheet_block("false") + f"""
output
    image ../../WorldEditor/assets/worldcanvas/template_{name}.png
    sheet ../../WorldEditor/assets/worldcanvas/template_{name}.sheet.ron
end
""")


# ── 해상도 배율 — 1배로 만든 원본 텍스트의 좌표 인자에 배율을 곱한다 ──
# 연산 → 배율을 곱할 인자 자리 (연산 이름 다음부터 0번). "*" 는 그 자리부터 끝까지.
SCALED_ARGS = {
    "cell": (0, 1), "px": (0, 1), "rect": (0, 1, 2, 3), "box": (0, 1, 2, 3), "line": (0, 1, 2, 3),
    "fill": (0, 1), "ellipse": (0, 1, 2, 3), "poly": ("*", 1), "stamp": (1, 2), "stamp_rot": (1, 2, 4, 5),
    "shift": (0, 1), "shade": (2, 3, 4, 5), "dither": (2, 3, 4, 5), "pixels_per_meter": (0,),
    "swap": (2, 3, 4, 5), "move": (0, 1, 2, 3, 4, 5), "pattern": (1, 2),
}


def upscale(text, k):
    """원본 텍스트의 좌표를 k 배로. 머리 grid 는 손대지 않는다(머리 엔진이 고해상도로 따로 칠한다)."""
    import re
    token = re.compile(r"\[[^\]]*\]|\S+")

    def mul(t):
        if t.startswith("["):
            return "[" + " ".join(mul(x) for x in t[1:-1].split()) + "]"
        try:
            return str(int(t) * k)
        except ValueError:
            pass
        try:
            return str(round(float(t) * k))
        except ValueError:
            return t

    out = []
    for line in text.split("\n"):
        body = line.split("//", 1)[0]
        toks = token.findall(body)
        if not toks:
            out.append(line)
            continue
        op, args = toks[0], toks[1:]
        if op == "part" and len(args) >= 3:
            args[1], args[2] = mul(args[1]), mul(args[2])
        elif op == "light" and len(args) >= 2 and args[1] != "0":
            args[1] = mul(args[1])
        elif op in SCALED_ARGS:
            spec = SCALED_ARGS[op]
            if spec[0] == "*":
                args = args[:spec[1]] + [mul(a) for a in args[spec[1]:]]
            else:
                args = [mul(a) if i in spec else a for i, a in enumerate(args)]
        else:
            out.append(line)
            continue
        indent = line[: len(line) - len(line.lstrip())]
        out.append(indent + " ".join([op, *args]))
    return "\n".join(out)


def head_args(name, b, path):
    args = [sys.executable, f"tools/template_{b['sex']}_head.py", "--write", f"--canvas={path}", "--part",
            f"--scale={b.get('head_scale', HEAD_SCALE)}", f"--res={b.get('res', 1)}"]
    if "hair_end" in b:
        args.append(f"--hair-end={b['hair_end']}")
    if "hair" in b:
        args.append(f"--style={b['hair']}")  # 머리 모양 라이브러리 (tools/hair_styles.py)
    return args


def body_text(name, b):
    """원본 텍스트(해상도 배율 적용). 사실 등신 화풍은 머리까지 — 나머지는 머리 엔진이 「머리」 절을 채운다."""
    text = canvas(name, b)
    if b.get("res", 1) > 1:
        text = upscale(text, b["res"])
    if b.get("style") == "real":
        mark = "// ── 머리 끝 ──"
        text = text.replace(mark, real_head(b) + mark, 1)
    return text


def uses_head_engine(b):
    return b.get("style") != "real"


def main():
    names = [a for a in sys.argv[1:] if not a.startswith("-")] or list(BODIES)
    for name in names:
        b = BODIES[name]
        path = f"sprites/template_{name}.canvas"
        open(path, "w").write(body_text(name, b))
        # 머리는 56×56 으로 칠한다 — part 로 받아 head_y 만큼 내려 얹는다
        if uses_head_engine(b):
            subprocess.run(head_args(name, b, path), check=True)
        print(path)


if __name__ == "__main__":
    main()
