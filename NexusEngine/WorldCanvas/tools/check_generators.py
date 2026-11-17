"""생성기 스냅숏 검사 — 생성기 규칙을 고쳤을 때 의도하지 않은 변화를 잡는다.

    python3 tools/check_generators.py            # 검사만 (다르면 종료 코드 1)
    python3 tools/check_generators.py --update   # 종이 인형 스냅숏을 지금 출력으로 다시 적는다

검사하는 것:
1. 체형 템플릿 — `sd_template.py` 가 만드는 원본(머리 포함)이 `sprites/template_<체형>.canvas` 와 같은가.
2. 손으로 짠 원본의 생성 절 · 통째 생성 원본(gloom_sd.py) — 머리(`tools/<이름>_head.py`)·골격 다리(`skeleton_legs.py`)가 원본의 그 절과 같은가.
3. 종이 인형 — `sd_paperdoll.py`·`grim_paperdoll.py` 가 파츠마다 만드는 임시 원본의 해시가 `tools/snapshots/paperdoll.txt` 와 같은가.
   종이 인형 결과물(WorldEditor 애셋)은 추적하지 않으므로 해시로 지킨다.

1·2 가 다르면 둘 중 하나다 — 생성기를 고쳤는데 다시 만들지 않았거나(→ 생성기를 돌린다), 원본을 손으로 고쳤다
(→ 규칙으로 옮긴다). 3 이 다르면 바뀐 파츠를 보고 의도한 변화면 `--update`.

`cargo test`(tests/generators_in_sync.rs)가 이것을 부른다. 종이 인형 회색 단계는 `worldcanvas list` 가 필요하다 —
`WORLDCANVAS_BIN` 이 있으면 그 실행 파일을, 없으면 target/release/worldcanvas 를 쓴다.
"""
import hashlib
import os
import subprocess
import sys
import tempfile
from concurrent.futures import ProcessPoolExecutor, ThreadPoolExecutor
from pathlib import Path

TOOLS = Path(__file__).resolve().parent
ROOT = TOOLS.parent
sys.path.insert(0, str(TOOLS))
os.chdir(ROOT)

import sd_template as T  # noqa: E402

SNAPSHOT = TOOLS / "snapshots" / "paperdoll.txt"
HEADS = ["dark_knight_v2", "forest_witch_v4", "template_m", "template_f"]
LEGS = ["dark_knight_v2", "forest_witch_v4"]


def first_diff(a, b):
    """처음 다른 줄 — (줄 번호, 생성기, 원본)."""
    la, lb = a.split("\n"), b.split("\n")
    for i, (x, y) in enumerate(zip(la, lb)):
        if x != y:
            return i + 1, x, y
    n = min(len(la), len(lb))
    return n + 1, (la[n] if n < len(la) else "<끝>"), (lb[n] if n < len(lb) else "<끝>")


def section(text, head, end):
    a = text.index(head)
    return text[a:text.index(end, a) + len(end)]


def prepare_template(name, tmp):
    """몸 부분 원본을 임시 파일로 — sd_template 은 전역(FINE)을 쓰므로 한 스레드에서."""
    b = T.BODIES[name]
    path = Path(tmp) / f"template_{name}.canvas"
    path.write_text(T.body_text(name, b))
    return path


def check_template(name, path):
    """머리를 칠해 넣고(하위 프로세스 — 병렬) 원본과 비교."""
    b = T.BODIES[name]
    r = subprocess.run(T.head_args(name, b, str(path)) if T.uses_head_engine(b) else ["true"],
                       capture_output=True, text=True)
    if r.returncode:
        return f"template_{name}: 머리 생성 실패 — {r.stderr.strip()}"
    want = (ROOT / "sprites" / f"template_{name}.canvas").read_text()
    got = path.read_text()
    if got != want:
        n, g, w = first_diff(got, want)
        return f"template_{name}.canvas: {n}줄이 다름\n    생성기: {g[:120]}\n    원본  : {w[:120]}"
    return None


def check_head(name):
    src = (ROOT / "sprites" / f"{name}.canvas").read_text()
    res = int(src.split("\ncell ", 1)[1].split()[0]) // 56
    r = subprocess.run(["python3", f"tools/{name}_head.py", f"--res={res}"], capture_output=True, text=True)
    if r.returncode:
        return f"{name}: 머리 생성 실패 — {r.stderr.strip()}"
    want = section(src, "// ── 머리 — ", "// ── 머리 끝 ──") + "\n"
    if r.stdout != want:
        n, g, w = first_diff(r.stdout, want)
        return f"{name}.canvas 머리 절: {n}줄이 다름\n    생성기: {g[:120]}\n    원본  : {w[:120]}"
    return None


def check_legs(name):
    r = subprocess.run(["python3", "tools/skeleton_legs.py", name], capture_output=True, text=True)
    if r.returncode:
        return f"{name}: 골격 다리 생성 실패 — {r.stderr.strip()}"
    got = r.stdout.split("\n", 1)[1]  # 첫 줄은 몸 오르내림 안내
    src = (ROOT / "sprites" / f"{name}.canvas").read_text()
    want = section(src, "// ── 골격 다리 — tools/skeleton_legs.py", "// ── 골격 다리 끝 ──") + "\n"
    if got != want:
        n, g, w = first_diff(got, want)
        return f"{name}.canvas 골격 다리 절: {n}줄이 다름\n    생성기: {g[:120]}\n    원본  : {w[:120]}"
    return None


def body_hashes(name):
    """체형 하나의 파츠 → 임시 원본 텍스트의 해시 (sd_paperdoll 이 build 하기 직전의 원본). 프로세스 하나에서."""
    import sd_paperdoll as P
    b = dict(T.BODIES[name])
    out = {}
    for p in P.parts_for(b, P.head_grids(b)):
        text = P.part_canvas(name, b, p)
        out[f"sd2_{name}/{p.key}"] = hashlib.sha256(text.encode()).hexdigest()[:16]
    return out


def check_whole(script, canvas, *args):
    """원본 하나를 통째로 쓰는 생성기 (`--print`) — 출력 = sprites/<canvas>."""
    r = subprocess.run(["python3", f"tools/{script}", "--print", *args], capture_output=True, text=True)
    if r.returncode:
        return f"{script}: 실패 — {r.stderr.strip()}"
    want = (ROOT / "sprites" / canvas).read_text()
    if r.stdout != want:
        n, g, w = first_diff(r.stdout, want)
        return f"{canvas}: {n}줄이 다름\n    생성기: {g[:120]}\n    원본  : {w[:120]}"
    return None


WHOLE = [("gloom_sd.py", "gloom_m.canvas"), ("grim_sd.py", "grim_m.canvas"),
         ("grim_sd.py", "grim_face_m.canvas", "--face"), ("grim_sd.py", "grim_f.canvas", "--female"),
         ("grim_sd.py", "grim_face_f.canvas", "--female", "--face"),
         *[("grim_sd.py", f"grim_obj_{h}.canvas", f"--head={h}") for h in ("lantern", "cage", "bell", "candle")],
         *[("grim_sd.py", f"grim_obj_{h}_{b}.canvas", f"--head={h}", f"--body={b}")
           for h, b in (("candle", "robe"), ("cage", "coat"), ("bell", "rags"), ("lantern", "traveler"))],
         *[("grim_sd.py", f"grim_obj_{h}_{b}_lit.canvas", f"--head={h}", f"--body={b}", "--lit")
           for h, b in (("candle", "robe"), ("cage", "coat"), ("lantern", "traveler"))]]


def paperdoll_hashes():
    # 머리 엔진(sd_head)과 sd_template 은 전역 상태를 쓴다 — 체형마다 프로세스를 나눠 병렬로
    import sd_paperdoll as P
    out = {}
    with ProcessPoolExecutor(max_workers=os.cpu_count() or 4) as pool:
        for h in pool.map(body_hashes, P.PAPERDOLL):
            out.update(h)
        out.update(pool.submit(grim_hashes).result())
    return out


def grim_hashes():
    """grim 종이 인형(오브젝트헤드 + 장비 파츠) — 파츠마다 원본 텍스트의 해시. 프로세스 하나에서 (grim_sd 전역)."""
    import grim_paperdoll as GP
    return {f"grim/{key}": hashlib.sha256(GP.part_canvas(key, draw, layer).encode()).hexdigest()[:16]
            for key, _, _, draw, layer in GP.PARTS}


def present(canvas):
    """비교할 원본이 sprites/ 에 있는가 — 정리해서 지운 원본(시험 샘플)은 검사하지 않는다."""
    return (ROOT / "sprites" / canvas).exists()


def main():
    update = "--update" in sys.argv
    problems = []
    with tempfile.TemporaryDirectory() as tmp, ThreadPoolExecutor(max_workers=os.cpu_count() or 4) as pool:
        bodies = [n for n in T.BODIES if present(f"template_{n}.canvas")]
        heads = [n for n in HEADS if present(f"{n}.canvas")]
        legs = [n for n in LEGS if present(f"{n}.canvas")]
        whole = [w for w in WHOLE if present(w[1])]
        paths = {n: prepare_template(n, tmp) for n in bodies}
        jobs = [pool.submit(check_template, n, paths[n]) for n in bodies]
        jobs += [pool.submit(check_head, n) for n in heads]
        jobs += [pool.submit(check_legs, n) for n in legs]
        jobs += [pool.submit(check_whole, *w) for w in whole]
        problems += [j.result() for j in jobs]
    checked = len(bodies) + len(heads) + len(legs) + len(whole)
    skipped = len(T.BODIES) + len(HEADS) + len(LEGS) + len(WHOLE) - checked

    hashes = paperdoll_hashes()
    if update:
        SNAPSHOT.parent.mkdir(exist_ok=True)
        SNAPSHOT.write_text("# tools/check_generators.py --update 가 적는다 — 종이 인형 파츠 원본의 해시\n" +
                            "".join(f"{k} {v}\n" for k, v in sorted(hashes.items())))
        print(f"종이 인형 스냅숏 갱신 — {len(hashes)}개 → {SNAPSHOT.relative_to(ROOT)}")
    elif not SNAPSHOT.exists():
        problems.append(f"{SNAPSHOT.relative_to(ROOT)} 가 없다 — `python3 tools/check_generators.py --update`")
    else:
        old = dict(ln.split() for ln in SNAPSHOT.read_text().splitlines() if ln and not ln.startswith("#"))
        changed = sorted(k for k in hashes.keys() & old.keys() if hashes[k] != old[k])
        added, removed = sorted(hashes.keys() - old.keys()), sorted(old.keys() - hashes.keys())
        for label, keys in (("바뀜", changed), ("새로 생김", added), ("사라짐", removed)):
            if keys:
                more = f" 외 {len(keys) - 8}개" if len(keys) > 8 else ""
                problems.append(f"종이 인형 {label}: {', '.join(keys[:8])}{more}")
        checked += len(hashes)

    problems = [p for p in problems if p]
    for p in problems:
        print(p)
    if problems:
        print(f"\n생성기와 다름 {len(problems)}건 — 생성기를 다시 돌리거나(의도한 변화), 규칙을 고친다(의도하지 않은 변화). "
              "종이 인형이 의도한 변화면 `python3 tools/check_generators.py --update`.")
        sys.exit(1)
    print(f"생성기 스냅숏 일치 — {checked}개" + (f" (sprites/ 에 원본이 없어 건너뜀 {skipped}개)" if skipped else ""))


if __name__ == "__main__":
    main()
