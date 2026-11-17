"""SD2 종이 인형 — sd_template 체형을 에디터 캐릭터 만들기용 몸·파츠 시트로 내보낸다.

    python3 tools/sd_paperdoll.py            # PAPERDOLL 전부 → WorldEditor/assets/worldcanvas/paperdoll/sd2_<체형>/
    python3 tools/sd_paperdoll.py elf_f      # 하나만

tools/paperdoll.py 의 종이 인형 체계와 같은 형식이다 (그쪽 머리말·WorldEditor `appearance.rs`):
템플릿 폴더에 `body.png` + 파츠별 시트, `catalog.ron` 이 파츠(분류·색 묶음·tinted·장비 자리)를 적고,
공용 `paperdoll/catalog.ron` 의 템플릿 목록에 `sd2_*` 를 더한다 (paperdoll.py 가 다시 써도 남는다).

무엇을 어떻게:
- 겹 정의는 tools/sd_template.py 와 **같다** — 체형을 고치면 완성 스프라이트와 파츠가 함께 바뀐다.
  파츠마다 그 겹만 남긴 `.canvas` 를 임시로 만들어 `worldcanvas build` 로 그린다.
- 해상도는 완성 스프라이트와 같다 (생성 체형 기본 res 2 = 32px/m, 칸 112×…). 머리 grid 는 머리 엔진이 112×112 에
  직접 칠하고(눈은 scaled_eyes), 나머지 겹은 1배로 쓴 뒤 sd_template.upscale. `--res=1` 이면 예전 16px/m.
- 색을 고를 파츠는 그 파츠의 **재질 램프 하나만** 회색 단계로 그리고 시트에 `ramp: true` 를 적는다. 회색 단계는
  완성 스프라이트 팔레트에서 잰다(grey_palette) — 기준 단계가 PIVOT 회색 = 고른 색, 더 밝은 단계는 하이라이트.
  나머지 재질(허리띠 가죽·금 단추·크림 테·외곽선·흰자)은 원래 색 그대로 — 에디터(WorldEditor `ramp_tint.rs`)는
  회색이 아닌 픽셀을 칠하지 않는다. 템플릿 기본색을 고르면 완성 스프라이트와 거의 같은 그림이 된다.
- 고해상도 바깥 윤곽(`outline o` 한 겹 더)은 옷 파츠에만. 머리·눈·입·손은 늘 몸 안쪽에 붙어 있어서 따로 두르면
  얼굴 구멍 둘레·옆머리 아랫단·손 둘레에 완성 스프라이트에 없는 선이 생긴다.
- 얼굴(두피 포함)은 `body_part` 파츠 `head` 로 옷깃·손 앞(층 6)에 — 완성 스프라이트처럼 턱이 윗옷을 덮는다.
  몸 시트에도 얼굴을 남긴다(빼면 몸 윤곽이 목 위를 둘러 V 깃 사이로 보인다).
- 키 이름은 paperdoll.py 와 맞춘다(eyes_round · face_smile · hair_short · top_tunic …) — 템플릿을 바꿔도 고른
  파츠가 이어진다.
- 손은 피부색을 따르는 `body_part` 파츠(hands)다 — 늘 붙고 특징 문구에는 안 나온다. 옆모습에서 손이 윗옷 앞에 와야 하는데 몸(0)은 윗옷(4) 아래다.
"""
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import hair_styles  # noqa: E402
import sd_head  # noqa: E402
import sd_template as T  # noqa: E402
import template_f_head as HF  # noqa: E402
import template_m_head as HM  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT.parent / "WorldEditor" / "assets" / "worldcanvas" / "paperdoll"
# 실행 파일 — 검사(cargo test)는 WORLDCANVAS_BIN 으로 방금 빌드한 것을 넘긴다
# Windows 는 실행 파일에 .exe 가 붙는다.
WC = Path(os.environ.get("WORLDCANVAS_BIN") or ROOT / "target" / "release" / ("worldcanvas.exe" if os.name == "nt" else "worldcanvas"))

PAPERDOLL = ["std_m", "std_f", "tall_m", "tall_f", "small_m", "small_f", "dwarf_m", "dwarf_f", "big_m",
             "elf_m", "elf_f", "beast_m", "beast_f", "demon_m", "demon_f", "orc_m"]

# 램프로 칠할 파츠의 회색 단계 — 손으로 정하지 않고 완성 스프라이트의 팔레트에서 잰다(grey_palette).
# 기준 단계(`ramp … at N`)가 PIVOT 회색 = 에디터에서 고른 색 그대로. 더 밝은 단계는 흰색까지 남은 명도의
# 비율로, 더 어두운 단계는 기준과의 밝기(선형 휘도) 비로 — WorldEditor `ramp_tint.rs` 의 역산.
# 그래서 템플릿 기본색을 고르면 완성 스프라이트와 거의 같은 색이 나온다.
# 램프가 아닌 색(외곽선 o·속눈썹 K·흰자 Y·광채)은 원래 색 그대로 — 회색이 아니면 에디터가 칠하지 않는다.
PIVOT = 0xB4  # WorldEditor ramp_tint::PIVOT 과 같아야 한다


def _lin(c):
    c /= 255
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def _srgb(v):
    v = min(1.0, max(0.0, v))
    e = v * 12.92 if v <= 0.0031308 else 1.055 * v ** (1 / 2.4) - 0.055
    return round(e * 255)


def _rgb(h):
    return [int(h[i:i + 2], 16) for i in (1, 3, 5)]


def _okl(rgb):
    r, g, b = map(_lin, rgb)
    l_ = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b) ** (1 / 3)
    m_ = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b) ** (1 / 3)
    s_ = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b) ** (1 / 3)
    return 0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_


def _lum(rgb):
    r, g, b = map(_lin, rgb)
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def grey_palette(b, tinted):
    """체형 b 의 완성 스프라이트 팔레트(sd_template.palette)에서 램프 `tinted` 만 회색 단계로 바꾼 팔레트 블록.

    나머지 재질(윗옷의 가죽 허리띠·금 단추·크림 테 …)은 원래 색 그대로 — 에디터가 칠하지 않는다.
    """
    import re
    src = T.palette(b, T.COLORS[b["sex"]])
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "p.canvas"
        path.write_text("canvas 1\n" + src)
        listing = subprocess.run([str(WC), "list", str(path)], capture_output=True, text=True, check=True).stdout
    colour = dict(re.findall(r"^  (\S) (#[0-9a-f]{6,8})$", listing, re.M))
    ramps = re.findall(r"ramp (\w+) (\S+) from \S+ at (\d+)", src)
    pivot_l, pivot_lin = _okl([PIVOT] * 3), _lin(PIVOT)
    out = ["palette"]
    in_ramp = set()
    for name, keys, at in ramps:
        if name != tinted:
            continue
        base = _rgb(colour[keys[int(at)]])
        lb, yb = _okl(base), _lum(base)
        for i, k in enumerate(keys):
            c = _rgb(colour[k])
            if i < int(at):  # 하이라이트
                t = max(0.0, min(1.0, (_okl(c) - lb) / max(1e-6, 1 - lb)))
                grey = _srgb((pivot_l + (1 - pivot_l) * t) ** 3)
            else:
                grey = _srgb(pivot_lin * _lum(c) / yb)
            out.append(f"    {k} #{grey:02x}{grey:02x}{grey:02x}")
        out.append(f"    ramp {name} {keys}")
        in_ramp.update(keys)
    for k, v in colour.items():
        if k in in_ramp:
            continue
        rgb = _rgb(v)
        if max(rgb) - min(rgb) <= 4:  # 회색에 가까우면 에디터가 칠한다 — 한 칸 비켜 둔다
            rgb[2] = min(255, rgb[2] + 6) if rgb[2] < 249 else rgb[2] - 6
            v = "#" + "".join(f"{c:02x}" for c in rgb) + v[7:]
        out.append(f"    {k} {v}")
    return "\n".join(out + ["end", ""])


def side(front, back=None):
    """방향별 앞뒤 `[동, 북, 서, 남]` — paperdoll.py 의 layers() 와 같다."""
    back = front if back is None else back
    return [front, back, front, front]


# 앞뒤는 완성 스프라이트의 그리는 순서(sd_template.ORDER)를 따른다 — 몸 < 아랫옷 < 신발 < 윗옷 < 손 < 얼굴
# < 눈·입 < 머리카락 < 수염 < 귀 < 뿔. 얼굴이 손·옷깃을 덮는다 (SD 체형은 큰 머리가 가슴 앞 손을 가린다).
L_HAIR_BACK = side(-2, back=9)
L_TAIL = side(-1, back=13)
L_BOTTOM = side(2)
L_SHOES = side(3)
L_TOP = side(4)
L_HANDS = side(5)
L_HEAD = side(6)
L_FACE = side(7)
L_HAIR = side(9)
L_FACIAL = side(10)
L_EARS = side(11)
L_HORNS = side(12)


def grid_part(name, g):
    # 크기는 1배 좌표로 적는다 — part_canvas 의 upscale 이 해상도를 곱한다 (grid 줄은 건드리지 않는다)
    return "\n".join([f"part {name} 56 56", "    grid", *("        " + "".join(r) for r in g), "    end", "end"])


def head_grids(b):
    """체형 b 에 쓸 머리 겹들 — 피부(두피+얼굴), 머리카락 둘(짧은/긴), 눈 둘, 입·볼."""
    HF.HAIR_END = b.get("hair_end", 46)
    # 완성 스프라이트(sd_template)와 같은 머리 — 배율·가닥 결·작은 눈
    sd_head.SCALE = b.get("head_scale", T.HEAD_SCALE)
    sd_head.STRANDS = True
    # 해상도 — 2 면 머리를 112×112 에 직접 칠한다 (나머지 겹은 part_canvas 가 upscale)
    sd_head.RES = b.get("res", 1)
    sd_head.W_ = sd_head.H_ = 56 * sd_head.RES
    small = sd_head.SCALE < 0.9
    skin_views = {v.name: v for v in (HM.VIEWS if b["sex"] == "m" else HF.VIEWS)}
    out = {"skin": {v: sd_head.head_skin(skin_views[v]) for v in "sne"}}
    end = b.get("hair_end", 46)
    styles = [("short", HM.VIEWS), ("long", HF.VIEWS)]
    styles += [(k, hair_styles.views(k, b["sex"], end)) for k in hair_styles.STYLES]
    for key, views in styles:
        front, behind = {}, {}
        for v in views:
            bh, g = sd_head.paint(v, ahoge_on=False, eyes_s=(), eye_e=None, blush=False, mouth=False)
            front[v.name] = sd_head.hair_only(g)
            if v.name != "n":
                behind[v.name] = sd_head.hair_only(bh)
        out[f"hair_{key}"] = front
        # 몸 뒤로 내려오는 머리가 있는 모양만 뒷머리 파츠 (긴 머리·포니테일·트윈테일)
        if any(c != "." for g in behind.values() for r in g for c in r):
            out[f"hairback_{key}"] = behind
    for key, mod in (("round", HF), ("sharp", HM)):
        if sd_head.RES >= 2:
            # 고해상도 눈은 머리 배율에 맞춰 그린다 (완성 스프라이트와 같은 눈)
            eyes_s, eye_e = sd_head.scaled_eyes(mod.HIRES)
        else:
            eyes_s = mod.SMALL["eyes_s"] if small else mod.EYES_S
            eye_e = mod.SMALL["eye_e"] if small else mod.EYE_E
        eyes = {}
        for v in "se":
            g = sd_head.blank()
            for rows, x0, y0 in (eyes_s if v == "s" else [eye_e]):
                sd_head.stamp(g, rows, *sd_head.to_out(x0, y0))
            eyes[v] = g
        out[f"eyes_{key}"] = eyes
    for key, blush in (("smile", False), ("blush", True)):
        face = {}
        for v, mouth, cheeks in (("s", sd_head.MOUTH_S, sd_head.BLUSH_S), ("e", sd_head.MOUTH_E, sd_head.BLUSH_E)):
            g = sd_head.blank()
            sd_head.face_marks(g, cheeks if blush else [], mouth)
            face[v] = g
        out[f"face_{key}"] = face
    return out


def picks(part):
    """파츠가 남길 겹인가 — 겹 이름이 파츠 목록의 어느 것과 같거나 그것으로 시작하면."""
    return lambda name: any(name == p or name.startswith(p) for p in part.layers)


class Part:
    def __init__(self, key, name, category, layers, order, tinted=True, palette="cloth", slot=None, heads=None,
                 dress=None, colour=False, ramp=None):
        self.key, self.name, self.category = key, name, category
        self.layers = tuple(layers)  # 남길 겹 — 이름 또는 접두사 (자세별 겹 l_pants_en_w0 … 을 한 번에)
        self.order = order  # 방향별 앞뒤
        self.tinted, self.palette, self.slot = tinted, palette, slot
        self.heads = heads or {}  # 머리 겹 이름 → 56×56 grid
        self.dress = dress  # 윗옷 원피스 치마 퍼짐
        self.colour = colour  # 곱하지 않는 파츠 — 색 그대로의 팔레트
        # 고른 색으로 칠할 재질 램프 (sd_template.palette 의 ramp 이름) — 나머지 재질은 색 그대로
        self.ramp = ramp or {"skin": "skin", "hair": "hair", "eyes": "eye", "horn": "horn"}.get(palette, "cloth")


def parts_for(b, H):
    S, E, N = "s", "e", "n"
    heads = lambda grids, behind=False: {  # noqa: E731
        (f"l_hair_behind_{v}" if behind else f"l_head_{v}"): g for v, g in grids.items()}
    parts = [
        # 먼 쪽 손(옆모습)은 몸 뒤에 있어야 하므로 몸(층 0)에 — 손 파츠(층 5)는 윗옷 앞에 온다
        # 얼굴도 몸에 남긴다 — 빼면 몸 윤곽이 목 위를 둘러 V 깃 사이로 보인다. 얼굴 파츠가 같은 자리를 덮는다
        Part("body", "몸", "body", ["l_skin_", "l_hand_ef_", "l_head_s", "l_head_n", "l_head_e"], None,
             palette="skin", heads=heads(H["skin"])),
        Part("hands", "손", "body_part", ["l_hand_sl", "l_hand_sr", "l_hand_en_"], L_HANDS, palette="skin"),
        # 얼굴(두피 포함)은 옷깃·손 앞 — 완성 스프라이트처럼 턱이 윗옷 위에 온다
        Part("head", "얼굴", "body_part", ["l_head_s", "l_head_n", "l_head_e"], L_HEAD, palette="skin",
             heads=heads(H["skin"])),
        Part("eyes_round", "눈 — 동그란 큰 눈", "eyes", ["l_head_s", "l_head_e"], L_FACE, palette="eyes",
             heads=heads(H["eyes_round"])),
        Part("eyes_sharp", "눈 — 가는 눈", "eyes", ["l_head_s", "l_head_e"], L_FACE, palette="eyes",
             heads=heads(H["eyes_sharp"])),
        Part("face_smile", "입 — 작은 입", "face", ["l_head_s", "l_head_e"], L_FACE, tinted=False,
             heads=heads(H["face_smile"]), colour=True),
        Part("face_blush", "입 — 볼 홍조", "face", ["l_head_s", "l_head_e"], L_FACE, tinted=False,
             heads=heads(H["face_blush"]), colour=True),
        Part("hair_short", "짧은 머리", "hair", ["l_head_s", "l_head_n", "l_head_e"], L_HAIR, palette="hair",
             heads=heads(H["hair_short"])),
        Part("hair_long", "긴 물결 머리", "hair", ["l_head_s", "l_head_n", "l_head_e"], L_HAIR, palette="hair",
             heads=heads(H["hair_long"])),
        Part("hairback_long", "긴 머리 (뒤)", "hair_back", ["l_hair_behind_s", "l_hair_behind_e"], L_HAIR_BACK,
             palette="hair", heads=heads(H["hairback_long"], behind=True)),
        # 머리 모양 라이브러리 (tools/hair_styles.py) — 키는 paperdoll.py 와 같다
        *[Part(f"hair_{k}", name, "hair", ["l_head_s", "l_head_n", "l_head_e"], L_HAIR, palette="hair",
               heads=heads(H[f"hair_{k}"])) for k, name in hair_styles.STYLES.items()],
        *[Part(f"hairback_{k}", f"{name} (뒤)", "hair_back", ["l_hair_behind_s", "l_hair_behind_e"], L_HAIR_BACK,
               palette="hair", heads=heads(H[f"hairback_{k}"], behind=True))
          for k, name in hair_styles.STYLES.items() if f"hairback_{k}" in H],
        Part("top_tunic", "튜닉", "top", ["l_top_", "l_sleeve_"], L_TOP, slot="Body", dress=0),
        Part("top_dress", "원피스", "top", ["l_top_", "l_sleeve_"], L_TOP, slot="Body",
             dress=b.get("skirt") or b["ww"] + 5),
        Part("bottom_pants", "긴바지", "bottom", ["l_pants_"], L_BOTTOM, ramp="pants"),
        Part("shoes_boots", "부츠", "shoes", ["l_boot_"], L_SHOES, slot="Shoes", ramp="leather"),
        Part("beard_full", "턱수염", "facial", ["l_beard_s", "l_beard_e"], L_FACIAL, palette="hair"),
    ]
    if b.get("ears") == "elf":
        parts.append(Part("ears_elf", "엘프 귀", "feature", [f"l_ear_{v}" for v in (S, N, E)], L_EARS,
                          palette="skin"))
    if b.get("ears") == "cat":
        parts.append(Part("ears_cat", "고양이 귀", "feature", [f"l_catear_{v}" for v in (S, N, E)], L_EARS,
                          palette="hair"))
    if b.get("tail") == "cat":
        parts.append(Part("tail_cat", "고양이 꼬리", "feature", [f"l_tail_{v}" for v in (S, N, E)], L_TAIL,
                          palette="hair"))
    if b.get("horns"):
        parts.append(Part("horns", "뿔", "feature", [f"l_horn_{v}" for v in (S, N, E)], L_HORNS, palette="horn"))
    if b.get("tusks"):
        parts.append(Part("tusks", "엄니", "feature", ["l_tusk_s", "l_tusk_e"], L_FACIAL, tinted=False,
                          colour=True))
    return parts


def part_canvas(name, b, p):
    # 수염은 모든 체형이 고를 수 있다 — 겹은 늘 만든다
    full = dict(b, beard=True)
    L = T.layers(full, dress=p.dress if p.dress is not None else b.get("skirt"))
    pick = picks(p)
    keep = {k: v for k, v in L.items() if pick(k) or k == "l_empty"}
    pal = T.palette(b, T.COLORS[b["sex"]]) if p.colour else grey_palette(b, p.ramp)
    defs = T.layer_defs(b, keep) + "\n".join(grid_part(n, g) for n, g in p.heads.items()) + "\n"
    frames = T.frames_text(full, L, only=pick)
    if (p.heads and p.key != "body") or p.key == "hands":
        # 머리 그림(머리카락·눈·입)은 머리 엔진이 윤곽까지 그렸다. 고해상도 바깥 윤곽을 한 겹 더 두르면
        # 몸에 붙는 자리(얼굴 구멍 둘레·옆머리 아랫단)에도 선이 생긴다 — 완성 스프라이트에서는 안 보이는 선.
        # 손도 늘 몸·옷 안쪽에 있어 바깥 윤곽이 없다 (겹 자기 윤곽만 — selout 이 피부 그늘색으로 바꾼다)
        frames = frames.replace("    outline o\n", "")
    text = (f"canvas 1\ncell 56 {b['h']}\n\n" + pal + "\n" + defs + frames +
            T.sheet_block("true" if p.tinted else "false") +
            f"\noutput\n    image {p.key}.png\n    sheet {p.key}.sheet.ron\nend\n")
    res = b.get("res", 1)
    return T.upscale(text, res) if res > 1 else text


def sheet_ron(b, image, tinted, order):
    res = b.get("res", 1)
    lines = ["// tools/sd_paperdoll.py 가 만든 파일 — 직접 고치지 말고 생성기를 고친 뒤 다시 만든다.",
             "//",
             "// 배치: 행 = 클립 시작 행 + 방향, 열 = 프레임. 방향 0 은 +X(동)에서 반시계로 증가.",
             "(", "    version: 1,", f'    image: "{image}",', f"    cell: ({56 * res}, {b['h'] * res}),",
             "    directions: 4,", f"    pixels_per_meter: {16 * res}.0,", f"    tinted: {'true' if tinted else 'false'},"]
    if tinted:
        # 곱하지 말고 램프로 칠하라 — 그늘에서 색상이 돈다 (WorldEditor ramp_tint.rs)
        lines.append("    ramp: true,")
    if order is not None:
        if len(set(order)) == 1:
            lines.append(f"    layer: {order[0]},")
        else:
            lines.append(f"    layer_by_direction: [{', '.join(map(str, order))}],")
    lines.append("    clips: [")
    for i, (_, state, n, ms, mode, _) in enumerate(T.CLIPS):
        looping = "true" if mode == "loop" else "false"
        lines.append(f"        (state: {state}, row: {i * 4}, frames: {n}, frame_ms: {ms}, looping: {looping}),")
    lines += ["    ],", ")", ""]
    return "\n".join(lines)


def ron_str(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def build(name):
    # 완성 스프라이트와 같은 해상도 (생성 체형 기본 res 2 = 32px/m). `--res=1` 이면 예전 16px/m
    b = dict(T.BODIES[name])
    for a in sys.argv[1:]:
        if a.startswith("--res="):
            b["res"] = int(a.split("=", 1)[1])
    key = f"sd2_{name}"
    folder = OUT / key
    folder.mkdir(parents=True, exist_ok=True)
    H = head_grids(b)
    parts = parts_for(b, H)
    with tempfile.TemporaryDirectory() as tmp:
        for p in parts:
            src = Path(tmp) / f"{p.key}.canvas"
            src.write_text(part_canvas(name, b, p))
            r = subprocess.run([str(WC), "build", str(src), "--out", tmp], capture_output=True, text=True)
            if r.returncode:
                sys.exit(f"{key}/{p.key}: {r.stderr}")
            image = "body.png" if p.key == "body" else f"{p.key}.png"
            shutil.copy(Path(tmp) / f"{p.key}.png", folder / image)
            (folder / image.replace(".png", ".sheet.ron")).write_text(sheet_ron(b, image, p.tinted, p.order))
    cat = ["// tools/sd_paperdoll.py 가 만든 템플릿 카탈로그 — 이 체형에 맞는 파츠 목록.",
           "// sheet 경로는 이 파일 폴더 기준. slot 은 그 자리에 외형이 있는 장비를 입으면 빠지는 파츠.",
           "(", "    version: 1,", f"    key: {ron_str(key)},", f"    name: {ron_str('SD2 · ' + b['label'])}",
           f"    reference: {ron_str('style/sheet_style.png 화풍 — ' + b['label'])},",
           '    body: "body.sheet.ron",', "    parts: ["]
    cat[5] += ","
    for p in parts:
        if p.key == "body":
            continue
        slot = f"Some({p.slot})" if p.slot else "None"
        cat.append(f"        (key: {ron_str(p.key)}, name: {ron_str(p.name)}, category: {ron_str(p.category)}, "
                   f"palette: {ron_str(p.palette)}, tinted: {'true' if p.tinted else 'false'}, slot: {slot}),")
    cat += ["    ],", ")", ""]
    (folder / "catalog.ron").write_text("\n".join(cat))
    return key


def register(keys):
    """공용 카탈로그의 템플릿 목록에 sd2_* 를 (다시) 적는다 — 다른 템플릿 줄은 그대로."""
    path = OUT / "catalog.ron"
    lines = path.read_text().split("\n")
    a = lines.index("    templates: [")
    b = lines.index("    ],", a)
    keep = [ln for ln in lines[a + 1:b] if '"sd2_' not in ln]
    existing = sorted(p.parent.name for p in OUT.glob("sd2_*/catalog.ron"))
    add = [f"        {ron_str(k + '/catalog.ron')}," for k in existing]
    path.write_text("\n".join(lines[:a + 1] + keep + add + lines[b:]))


def main():
    if not WC.exists():
        sys.exit(f"{WC} 가 없다 — 먼저 `cargo build --release --no-default-features`")
    names = [a for a in sys.argv[1:] if not a.startswith("-")] or PAPERDOLL
    os.chdir(ROOT)
    keys = [build(n) for n in names]
    register(keys)
    for k in keys:
        print(OUT / k)


if __name__ == "__main__":
    # 카탈로그·원본에 한글이 있다 — Windows 기본 인코딩(CP949 등)이 아니라 UTF-8 로 쓰도록 그 모드로 다시 띄운다.
    if not sys.flags.utf8_mode:
        import os as _os
        import subprocess as _sp

        _os.environ["PYTHONUTF8"] = "1"
        sys.exit(_sp.run([sys.executable, "-X", "utf8", *sys.argv]).returncode)
    main()
