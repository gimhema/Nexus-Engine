//! `.canvas` 텍스트 → [`Document`].
//!
//! 줄 단위 문법이다. `//` 부터 줄 끝까지 주석, 빈 줄은 무시, 블록은 `end` 로 닫는다.
//! 문법 전체는 `CLAUDE.md` 의 "원본 형식" 절에 있다.
//!
//! 파싱이 끝나면 [`validate`] 가 이름·크기·팔레트·시트 배치를 한 번에 검사한다 —
//! 팔레트를 프레임보다 뒤에 적어도 되도록 색 검사는 파싱 중이 아니라 여기서 한다.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::color::{KEEP, Palette, Rgba, generate_ramp};
use crate::doc::{
    AtlasDef, ClipDef, Document, ImageDef, ImageKind, Op, OpAt, OutputDef, Ramp, STATES, SheetDef,
    VERSION,
};
use crate::error::{Error, Result};

#[derive(Clone, Copy)]
struct Line<'a> {
    no: usize,
    text: &'a str,
}

impl<'a> Line<'a> {
    fn words(&self) -> Vec<&'a str> {
        self.text.split_whitespace().collect()
    }

    fn err(&self, msg: impl Into<String>) -> Error {
        Error::at(self.no, msg)
    }
}

struct Parser<'a> {
    lines: Vec<Line<'a>>,
    pos: usize,
    /// 원본 파일 폴더 — `use` 경로의 기준. 문자열에서 읽으면 없다.
    dir: Option<PathBuf>,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str, dir: Option<PathBuf>) -> Self {
        let lines = src
            .lines()
            .enumerate()
            .map(|(i, raw)| {
                let text = raw.find("//").map_or(raw, |at| &raw[..at]).trim();
                Line { no: i + 1, text }
            })
            .filter(|l| !l.text.is_empty())
            .collect();
        Self { lines, pos: 0, dir }
    }

    fn next(&mut self) -> Option<Line<'a>> {
        let l = self.lines.get(self.pos).copied();
        self.pos += 1;
        l
    }

    /// 블록 안의 다음 줄. `end` 면 `None`, 파일이 끝나면 오류.
    fn block_line(&mut self, opened: Line<'_>, what: &str) -> Result<Option<Line<'a>>> {
        match self.next() {
            Some(l) if l.text == "end" => Ok(None),
            Some(l) => Ok(Some(l)),
            None => Err(opened.err(format!("{what} 블록이 `end` 로 닫히지 않음"))),
        }
    }

    /// `grid` / `patch` 의 텍스트 행들.
    fn rows(&mut self, opened: Line<'_>) -> Result<Vec<Vec<u8>>> {
        let mut rows = Vec::new();
        while let Some(l) = self.block_line(opened, "그림")? {
            if l.text.contains(char::is_whitespace) {
                return Err(l.err("그림 행에 공백이 있음 — 한 글자가 한 픽셀"));
            }
            rows.push(l.text.as_bytes().to_vec());
        }
        if rows.is_empty() {
            return Err(opened.err("그림 행이 없음"));
        }
        let w = rows[0].len();
        if let Some(i) = rows.iter().position(|r| r.len() != w) {
            return Err(opened.err(format!(
                "그림 행 길이가 다름 — 첫 행 {w}, {}번째 행 {}",
                i + 1,
                rows[i].len()
            )));
        }
        Ok(rows)
    }
}

fn arity(l: &Line<'_>, w: &[&str], n: usize, usage: &str) -> Result<()> {
    if w.len() == n {
        Ok(())
    } else {
        Err(l.err(format!("형식: {usage}")))
    }
}

fn int(l: &Line<'_>, s: &str) -> Result<i32> {
    s.parse().map_err(|_| l.err(format!("정수가 아님: {s}")))
}

fn uint(l: &Line<'_>, s: &str) -> Result<u32> {
    s.parse()
        .map_err(|_| l.err(format!("0 이상의 정수가 아님: {s}")))
}

fn positive(l: &Line<'_>, s: &str) -> Result<u32> {
    match uint(l, s)? {
        0 => Err(l.err("0 이 될 수 없음")),
        n => Ok(n),
    }
}

fn color(l: &Line<'_>, s: &str) -> Result<u8> {
    match s.as_bytes() {
        [c] => Ok(*c),
        _ => Err(l.err(format!("색은 팔레트 문자 한 글자: {s}"))),
    }
}

fn boolean(l: &Line<'_>, s: &str) -> Result<bool> {
    match s {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(l.err(format!("true 또는 false: {s}"))),
    }
}

fn is_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// 문서를 읽고 검사까지 마친다. 팔레트의 `use` 는 쓸 수 없다 — 기준 폴더가 없다.
pub fn parse(src: &str) -> Result<Document> {
    parse_in(src, None)
}

/// 파일에서 읽는다. 팔레트의 `use <경로>` 는 이 파일 폴더 기준이다.
pub fn parse_file(path: &Path) -> Result<Document> {
    let src = std::fs::read_to_string(path)
        .map_err(|e| Error::new(format!("{}: {e}", path.display())))?;
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    parse_in(&src, Some(dir))
}

/// 원본이 읽어 들이는 다른 파일들 (`use`) — 뷰어가 바뀜을 감시할 때.
#[must_use]
pub fn dependencies(path: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut todo = vec![path.to_path_buf()];
    while let Some(p) = todo.pop() {
        let Ok(src) = std::fs::read_to_string(&p) else {
            continue;
        };
        let dir = p.parent().map(Path::to_path_buf).unwrap_or_default();
        for line in src.lines() {
            let text = line.find("//").map_or(line, |at| &line[..at]).trim();
            if let Some(rest) = text.strip_prefix("use ") {
                let dep = dir.join(rest.trim());
                if !out.contains(&dep) && out.len() < 32 {
                    out.push(dep.clone());
                    todo.push(dep);
                }
            }
        }
    }
    out
}

fn parse_in(src: &str, dir: Option<PathBuf>) -> Result<Document> {
    let mut p = Parser::new(src, dir);
    let mut doc = Document::default();

    let first = p.next().ok_or_else(|| Error::new("빈 문서"))?;
    match first.words().as_slice() {
        ["canvas", v] if *v == VERSION.to_string() => {}
        ["canvas", v] => return Err(first.err(format!("형식 버전 {v} 은(는) 읽을 수 없음"))),
        _ => return Err(first.err(format!("첫 줄은 `canvas {VERSION}`"))),
    }

    let mut cell = None;
    let mut palette = None;
    while let Some(l) = p.next() {
        let w = l.words();
        match w[0] {
            "cell" => {
                arity(&l, &w, 3, "cell <가로> <세로>")?;
                cell = Some((positive(&l, w[1])?, positive(&l, w[2])?));
            }
            "palette" => {
                if palette.is_some() {
                    return Err(l.err("palette 블록이 두 번 나옴"));
                }
                let (pal, ramps) = parse_palette(&mut p, l)?;
                palette = Some(pal);
                doc.ramps = ramps;
            }
            "frame" | "part" => {
                let def = parse_image(&mut p, l, &w, cell)?;
                doc.images.push(def);
            }
            "frames" => {
                let defs = parse_frames(&mut p, l, cell)?;
                doc.images.extend(defs);
            }
            "sheet" => {
                if doc.sheet.is_some() {
                    return Err(l.err("sheet 블록이 두 번 나옴"));
                }
                doc.sheet = Some(parse_sheet(&mut p, l)?);
            }
            "atlas" => {
                if doc.atlas.is_some() {
                    return Err(l.err("atlas 블록이 두 번 나옴"));
                }
                doc.atlas = Some(parse_atlas(&mut p, l)?);
            }
            "output" => {
                if doc.output.is_some() {
                    return Err(l.err("output 블록이 두 번 나옴"));
                }
                doc.output = Some(parse_output(&mut p, l)?);
            }
            other => return Err(l.err(format!("알 수 없는 항목: {other}"))),
        }
    }

    // frame 이 없는 문서(부품만 모은 아틀라스)는 cell 이 필요 없다.
    doc.cell = cell.unwrap_or((0, 0));
    doc.palette = palette.unwrap_or_default();
    validate(&doc)?;
    Ok(doc)
}

fn parse_palette(p: &mut Parser<'_>, opened: Line<'_>) -> Result<(Palette, Vec<Ramp>)> {
    let mut pal = Palette::default();
    let mut ramps = Vec::new();
    let dir = p.dir.clone();
    while let Some(l) = p.block_line(opened, "palette")? {
        palette_line(&l.words(), l.no, dir.as_deref(), &mut pal, &mut ramps, 0)
            .map_err(|e| l.err(e))?;
    }
    Ok((pal, ramps))
}

/// 팔레트 한 줄 — `<문자> #색`, `ramp …`, `use <경로>`.
///
/// 공용 팔레트 파일(`use`)도 같은 줄들로 되어 있다. 그 파일 안 오류는 `파일:N줄:` 을 붙여
/// `use` 줄의 오류로 돌려준다.
fn palette_line(
    w: &[&str],
    line: usize,
    dir: Option<&Path>,
    pal: &mut Palette,
    ramps: &mut Vec<Ramp>,
    depth: usize,
) -> std::result::Result<(), String> {
    match w[0] {
        "use" => {
            if w.len() != 2 {
                return Err(String::from("형식: use <팔레트 파일 경로>"));
            }
            let dir = dir.ok_or("use 는 파일에서 읽을 때만 쓸 수 있음")?;
            if depth > 8 {
                return Err(String::from("use 가 너무 깊음 — 순환?"));
            }
            let path = dir.join(w[1]);
            let src =
                std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let sub = path.parent().unwrap_or(Path::new(""));
            for (i, raw) in src.lines().enumerate() {
                let text = raw.find("//").map_or(raw, |at| &raw[..at]).trim();
                if text.is_empty() {
                    continue;
                }
                let words: Vec<&str> = text.split_whitespace().collect();
                palette_line(&words, line, Some(sub), pal, ramps, depth + 1)
                    .map_err(|e| format!("{}:{}줄: {e}", w[1], i + 1))?;
            }
            Ok(())
        }
        "ramp" => parse_ramp(w, line, pal, ramps),
        _ => {
            if w.len() != 2 {
                return Err(String::from(
                    "형식: <문자> #rrggbb  |  ramp …  |  use <경로>",
                ));
            }
            let key = match w[0].as_bytes() {
                [c] => *c,
                _ => return Err(format!("색은 팔레트 문자 한 글자: {}", w[0])),
            };
            pal.insert(key, Rgba::parse_hex(w[1])?)
        }
    }
}

/// `ramp <이름> <문자들>` — 이미 있는 색을 밝은 것부터 묶는다.
/// `ramp <이름> <문자들> from #색 [at N] [step S] [hue H]` — 색을 만들어 팔레트에 넣는다.
fn parse_ramp(
    w: &[&str],
    line: usize,
    pal: &mut Palette,
    ramps: &mut Vec<Ramp>,
) -> std::result::Result<(), String> {
    const USAGE: &str = "형식: ramp <이름> <문자들> [from #색 [at N] [step S] [hue H]]";
    if w.len() < 3 {
        return Err(USAGE.into());
    }
    let (name, keys) = (w[1], w[2].as_bytes().to_vec());
    if !is_name(name) {
        return Err(format!("램프 이름은 글자·숫자·_ 만: {name}"));
    }
    if keys.len() < 2 {
        return Err(format!("램프 `{name}` 은 두 단계 이상"));
    }
    if ramps.iter().any(|r| r.name == name) {
        return Err(format!("램프 `{name}` 이 두 번 나옴"));
    }
    match &w[3..] {
        [] => {}
        ["from", hex, opts @ ..] => {
            let base = Rgba::parse_hex(hex)?;
            let (mut at, mut step, mut hue) = (keys.len() / 2, 0.10_f32, 6.0_f32);
            for kv in opts.chunks(2) {
                let [k, v] = kv else { return Err(USAGE.into()) };
                let num = |v: &str| v.parse::<f32>().map_err(|_| format!("수가 아님: {v}"));
                match *k {
                    "at" => {
                        at = v.parse().map_err(|_| format!("at 은 0 이상 정수: {v}"))?;
                        if at >= keys.len() {
                            return Err(format!("at {at} 이 단계 수 {} 밖", keys.len()));
                        }
                    }
                    "step" => step = num(v)?,
                    "hue" => hue = num(v)?,
                    other => return Err(format!("ramp 옵션은 at·step·hue: {other}")),
                }
            }
            for (k, c) in keys
                .iter()
                .zip(generate_ramp(base, keys.len(), at, step, hue))
            {
                pal.insert(*k, c)?;
            }
        }
        _ => return Err(USAGE.into()),
    }
    ramps.push(Ramp {
        name: name.to_owned(),
        keys,
        line,
    });
    Ok(())
}

/// `frame <이름> [= <바탕>]` / `part <이름> <가로> <세로> [= <바탕>]`.
fn parse_image(
    p: &mut Parser<'_>,
    opened: Line<'_>,
    w: &[&str],
    cell: Option<(u32, u32)>,
) -> Result<ImageDef> {
    let (kind, head, rest) = if w[0] == "frame" {
        (ImageKind::Frame, 2, &w[2..])
    } else {
        (ImageKind::Part, 4, w.get(4..).unwrap_or_default())
    };
    let usage = match kind {
        ImageKind::Frame => "frame <이름> [= <바탕>]",
        ImageKind::Part => "part <이름> <가로> <세로> [= <바탕>]",
    };
    if w.len() < head {
        return Err(opened.err(format!("형식: {usage}")));
    }
    let name = w[1];
    if !is_name(name) {
        return Err(opened.err(format!("이름은 글자·숫자·_ 만: {name}")));
    }
    let base = match rest {
        [] => None,
        ["=", base] => Some((*base).to_owned()),
        _ => return Err(opened.err(format!("형식: {usage}"))),
    };
    let size = match kind {
        ImageKind::Frame => cell.ok_or_else(|| opened.err("frame 보다 `cell` 이 먼저 와야 함"))?,
        ImageKind::Part => (positive(&opened, w[2])?, positive(&opened, w[3])?),
    };

    let mut ops = Vec::new();
    while let Some(l) = p.block_line(opened, w[0])? {
        let op = parse_op(p, l)?;
        ops.push(OpAt { op, line: l.no });
    }
    Ok(ImageDef {
        name: name.to_owned(),
        kind,
        size,
        base,
        ops,
        line: opened.no,
    })
}

/// `frames <접두사> <수> [= <바탕>]` — 연산 줄을 공유하는 프레임 묶음.
///
/// 프레임 `<접두사>0` … `<접두사><수-1>` 을 만든다. 연산 줄 안의 `[a b c d]` 는 프레임마다 다른
/// 값 — i 번째 프레임은 i 번째 값을 쓴다. 걷기 프레임의 상체 오르내림, 한 박자 늦게 따라오는
/// 망토·머리카락, 칼 각도를 한 표로 적는다. 바탕도 `= [a b c d]` 로 프레임마다 다르게 둘 수 있다.
fn parse_frames(
    p: &mut Parser<'_>,
    opened: Line<'_>,
    cell: Option<(u32, u32)>,
) -> Result<Vec<ImageDef>> {
    const USAGE: &str = "frames <접두사> <수> [= <바탕> | = [바탕들]]";
    let size = cell.ok_or_else(|| opened.err("frames 보다 `cell` 이 먼저 와야 함"))?;
    let (head, rest) = opened
        .text
        .split_once('=')
        .map_or((opened.text, None), |(h, r)| (h, Some(r.trim())));
    let hw: Vec<&str> = head.split_whitespace().collect();
    let [_, prefix, count] = hw.as_slice() else {
        return Err(opened.err(format!("형식: {USAGE}")));
    };
    let count = positive(&opened, count)? as usize;
    let names: Vec<String> = (0..count).map(|i| format!("{prefix}{i}")).collect();
    if !is_name(&names[0]) {
        return Err(opened.err(format!("이름은 글자·숫자·_ 만: {prefix}")));
    }
    let bases: Vec<Option<String>> = match rest {
        None => vec![None; count],
        Some(r) => (0..count)
            .map(|i| {
                let b = per_frame(r, i, count).map_err(|e| opened.err(e))?;
                match b.split_whitespace().collect::<Vec<_>>().as_slice() {
                    [one] => Ok(Some((*one).to_owned())),
                    _ => Err(opened.err(format!("형식: {USAGE}"))),
                }
            })
            .collect::<Result<_>>()?,
    };

    let mut ops: Vec<Vec<OpAt>> = vec![Vec::new(); count];
    while let Some(l) = p.block_line(opened, "frames")? {
        if l.text.contains('[') {
            for (i, list) in ops.iter_mut().enumerate() {
                let text = per_frame(l.text, i, count).map_err(|e| l.err(e))?;
                let one = Line {
                    no: l.no,
                    text: &text,
                };
                if matches!(one.words()[0], "grid" | "patch") {
                    return Err(l.err("grid·patch 머리에는 [ ] 값을 쓸 수 없음"));
                }
                list.push(OpAt {
                    op: parse_op(p, one)?,
                    line: l.no,
                });
            }
        } else {
            let op = parse_op(p, l)?;
            for list in &mut ops {
                list.push(OpAt {
                    op: op.clone(),
                    line: l.no,
                });
            }
        }
    }
    Ok(names
        .into_iter()
        .zip(bases)
        .zip(ops)
        .map(|((name, base), ops)| ImageDef {
            name,
            kind: ImageKind::Frame,
            size,
            base,
            ops,
            line: opened.no,
        })
        .collect())
}

/// `[a b c]` 묶음을 i 번째 값으로 바꾼 줄. 묶음의 값 수는 프레임 수와 같아야 한다.
fn per_frame(text: &str, i: usize, count: usize) -> std::result::Result<String, String> {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let close = rest[open..].find(']').ok_or("`[` 가 `]` 로 닫히지 않음")? + open;
        let values: Vec<&str> = rest[open + 1..close].split_whitespace().collect();
        if values.len() != count {
            return Err(format!(
                "[{}] 의 값이 {} 개 — 프레임 수 {count} 와 같아야 함",
                rest[open + 1..close].trim(),
                values.len()
            ));
        }
        out.push_str(&rest[..open]);
        out.push_str(values[i]);
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn parse_op(p: &mut Parser<'_>, l: Line<'_>) -> Result<Op> {
    let w = l.words();
    let op = match w[0] {
        "grid" => {
            arity(&l, &w, 1, "grid")?;
            Op::Grid(p.rows(l)?)
        }
        "patch" => {
            arity(&l, &w, 3, "patch <x> <y>")?;
            let (x, y) = (int(&l, w[1])?, int(&l, w[2])?);
            Op::Patch {
                x,
                y,
                rows: p.rows(l)?,
            }
        }
        "px" => {
            arity(&l, &w, 4, "px <x> <y> <색>")?;
            Op::Px {
                x: int(&l, w[1])?,
                y: int(&l, w[2])?,
                c: color(&l, w[3])?,
            }
        }
        "rect" | "box" => {
            arity(&l, &w, 6, &format!("{} <x> <y> <가로> <세로> <색>", w[0]))?;
            let (x, y) = (int(&l, w[1])?, int(&l, w[2])?);
            let (rw, rh, c) = (positive(&l, w[3])?, positive(&l, w[4])?, color(&l, w[5])?);
            if w[0] == "rect" {
                Op::Rect {
                    x,
                    y,
                    w: rw,
                    h: rh,
                    c,
                }
            } else {
                Op::Box {
                    x,
                    y,
                    w: rw,
                    h: rh,
                    c,
                }
            }
        }
        "line" => {
            arity(&l, &w, 6, "line <x0> <y0> <x1> <y1> <색>")?;
            Op::Line {
                x0: int(&l, w[1])?,
                y0: int(&l, w[2])?,
                x1: int(&l, w[3])?,
                y1: int(&l, w[4])?,
                c: color(&l, w[5])?,
            }
        }
        "fill" => {
            arity(&l, &w, 4, "fill <x> <y> <색>")?;
            Op::Fill {
                x: int(&l, w[1])?,
                y: int(&l, w[2])?,
                c: color(&l, w[3])?,
            }
        }
        "flip_h" => {
            arity(&l, &w, 1, "flip_h")?;
            Op::FlipH
        }
        "flip_v" => {
            arity(&l, &w, 1, "flip_v")?;
            Op::FlipV
        }
        "shift" => {
            arity(&l, &w, 3, "shift <dx> <dy>")?;
            Op::Shift {
                dx: int(&l, w[1])?,
                dy: int(&l, w[2])?,
            }
        }
        "move" => {
            arity(&l, &w, 7, "move <x> <y> <가로> <세로> <dx> <dy>")?;
            Op::Move {
                x: int(&l, w[1])?,
                y: int(&l, w[2])?,
                w: positive(&l, w[3])?,
                h: positive(&l, w[4])?,
                dx: int(&l, w[5])?,
                dy: int(&l, w[6])?,
            }
        }
        "swap" => {
            let region = match w.len() {
                3 => None,
                7 => Some((
                    int(&l, w[3])?,
                    int(&l, w[4])?,
                    positive(&l, w[5])?,
                    positive(&l, w[6])?,
                )),
                _ => return Err(l.err("형식: swap <원래 색> <바꿀 색> [<x> <y> <가로> <세로>]")),
            };
            Op::Swap {
                from: color(&l, w[1])?,
                to: color(&l, w[2])?,
                region,
            }
        }
        "outline" => {
            arity(&l, &w, 2, "outline <색>")?;
            Op::Outline {
                c: color(&l, w[1])?,
            }
        }
        "selout" => {
            if w.len() < 3 {
                return Err(l.err(String::from("형식: selout <외곽색> <안쪽색><결과색> ...")));
            }
            let mut pairs = Vec::with_capacity(w.len() - 2);
            for pair in &w[2..] {
                match pair.as_bytes() {
                    [from, to] => pairs.push((*from, *to)),
                    _ => return Err(l.err(format!("안쪽색·결과색 두 글자여야 함: {pair}"))),
                }
            }
            Op::Selout {
                c: color(&l, w[1])?,
                pairs,
            }
        }
        "mirror" => {
            arity(&l, &w, 1, "mirror")?;
            Op::Mirror
        }
        "ellipse" => {
            arity(&l, &w, 6, "ellipse <x> <y> <가로> <세로> <색>")?;
            Op::Ellipse {
                x: int(&l, w[1])?,
                y: int(&l, w[2])?,
                w: positive(&l, w[3])?,
                h: positive(&l, w[4])?,
                c: color(&l, w[5])?,
            }
        }
        "poly" => {
            let usage = "poly <색> <x1> <y1> <x2> <y2> <x3> <y3> …";
            if w.len() < 8 || !w.len().is_multiple_of(2) {
                return Err(l.err(format!("형식: {usage} (꼭짓점 3개 이상)")));
            }
            let points = w[2..]
                .chunks(2)
                .map(|p| Ok((int(&l, p[0])?, int(&l, p[1])?)))
                .collect::<Result<_>>()?;
            Op::Poly {
                c: color(&l, w[1])?,
                points,
            }
        }
        "stamp_rot" => {
            let pivot = match w.len() {
                5 => None,
                7 => Some((int(&l, w[5])?, int(&l, w[6])?)),
                _ => {
                    return Err(l.err("형식: stamp_rot <이름> <x> <y> <각도> [<피벗 x> <피벗 y>]"));
                }
            };
            Op::StampRot {
                name: w[1].to_owned(),
                x: int(&l, w[2])?,
                y: int(&l, w[3])?,
                deg: int(&l, w[4])?,
                pivot,
            }
        }
        "shade" => {
            let region = match w.len() {
                3 => None,
                7 => Some((
                    int(&l, w[3])?,
                    int(&l, w[4])?,
                    positive(&l, w[5])?,
                    positive(&l, w[6])?,
                )),
                _ => return Err(l.err("형식: shade <램프> <단계> [<x> <y> <가로> <세로>]")),
            };
            Op::Shade {
                ramp: w[1].to_owned(),
                steps: int(&l, w[2])?,
                region,
            }
        }
        "light" => {
            if !(2..=5).contains(&w.len()) {
                return Err(l.err("형식: light <램프> [반경 0=자동] [어둡게 2] [밝게 1]"));
            }
            let opt = |i: usize, d: u32| w.get(i).map_or(Ok(d), |s| uint(&l, s));
            Op::Light {
                ramp: w[1].to_owned(),
                radius: opt(2, 0)?,
                dark: opt(3, 2)?,
                light: opt(4, 1)?,
            }
        }
        "stamp" => {
            arity(&l, &w, 4, "stamp <이름> <x> <y>")?;
            Op::Stamp {
                name: w[1].to_owned(),
                x: int(&l, w[2])?,
                y: int(&l, w[3])?,
            }
        }
        other => return Err(l.err(format!("알 수 없는 연산: {other}"))),
    };
    Ok(op)
}

fn parse_sheet(p: &mut Parser<'_>, opened: Line<'_>) -> Result<SheetDef> {
    let mut sheet = SheetDef {
        directions: 0,
        direction_rows: None,
        pixels_per_meter: None,
        tinted: false,
        clips: Vec::new(),
        line: opened.no,
    };
    while let Some(l) = p.block_line(opened, "sheet")? {
        let w = l.words();
        match w[0] {
            "directions" => {
                arity(&l, &w, 2, "directions <수>")?;
                sheet.directions = positive(&l, w[1])?;
            }
            "direction_rows" => {
                let rows = w[1..].iter().map(|s| uint(&l, s)).collect::<Result<_>>()?;
                sheet.direction_rows = Some(rows);
            }
            "pixels_per_meter" => {
                arity(&l, &w, 2, "pixels_per_meter <수>")?;
                let v: f32 = w[1]
                    .parse()
                    .map_err(|_| l.err(format!("수가 아님: {}", w[1])))?;
                if !(v.is_finite() && v > 0.0) {
                    return Err(l.err("pixels_per_meter 는 양수"));
                }
                sheet.pixels_per_meter = Some(v);
            }
            "tinted" => {
                arity(&l, &w, 2, "tinted true|false")?;
                sheet.tinted = boolean(&l, w[1])?;
            }
            "clip" => sheet.clips.push(parse_clip(p, l, &w)?),
            other => return Err(l.err(format!("sheet 안에 알 수 없는 항목: {other}"))),
        }
    }
    Ok(sheet)
}

/// `clip <상태> <프레임 ms> loop|once` + `row` 들.
fn parse_clip(p: &mut Parser<'_>, opened: Line<'_>, w: &[&str]) -> Result<ClipDef> {
    arity(&opened, w, 4, "clip <상태> <프레임 ms> loop|once")?;
    let looping = match w[3] {
        "loop" => true,
        "once" => false,
        s => return Err(opened.err(format!("loop 또는 once: {s}"))),
    };
    let frame_ms = positive(&opened, w[2])?.into();
    let mut rows = Vec::new();
    while let Some(l) = p.block_line(opened, "clip")? {
        let rw = l.words();
        if rw[0] != "row" || rw.len() < 2 {
            return Err(l.err("clip 안에는 `row <프레임> <프레임> …` 만"));
        }
        rows.push(rw[1..].iter().map(|s| (*s).to_owned()).collect());
    }
    Ok(ClipDef {
        state: w[1].to_owned(),
        frame_ms,
        looping,
        rows,
        line: opened.no,
    })
}

fn parse_atlas(p: &mut Parser<'_>, opened: Line<'_>) -> Result<AtlasDef> {
    let mut rows = Vec::new();
    while let Some(l) = p.block_line(opened, "atlas")? {
        let w = l.words();
        if w[0] != "row" || w.len() < 2 {
            return Err(l.err("atlas 안에는 `row <그림> <그림> …` 만"));
        }
        rows.push(w[1..].iter().map(|s| (*s).to_owned()).collect());
    }
    Ok(AtlasDef {
        rows,
        line: opened.no,
    })
}

fn parse_output(p: &mut Parser<'_>, opened: Line<'_>) -> Result<OutputDef> {
    let (mut image, mut sheet, mut image_ref) = (None, None, None);
    while let Some(l) = p.block_line(opened, "output")? {
        let (key, rest) = l
            .text
            .split_once(char::is_whitespace)
            .unwrap_or((l.text, ""));
        let rest = rest.trim();
        match key {
            "image" => image = Some(rest.to_owned()),
            "sheet" => sheet = Some(rest.to_owned()),
            "image_ref" => {
                let s = rest
                    .strip_prefix('"')
                    .and_then(|s| s.strip_suffix('"'))
                    .ok_or_else(|| l.err("image_ref 는 따옴표로: image_ref \"…\""))?;
                image_ref = Some(s.to_owned());
            }
            other => return Err(l.err(format!("output 안에 알 수 없는 항목: {other}"))),
        }
    }
    let need = |v: Option<String>, k: &str| {
        v.filter(|s| !s.is_empty())
            .ok_or_else(|| opened.err(format!("output 에 `{k} <경로>` 가 없음")))
    };
    Ok(OutputDef {
        image: need(image, "image")?,
        sheet: sheet.filter(|s| !s.is_empty()),
        image_ref,
    })
}

/// 이름 중복, 크기, 팔레트, 참조, 시트 배치.
///
/// 바탕·스탬프의 **순환**은 여기서 보지 않는다 — 실제로 따라가 봐야 알 수 있어
/// [`crate::resolve`] 가 잡는다.
pub fn validate(doc: &Document) -> Result<()> {
    for ramp in &doc.ramps {
        let mut seen = HashSet::new();
        for &k in &ramp.keys {
            if !doc.palette.contains(k) || k == b'.' {
                return Err(Error::at(
                    ramp.line,
                    format!("램프 `{}`: 팔레트에 없는 색 '{}'", ramp.name, k as char),
                ));
            }
            if !seen.insert(k) {
                return Err(Error::at(
                    ramp.line,
                    format!("램프 `{}`: '{}' 가 두 번 나옴", ramp.name, k as char),
                ));
            }
        }
    }

    let mut names = HashSet::new();
    for img in &doc.images {
        if !names.insert(img.name.as_str()) {
            return Err(Error::at(
                img.line,
                format!("이름이 두 번 나옴: {}", img.name),
            ));
        }
    }

    for img in &doc.images {
        if let Some(base) = &img.base {
            let b = doc
                .image(base)
                .ok_or_else(|| Error::at(img.line, format!("바탕 `{base}` 이(가) 없음")))?;
            if b.size != img.size {
                return Err(Error::at(
                    img.line,
                    format!(
                        "바탕 `{base}` 의 크기 {:?} 가 {:?} 와 다름",
                        b.size, img.size
                    ),
                ));
            }
        }
        for at in &img.ops {
            check_op(doc, img, at)?;
        }
    }

    if let Some(sheet) = &doc.sheet {
        check_sheet(doc, sheet)?;
    }
    if let Some(atlas) = &doc.atlas {
        if doc.sheet.is_some() {
            return Err(Error::at(
                atlas.line,
                "sheet 와 atlas 는 한 문서에 함께 둘 수 없음 — 결과 그림이 하나다",
            ));
        }
        if atlas.rows.is_empty() {
            return Err(Error::at(atlas.line, "atlas 에 row 가 없음"));
        }
        for name in atlas.rows.iter().flatten() {
            if doc.image(name).is_none() {
                return Err(Error::at(
                    atlas.line,
                    format!("atlas: `{name}` 이(가) 없음"),
                ));
            }
        }
    }
    if let Some(out) = &doc.output {
        match (&out.sheet, &doc.sheet, &doc.atlas) {
            (None, Some(s), _) => {
                return Err(Error::at(
                    s.line,
                    "sheet 블록이 있으면 output 에 `sheet <경로>` 가 필요함",
                ));
            }
            (Some(_), None, _) => {
                return Err(Error::new(
                    "output 의 sheet 경로가 있는데 sheet 블록이 없음",
                ));
            }
            (_, None, None) => return Err(Error::new("output 이 있는데 sheet 도 atlas 도 없음")),
            _ => {}
        }
    }
    Ok(())
}

fn check_op(doc: &Document, img: &ImageDef, at: &OpAt) -> Result<()> {
    let err = |msg: String| Error::at(at.line, msg);
    let block = matches!(at.op, Op::Grid(_) | Op::Patch { .. });
    for c in at.op.colors() {
        let ok = doc.palette.contains(c) || (block && c == KEEP);
        if !ok {
            return Err(err(format!("팔레트에 없는 색 '{}'", c as char)));
        }
    }
    match &at.op {
        Op::Grid(rows) => {
            let (w, h) = (rows[0].len() as u32, rows.len() as u32);
            if (w, h) != img.size {
                return Err(err(format!(
                    "grid 는 {}x{} 이어야 함 (지금 {w}x{h})",
                    img.size.0, img.size.1
                )));
            }
        }
        Op::Stamp { name, .. } | Op::StampRot { name, .. } if doc.image(name).is_none() => {
            return Err(err(format!("stamp 대상 `{name}` 이(가) 없음")));
        }
        Op::Shade { ramp, .. } | Op::Light { ramp, .. } if doc.ramp(ramp).is_none() => {
            return Err(err(format!("램프 `{ramp}` 이(가) 없음")));
        }
        Op::Swap { to, .. } if *to == KEEP => {
            return Err(err(String::from("swap 의 결과로 `~` 는 쓸 수 없음")));
        }
        _ => {}
    }
    Ok(())
}

fn check_sheet(doc: &Document, sheet: &SheetDef) -> Result<()> {
    let err = |line, msg: String| Error::at(line, msg);
    if doc.cell.0 == 0 {
        return Err(err(
            sheet.line,
            String::from("sheet 를 쓰려면 `cell <가로> <세로>` 가 필요함"),
        ));
    }
    if sheet.directions == 0 {
        return Err(err(
            sheet.line,
            String::from("sheet 에 `directions <수>` 가 없음"),
        ));
    }
    if sheet.clips.is_empty() {
        return Err(err(sheet.line, String::from("sheet 에 clip 이 없음")));
    }
    if let Some(rows) = &sheet.direction_rows {
        if rows.len() != sheet.directions as usize {
            return Err(err(
                sheet.line,
                format!(
                    "direction_rows 는 {} 개여야 함 (지금 {})",
                    sheet.directions,
                    rows.len()
                ),
            ));
        }
        if let Some(bad) = rows.iter().find(|&&r| r >= sheet.directions) {
            return Err(err(
                sheet.line,
                format!("direction_rows 의 {bad} 는 방향 수 밖"),
            ));
        }
    }

    let mut states = HashSet::new();
    for clip in &sheet.clips {
        if !STATES.contains(&clip.state.as_str()) {
            return Err(err(
                clip.line,
                format!("상태 `{}` 는 없음 — {}", clip.state, STATES.join(" / ")),
            ));
        }
        if !states.insert(clip.state.as_str()) {
            return Err(err(
                clip.line,
                format!("클립 `{}` 가 두 번 나옴", clip.state),
            ));
        }
        if clip.rows.len() != sheet.directions as usize {
            return Err(err(
                clip.line,
                format!(
                    "{}: row 가 {} 개여야 함 (방향 수) — 지금 {}",
                    clip.state,
                    sheet.directions,
                    clip.rows.len()
                ),
            ));
        }
        let frames = clip.frames() as usize;
        if clip.rows.iter().any(|r| r.len() != frames) {
            return Err(err(
                clip.line,
                format!("{}: 방향마다 프레임 수가 다름", clip.state),
            ));
        }
        for name in clip.rows.iter().flatten() {
            match doc.image(name) {
                None => {
                    return Err(err(
                        clip.line,
                        format!("{}: 프레임 `{name}` 이(가) 없음", clip.state),
                    ));
                }
                Some(i) if i.kind != ImageKind::Frame => {
                    return Err(err(
                        clip.line,
                        format!("{}: `{name}` 은(는) part — 시트에는 frame 만", clip.state),
                    ));
                }
                Some(_) => {}
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINI: &str = "
canvas 1
cell 4 2   // 작은 칸
palette
  K #000000
  W #ffffff
end
frame a
  grid
    KW..
    ..WK
  end
end
frame b = a
  flip_h
  px 0 0 K
end
part dot 1 1
  px 0 0 W
end
sheet
  directions 1
  tinted true
  clip Idle 200 loop
    row a b
  end
end
output
  image out.png
  sheet out.sheet.ron
  image_ref \"\"
end
";

    #[test]
    fn parses_a_small_document() {
        let doc = parse(MINI).unwrap();
        assert_eq!(doc.cell, (4, 2));
        assert_eq!(doc.palette.len(), 2);
        assert_eq!(doc.images.len(), 3);
        assert_eq!(doc.image("b").unwrap().base.as_deref(), Some("a"));
        assert_eq!(doc.image("dot").unwrap().size, (1, 1));
        let sheet = doc.sheet.unwrap();
        assert_eq!((sheet.columns(), sheet.rows()), (2, 1));
        assert_eq!(doc.output.unwrap().image_ref.as_deref(), Some(""));
    }

    fn fails(from: &str, to: &str) -> String {
        let src = MINI.replacen(from, to, 1);
        assert_ne!(src, MINI, "치환할 문자열이 없음: {from}");
        parse(&src).err().map(|e| e.to_string()).unwrap_or_default()
    }

    #[test]
    fn errors_carry_line_numbers() {
        let e = fails("px 0 0 K", "px 0 0 Q");
        assert!(e.starts_with("16줄") && e.contains("'Q'"), "{e}");
    }

    #[test]
    fn ramps_group_and_generate_colors() {
        let doc = parse(&MINI.replacen(
            "  W #ffffff\nend",
            "  W #ffffff\n  ramp mono WK\n  ramp skin abc from #e0a080 hue 12\nend",
            1,
        ))
        .unwrap();
        assert_eq!(doc.ramp("mono").unwrap().keys, b"WK");
        let skin: Vec<_> = b"abc"
            .iter()
            .map(|&k| doc.palette.get(k).unwrap())
            .collect();
        assert_eq!(skin[1].to_string(), "#e0a080", "가운데가 기준색");
        let l: Vec<f32> = skin.iter().map(|c| c.oklab().l).collect();
        assert!(l[0] > l[1] && l[1] > l[2], "밝은 것부터: {l:?}");

        assert!(fails("  W #ffffff\nend", "  W #ffffff\n  ramp bad WQ\nend").contains("'Q'"));
        assert!(fails("  px 0 0 K", "  shade nope 1").contains("nope"));
        assert!(fails("  W #ffffff\nend", "  W #ffffff\n  use x.palette\nend").contains("파일"));
    }

    #[test]
    fn shared_palette_is_used_relative_to_the_file() {
        let sprites = Path::new(env!("CARGO_MANIFEST_DIR")).join("sprites");
        let src = MINI.replacen(
            "  W #ffffff\n",
            "  W #ffffff\n  use ../palettes/common.palette\n",
            1,
        );
        let doc = parse_in(&src, Some(sprites)).unwrap();
        assert_eq!(doc.ramp("skin").unwrap().keys, b"12345");
        assert!(doc.palette.contains(b'!'));
    }

    #[test]
    fn frames_expand_per_frame_values() {
        let src = MINI.replacen(
            "part dot 1 1",
            "frames f 3 = [a b a]\n  px [0 1 2] 1 K\n  swap W K\nend\npart dot 1 1",
            1,
        );
        let doc = parse(&src).unwrap();
        let f: Vec<_> = (0..3)
            .map(|i| doc.image(&format!("f{i}")).unwrap())
            .collect();
        assert_eq!(f[1].base.as_deref(), Some("b"));
        assert_eq!(
            f[2].ops[0].op,
            Op::Px {
                x: 2,
                y: 1,
                c: b'K'
            }
        );
        assert_eq!(
            f[0].ops[1].op,
            Op::Swap {
                from: b'W',
                to: b'K',
                region: None
            }
        );

        let bad = MINI.replacen(
            "part dot 1 1",
            "frames f 3\n  px [0 1] 1 K\nend\npart dot 1 1",
            1,
        );
        assert!(parse(&bad).unwrap_err().to_string().contains("프레임 수 3"));
    }

    #[test]
    fn rejects_bad_documents() {
        assert!(fails("canvas 1", "canvas 2").contains("버전"));
        assert!(fails("    ..WK", "    ..W").contains("길이"));
        assert!(fails("    ..WK\n", "").contains("4x2"), "grid 크기");
        assert!(fails("frame b = a", "frame b = zz").contains("zz"));
        assert!(fails("row a b", "row a dot").contains("part"));
        assert!(fails("row a b", "row a missing").contains("missing"));
        assert!(fails("clip Idle", "clip Run").contains("Run"));
        assert!(fails("directions 1", "directions 2").contains("row"));
        assert!(fails("frame b = a", "frame a = a").contains("두 번"));
        assert!(fails("  W #ffffff", "  W #ffffff\n  W #000000").contains("두 번"));
        assert!(fails("  image_ref \"\"\nend", "  image_ref \"\"\n").contains("닫히지"));
        assert!(
            fails("  px 0 0 K", "  px 0 0 ~").contains("'~'"),
            "~ 는 블록 밖에서 못 씀"
        );
    }
}
