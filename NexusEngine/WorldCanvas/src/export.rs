//! 시트 PNG 와 `.sheet.ron` 내보내기, PNG 입출력.
//!
//! `.sheet.ron` 은 WorldEditor `sprites.rs` 의 `SheetFile` 스키마를 그대로 따른다.
//! 배치 규칙도 엔진과 같다 — **행 = 클립 시작 행 + 방향, 열 = 프레임.**
//! 클립은 적은 순서대로 `directions` 행씩 차지한다.

use std::path::{Component, Path, PathBuf};

use crate::bitmap::Bitmap;
use crate::color::Palette;
use crate::doc::{AtlasDef, Document, Placed, SheetDef};
use crate::error::{Error, Result};
use crate::resolve::Resolver;

/// RGBA8 그림.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rgba8 {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Rgba8 {
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0; width as usize * height as usize * 4],
        }
    }

    pub fn put(&mut self, x: u32, y: u32, px: [u8; 4]) {
        if x < self.width && y < self.height {
            let i = (y as usize * self.width as usize + x as usize) * 4;
            self.data[i..i + 4].copy_from_slice(&px);
        }
    }

    #[must_use]
    pub fn at(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        [
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        ]
    }

    /// 팔레트 비트맵을 `(ox, oy)` 에 그린다.
    pub fn draw(&mut self, bm: &Bitmap, pal: &Palette, ox: u32, oy: u32) {
        for y in 0..bm.height() {
            for x in 0..bm.width() {
                let c = bm.get(x as i32, y as i32).and_then(|c| pal.get(c));
                if let Some(c) = c {
                    self.put(ox + x, oy + y, c.normalized().0);
                }
            }
        }
    }
}

/// 비트맵 하나를 RGBA 로 — 뷰어가 그림마다 텍스처를 만들 때.
#[must_use]
pub fn bitmap_rgba(bm: &Bitmap, pal: &Palette) -> Rgba8 {
    let mut out = Rgba8::new(bm.width(), bm.height());
    out.draw(bm, pal, 0, 0);
    out
}

/// 시트 한 장을 그린다.
pub fn build_sheet(doc: &Document) -> Result<Rgba8> {
    let sheet = doc
        .sheet
        .as_ref()
        .ok_or_else(|| Error::new("sheet 블록이 없어 시트를 만들 수 없음"))?;
    let (cw, ch) = doc.cell;
    let mut out = Rgba8::new(sheet.columns() * cw, sheet.rows() * ch);
    let mut r = Resolver::new(doc);
    for (k, clip) in sheet.clips.iter().enumerate() {
        for (d, row) in clip.rows.iter().enumerate() {
            let sheet_row = sheet.clip_row(k) + d as u32;
            for (col, name) in row.iter().enumerate() {
                let bm = r.get(name)?;
                out.draw(&bm, &doc.palette, col as u32 * cw, sheet_row * ch);
            }
        }
    }
    Ok(out)
}

/// 아틀라스 한 장과 그림별 자리.
pub fn build_atlas(doc: &Document, atlas: &AtlasDef) -> Result<(Rgba8, Vec<Placed>)> {
    let (placed, w, h) = atlas.place(doc);
    let mut out = Rgba8::new(w, h);
    let mut r = Resolver::new(doc);
    for p in &placed {
        out.draw(&r.get(&p.name)?, &doc.palette, p.x, p.y);
    }
    Ok((out, placed))
}

/// 결과 그림 — 시트든 아틀라스든.
pub fn build_image(doc: &Document) -> Result<Rgba8> {
    match &doc.atlas {
        Some(atlas) => Ok(build_atlas(doc, atlas)?.0),
        None => build_sheet(doc),
    }
}

/// 모든 그림을 한 번씩 계산해 본다 — `check` 용.
pub fn resolve_all(doc: &Document) -> Result<()> {
    let mut r = Resolver::new(doc);
    for img in &doc.images {
        r.get(&img.name)?;
    }
    Ok(())
}

/// `.sheet.ron` 본문.
#[must_use]
pub fn sheet_ron(doc: &Document, sheet: &SheetDef, image_ref: &str, source: &str) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "// WorldCanvas 가 {source} 에서 만든 파일 — 직접 고치지 말고 원본을 고친 뒤 다시 빌드한다.\n"
    ));
    s.push_str(
        "//\n// 배치: 행 = 클립 시작 행 + 방향, 열 = 프레임. 방향 0 은 +X(동)에서 반시계로 증가.\n",
    );
    s.push_str("(\n    version: 1,\n");
    s.push_str(&format!("    image: {},\n", ron_string(image_ref)));
    s.push_str(&format!("    cell: ({}, {}),\n", doc.cell.0, doc.cell.1));
    s.push_str(&format!("    directions: {},\n", sheet.directions));
    if let Some(rows) = &sheet.direction_rows {
        let list: Vec<String> = rows.iter().map(u32::to_string).collect();
        s.push_str(&format!("    direction_rows: [{}],\n", list.join(", ")));
    }
    if let Some(ppm) = sheet.pixels_per_meter {
        s.push_str(&format!("    pixels_per_meter: {ppm:?},\n"));
    }
    s.push_str(&format!("    tinted: {},\n", sheet.tinted));
    s.push_str("    clips: [\n");
    for (k, clip) in sheet.clips.iter().enumerate() {
        s.push_str(&format!(
            "        (state: {}, row: {}, frames: {}, frame_ms: {}, looping: {}),\n",
            clip.state,
            sheet.clip_row(k),
            clip.frames(),
            clip.frame_ms,
            clip.looping
        ));
    }
    s.push_str("    ],\n)\n");
    s
}

fn ron_string(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// `a/./b/../c` → `a/c` (파일 시스템을 보지 않는다).
#[must_use]
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// `from_dir` 에서 `target` 으로 가는 상대 경로 (`/` 구분). 둘 다 같은 기준이어야 한다.
#[must_use]
pub fn relative(target: &Path, from_dir: &Path) -> String {
    let (target, from_dir) = (normalize(target), normalize(from_dir));
    let t: Vec<Component> = target.components().collect();
    let f: Vec<Component> = from_dir.components().collect();
    let common = t.iter().zip(&f).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec![String::from(".."); f.len() - common];
    parts.extend(
        t[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join("/")
}

pub fn encode_png(img: &Rgba8) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, img.width, img.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc
            .write_header()
            .map_err(|e| Error::new(format!("PNG 인코딩: {e}")))?;
        w.write_image_data(&img.data)
            .map_err(|e| Error::new(format!("PNG 인코딩: {e}")))?;
    }
    Ok(out)
}

/// PNG → RGBA8. 회색조·팔레트·RGB 도 RGBA 로 편다 (nexus-assets 와 같은 규칙).
pub fn decode_png(bytes: &[u8]) -> Result<Rgba8> {
    let err = |e: String| Error::new(format!("PNG 디코딩: {e}"));
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().map_err(|e| err(e.to_string()))?;
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| err(String::from("버퍼 크기를 알 수 없음")))?;
    let mut buf = vec![0; size];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| err(e.to_string()))?;
    let src = &buf[..info.buffer_size()];
    let data = match info.color_type {
        png::ColorType::Rgba => src.to_vec(),
        png::ColorType::Rgb => src
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 0xFF])
            .collect(),
        png::ColorType::GrayscaleAlpha => src
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => src.iter().flat_map(|&g| [g, g, g, 0xFF]).collect(),
        png::ColorType::Indexed => return Err(err(String::from("팔레트가 펼쳐지지 않음"))),
    };
    if data.len() != info.width as usize * info.height as usize * 4 {
        return Err(err(String::from("픽셀 수 불일치")));
    }
    Ok(Rgba8 {
        width: info.width,
        height: info.height,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;

    #[test]
    fn relative_paths() {
        let p = |s: &str| PathBuf::from(s);
        assert_eq!(relative(&p("a/b/x.png"), &p("a/b")), "x.png");
        assert_eq!(relative(&p("a/img/x.png"), &p("a/def")), "../img/x.png");
        assert_eq!(relative(&p("a/./b/../b/x.png"), &p("a/b")), "x.png");
    }

    #[test]
    fn sheet_layout_and_ron_follow_the_engine_rules() {
        let doc = parse(
            "canvas 1
cell 2 1
palette
  A #ff0000
end
frame a
  px 0 0 A
end
frame b = a
  flip_h
end
sheet
  directions 2
  direction_rows 1 0
  pixels_per_meter 16
  clip Idle 100 loop
    row a
    row b
  end
  clip Die 80 once
    row a b a
    row b a b
  end
end
",
        )
        .unwrap();
        let img = build_sheet(&doc).unwrap();
        assert_eq!(
            (img.width, img.height),
            (6, 4),
            "3열(가장 긴 클립) × 4행(클립 2 × 방향 2)"
        );
        assert_eq!(img.at(0, 0), [255, 0, 0, 255]);
        assert_eq!(img.at(1, 1), [255, 0, 0, 255], "b 는 뒤집힌 a");
        assert_eq!(
            img.at(2, 0),
            [0, 0, 0, 0],
            "Idle 은 1프레임 — 나머지 칸은 투명"
        );

        let ron = sheet_ron(&doc, doc.sheet.as_ref().unwrap(), "x.png", "t.canvas");
        assert!(ron.contains("image: \"x.png\""));
        assert!(ron.contains("direction_rows: [1, 0]"));
        assert!(ron.contains("pixels_per_meter: 16.0"));
        assert!(ron.contains("(state: Die, row: 2, frames: 3, frame_ms: 80, looping: false)"));
    }

    #[test]
    fn atlas_packs_rows_left_to_right() {
        let doc = parse(
            "canvas 1
palette
  A #ff0000
end
part big 3 2
  rect 0 0 3 2 A
end
part small 1 1
  px 0 0 A
end
atlas
  row small big
  row big
end
",
        )
        .unwrap();
        let (img, placed) = build_atlas(&doc, doc.atlas.as_ref().unwrap()).unwrap();
        assert_eq!((img.width, img.height), (4, 4));
        let rect = |i: usize| (placed[i].x, placed[i].y, placed[i].w, placed[i].h);
        assert_eq!(rect(0), (0, 0, 1, 1));
        assert_eq!(rect(1), (1, 0, 3, 2));
        assert_eq!(rect(2), (0, 2, 3, 2), "행 높이는 그 행의 가장 큰 그림");
        assert_eq!(img.at(0, 1), [0, 0, 0, 0]);
    }

    #[test]
    fn png_round_trip() {
        let mut img = Rgba8::new(2, 2);
        img.put(1, 0, [1, 2, 3, 4]);
        let back = decode_png(&encode_png(&img).unwrap()).unwrap();
        assert_eq!(back, img);
    }
}
