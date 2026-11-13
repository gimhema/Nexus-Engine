//! 확대 미리보기 — 사람과 Claude 가 눈으로 확인하는 그림.
//!
//! 시트와 달리 **투명 자리를 배경색으로 채우고**, 칸 사이에 틈을 두고, 원하면 픽셀 격자를 긋는다.
//! 격자는 8픽셀마다 밝게 그어 좌표를 세기 쉽게 한다 (`patch 8 16` 같은 좌표를 그림에서 바로 읽는다).
//!
//! 확대 그림만 보면 놓치는 것을 보는 방법들:
//! - [`Mode::Value`] 명도만 — 덩어리끼리 흑백에서 갈리는가
//! - [`Mode::Silhouette`] 실루엣만 — 모양만으로 누군지·어느 방향인지 읽히는가
//! - [`with_actual`] 1배·2배를 덧붙임 — 게임에서 보이는 크기로 뭉개지지 않는가
//! - `onion` 이전 프레임을 붉은 잔상으로 — 걷기에서 무엇이 얼마나 움직이는가

use std::collections::HashMap;

use crate::bitmap::Bitmap;
use crate::color::{Palette, Rgba};
use crate::doc::Document;
use crate::error::Result;
use crate::export::Rgba8;
use crate::resolve::Resolver;

const BACKGROUND: [u8; 4] = [0x4a, 0x50, 0x60, 0xff];
const GUTTER: [u8; 4] = [0x16, 0x18, 0x1d, 0xff];

const ONION: [u8; 4] = [0xe0, 0x50, 0x50, 0xff];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Color,
    /// OKLab 명도를 회색으로.
    Value,
    /// 불투명 픽셀을 모두 한 색으로.
    Silhouette,
}

#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub scale: u32,
    pub grid: bool,
    pub mode: Mode,
    /// 같은 행의 이전 프레임(첫 프레임은 마지막 프레임)을 빈 자리에 붉은 잔상으로.
    pub onion: bool,
}

impl Options {
    #[must_use]
    pub fn new(scale: u32) -> Self {
        Self {
            scale,
            grid: false,
            mode: Mode::Color,
            onion: false,
        }
    }
}

/// 그림 이름 → 표시할 픽셀과 색. `lint` 결과를 그림 위에 테두리로 보여 준다.
pub type Marks = HashMap<String, Vec<(i32, i32, [u8; 4])>>;

/// 배치(행마다 이름 목록)대로 그린다. `None` 은 빈 칸.
pub fn render(doc: &Document, layout: &[Vec<Option<String>>], opt: Options) -> Result<Rgba8> {
    render_marked(doc, layout, opt, &Marks::new())
}

/// [`render`] 에 표시를 얹는다. 표시는 확대한 픽셀의 테두리라 아래 색이 그대로 보인다.
pub fn render_marked(
    doc: &Document,
    layout: &[Vec<Option<String>>],
    opt: Options,
    marks: &Marks,
) -> Result<Rgba8> {
    let mut r = Resolver::new(doc);
    let mut rows: Vec<Vec<Option<Bitmap>>> = Vec::new();
    for row in layout {
        let mut out = Vec::new();
        for name in row {
            out.push(match name {
                Some(n) => Some(r.get(n)?),
                None => None,
            });
        }
        rows.push(out);
    }

    let s = opt.scale.max(1);
    let gap = (s / 2).max(2);
    // 빈 칸은 칸 크기로 자리만 잡는다.
    let size = |b: &Option<Bitmap>| b.as_ref().map_or(doc.cell, |b| (b.width(), b.height()));
    let row_h: Vec<u32> = rows
        .iter()
        .map(|r| r.iter().map(|b| size(b).1).max().unwrap_or(0))
        .collect();
    let row_w: Vec<u32> = rows
        .iter()
        .map(|r| r.iter().map(|b| size(b).0 * s + gap).sum::<u32>() + gap)
        .collect();
    let width = row_w.iter().copied().max().unwrap_or(gap);
    let height = row_h.iter().map(|h| h * s + gap).sum::<u32>() + gap;

    let mut img = Rgba8::new(width, height);
    img.data
        .chunks_exact_mut(4)
        .for_each(|p| p.copy_from_slice(&GUTTER));

    let mut y = gap;
    for ((row, h), names) in rows.iter().zip(&row_h).zip(layout) {
        let mut x = gap;
        for (i, (b, name)) in row.iter().zip(names).enumerate() {
            let (w, _) = size(b);
            if let Some(b) = b {
                let ghost = if opt.onion && row.len() > 1 {
                    row[(i + row.len() - 1) % row.len()].as_ref()
                } else {
                    None
                };
                draw_scaled(&mut img, b, ghost, &doc.palette, x, y, opt);
                if let Some(list) = name.as_ref().and_then(|n| marks.get(n)) {
                    draw_marks(&mut img, list, (b.width(), b.height()), x, y, s);
                }
            }
            x += w * s + gap;
        }
        y += h * s + gap;
    }
    Ok(img)
}

fn draw_marks(
    img: &mut Rgba8,
    list: &[(i32, i32, [u8; 4])],
    size: (u32, u32),
    ox: u32,
    oy: u32,
    s: u32,
) {
    for &(px, py, c) in list {
        if px < 0 || py < 0 || px as u32 >= size.0 || py as u32 >= size.1 {
            continue;
        }
        let (bx, by) = (ox + px as u32 * s, oy + py as u32 * s);
        for i in 0..s {
            for (x, y) in [
                (bx + i, by),
                (bx + i, by + s - 1),
                (bx, by + i),
                (bx + s - 1, by + i),
            ] {
                img.put(x, y, c);
            }
        }
    }
}

fn draw_scaled(
    img: &mut Rgba8,
    bm: &Bitmap,
    ghost: Option<&Bitmap>,
    pal: &Palette,
    ox: u32,
    oy: u32,
    opt: Options,
) {
    let s = opt.scale.max(1);
    let grid = opt.grid && s >= 4;
    for py in 0..bm.height() {
        for px in 0..bm.width() {
            let c = bm.get(px as i32, py as i32).and_then(|c| pal.get(c));
            let ghosted = ghost
                .and_then(|g| g.get(px as i32, py as i32))
                .and_then(|c| pal.get(c))
                .is_some_and(|c| c.normalized().0[3] > 0);
            let base = match c.map(|c| c.normalized().0) {
                Some(c) if c[3] > 0 => blend(BACKGROUND, shown(c, opt.mode)),
                _ if ghosted => mix(BACKGROUND, ONION, 0.35),
                _ => BACKGROUND,
            };
            for sy in 0..s {
                for sx in 0..s {
                    let mut c = base;
                    if grid && (sx == 0 || sy == 0) {
                        let major = (sx == 0 && px % 8 == 0) || (sy == 0 && py % 8 == 0);
                        c = if major {
                            mix(c, [255, 255, 255, 255], 0.45)
                        } else {
                            mix(c, [0, 0, 0, 255], 0.22)
                        };
                    }
                    img.put(ox + px * s + sx, oy + py * s + sy, c);
                }
            }
        }
    }
}

/// 보기 방식에 따른 색.
fn shown(c: [u8; 4], mode: Mode) -> [u8; 4] {
    match mode {
        Mode::Color => c,
        Mode::Value => {
            let v = (Rgba(c).oklab().l.clamp(0.0, 1.0) * 255.0).round() as u8;
            [v, v, v, c[3]]
        }
        Mode::Silhouette => [0x12, 0x12, 0x16, c[3].max(0xc0)],
    }
}

/// 같은 배치를 1배·2배로도 그려 아래에 붙인다 — 게임에서 보이는 크기.
pub fn with_actual(
    main: Rgba8,
    doc: &Document,
    layout: &[Vec<Option<String>>],
    opt: Options,
) -> Result<Rgba8> {
    let small = |scale| {
        render(
            doc,
            layout,
            Options {
                scale,
                grid: false,
                ..opt
            },
        )
    };
    Ok(stack(&[main, small(2)?, small(1)?]))
}

/// 그림들을 위에서 아래로 붙인다. 빈 곳은 틈 색.
fn stack(parts: &[Rgba8]) -> Rgba8 {
    let w = parts.iter().map(|p| p.width).max().unwrap_or(0);
    let h = parts.iter().map(|p| p.height).sum();
    let mut out = Rgba8::new(w, h);
    out.data
        .chunks_exact_mut(4)
        .for_each(|p| p.copy_from_slice(&GUTTER));
    let mut oy = 0;
    for p in parts {
        for y in 0..p.height {
            for x in 0..p.width {
                out.put(x, oy + y, p.at(x, y));
            }
        }
        oy += p.height;
    }
    out
}

/// 반투명 색을 배경 위에 얹는다.
fn blend(bg: [u8; 4], fg: [u8; 4]) -> [u8; 4] {
    mix(bg, [fg[0], fg[1], fg[2], 255], f32::from(fg[3]) / 255.0)
}

fn mix(a: [u8; 4], b: [u8; 4], t: f32) -> [u8; 4] {
    let m = |x: u8, y: u8| (f32::from(x) * (1.0 - t) + f32::from(y) * t).round() as u8;
    [m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2]), 255]
}

/// 시트 배치 그대로 — 클립 순서, 방향별 행.
#[must_use]
pub fn sheet_layout(doc: &Document) -> Option<Vec<Vec<Option<String>>>> {
    let sheet = doc.sheet.as_ref()?;
    Some(
        sheet
            .clips
            .iter()
            .flat_map(|c| c.rows.iter())
            .map(|r| r.iter().cloned().map(Some).collect())
            .collect(),
    )
}

/// 문서의 모든 그림을 `per_row` 개씩.
#[must_use]
pub fn all_layout(doc: &Document, per_row: usize) -> Vec<Vec<Option<String>>> {
    doc.images
        .chunks(per_row.max(1))
        .map(|c| c.iter().map(|i| Some(i.name.clone())).collect())
        .collect()
}
