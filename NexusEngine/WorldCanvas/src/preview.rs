//! 확대 미리보기 — 사람과 Claude 가 눈으로 확인하는 그림.
//!
//! 시트와 달리 **투명 자리를 배경색으로 채우고**, 칸 사이에 틈을 두고, 원하면 픽셀 격자를 긋는다.
//! 격자는 8픽셀마다 밝게 그어 좌표를 세기 쉽게 한다 (`patch 8 16` 같은 좌표를 그림에서 바로 읽는다).

use crate::bitmap::Bitmap;
use crate::color::Palette;
use crate::doc::Document;
use crate::error::Result;
use crate::export::Rgba8;
use crate::resolve::Resolver;

const BACKGROUND: [u8; 4] = [0x4a, 0x50, 0x60, 0xff];
const GUTTER: [u8; 4] = [0x16, 0x18, 0x1d, 0xff];

#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub scale: u32,
    pub grid: bool,
}

/// 배치(행마다 이름 목록)대로 그린다. `None` 은 빈 칸.
pub fn render(doc: &Document, layout: &[Vec<Option<String>>], opt: Options) -> Result<Rgba8> {
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
    for (row, h) in rows.iter().zip(&row_h) {
        let mut x = gap;
        for b in row {
            let (w, _) = size(b);
            if let Some(b) = b {
                draw_scaled(&mut img, b, &doc.palette, x, y, opt);
            }
            x += w * s + gap;
        }
        y += h * s + gap;
    }
    Ok(img)
}

fn draw_scaled(img: &mut Rgba8, bm: &Bitmap, pal: &Palette, ox: u32, oy: u32, opt: Options) {
    let s = opt.scale.max(1);
    let grid = opt.grid && s >= 4;
    for py in 0..bm.height() {
        for px in 0..bm.width() {
            let c = bm.get(px as i32, py as i32).and_then(|c| pal.get(c));
            let base = match c.map(|c| c.normalized().0) {
                Some(c) if c[3] > 0 => blend(BACKGROUND, c),
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
