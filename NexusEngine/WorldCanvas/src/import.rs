//! 기존 시트 PNG → `.canvas` 원본.
//!
//! 이미 있는 그림을 원본 형식으로 옮겨 와서 이어 그리기 위한 것이다. 색마다 팔레트 문자를
//! 하나씩 붙이고, 칸마다 `frame` 을 만든다. 앞서 나온 칸과 같거나 좌우만 뒤집힌 칸은
//! 그림을 다시 적지 않고 `frame b = a` (+ `flip_h`) 로 적는다 — 원본이 짧아지고,
//! 어느 칸이 어느 칸의 파생인지 드러난다.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::bitmap::Bitmap;
use crate::color::{Palette, Rgba, TRANSPARENT};
use crate::doc::{Op, STATES};
use crate::error::{Error, Result};
use crate::export::Rgba8;

/// 팔레트 문자 후보 — 이 순서로 나눠 준다.
const KEYS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!$%&*+?@^";

/// `--clip Idle:4:250:loop` 하나.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipSpec {
    pub state: String,
    pub frames: u32,
    pub frame_ms: u64,
    pub looping: bool,
}

impl ClipSpec {
    pub fn parse(s: &str) -> Result<Self> {
        let usage = || {
            Error::new(format!(
                "--clip <상태>:<프레임 수>:<ms>:loop|once — 받은 값: {s}"
            ))
        };
        let parts: Vec<&str> = s.split(':').collect();
        let [state, frames, ms, mode] = parts.as_slice() else {
            return Err(usage());
        };
        if !STATES.contains(state) {
            return Err(Error::new(format!(
                "상태 `{state}` 는 없음 — {}",
                STATES.join(" / ")
            )));
        }
        let looping = match *mode {
            "loop" => true,
            "once" => false,
            _ => return Err(usage()),
        };
        Ok(Self {
            state: (*state).to_owned(),
            frames: frames.parse().ok().filter(|&n| n > 0).ok_or_else(usage)?,
            frame_ms: ms.parse().ok().filter(|&n| n > 0).ok_or_else(usage)?,
            looping,
        })
    }
}

#[derive(Debug)]
pub struct Options {
    pub cell: (u32, u32),
    pub directions: u32,
    pub clips: Vec<ClipSpec>,
    /// 원본 출처 — 머리 주석에 남긴다.
    pub source: String,
}

/// 방향 이름 — 엔진 순서(0 = 동, 반시계).
fn direction_name(d: u32, n: u32) -> String {
    match n {
        1 => String::new(),
        4 => ["e", "n", "w", "s"][d as usize].to_owned(),
        8 => ["e", "ne", "n", "nw", "w", "sw", "s", "se"][d as usize].to_owned(),
        _ => format!("d{d}"),
    }
}

pub fn import(img: &Rgba8, opt: &Options) -> Result<String> {
    let (cw, ch) = opt.cell;
    if cw == 0 || ch == 0 || !img.width.is_multiple_of(cw) || !img.height.is_multiple_of(ch) {
        return Err(Error::new(format!(
            "그림 {}x{} 를 칸 {cw}x{ch} 로 나눌 수 없음",
            img.width, img.height
        )));
    }
    let (cols, rows) = (img.width / cw, img.height / ch);
    let dirs = opt.directions.max(1);
    let need_rows = opt.clips.len() as u32 * dirs;
    if need_rows > rows {
        return Err(Error::new(format!(
            "클립 {}개 × 방향 {dirs} = {need_rows}행 — 그림은 {rows}행",
            opt.clips.len()
        )));
    }
    if let Some(c) = opt.clips.iter().find(|c| c.frames > cols) {
        return Err(Error::new(format!(
            "{}: 프레임 {} — 그림은 {cols}열",
            c.state, c.frames
        )));
    }

    // 팔레트 — 처음 나온 순서대로 문자를 붙인다.
    let mut palette = Palette::default();
    let mut key_of: BTreeMap<Rgba, u8> = BTreeMap::new();
    for p in img.data.chunks_exact(4) {
        let c = Rgba([p[0], p[1], p[2], p[3]]).normalized();
        if c == Rgba::CLEAR || key_of.contains_key(&c) {
            continue;
        }
        let key = *KEYS.get(key_of.len()).ok_or_else(|| {
            Error::new(format!(
                "색이 {}개를 넘음 — 팔레트 아트가 아닌 것 같다",
                KEYS.len()
            ))
        })?;
        palette.insert(key, c).map_err(Error::new)?;
        key_of.insert(c, key);
    }

    // 칸 이름 — 클립이 덮는 칸은 상태·방향·프레임으로, 나머지는 행·열로.
    let cell_name = |row: u32, col: u32| -> Option<String> {
        let k = row / dirs;
        match opt.clips.get(k as usize) {
            Some(c) if col < c.frames => {
                let d = direction_name(row % dirs, dirs);
                let state = c.state.to_lowercase();
                Some(if d.is_empty() {
                    format!("{state}_{col}")
                } else {
                    format!("{state}_{d}_{col}")
                })
            }
            Some(_) => None,
            None => Some(format!("r{row}c{col}")),
        }
    };

    let mut out = String::new();
    let _ = writeln!(
        out,
        "// {} 에서 가져온 원본 (worldcanvas import).",
        opt.source
    );
    let _ = writeln!(out, "canvas 1\ncell {cw} {ch}\n\npalette");
    for (k, c) in palette.iter() {
        let _ = writeln!(out, "    {} {c}", k as char);
    }
    out.push_str("end\n");

    let mut seen: Vec<(String, Bitmap)> = Vec::new();
    for row in 0..rows {
        for col in 0..cols {
            let mut bm = Bitmap::new(cw, ch);
            for y in 0..ch {
                for x in 0..cw {
                    let p = img.at(col * cw + x, row * ch + y);
                    let c = Rgba(p).normalized();
                    let key = if c == Rgba::CLEAR {
                        TRANSPARENT
                    } else {
                        key_of[&c]
                    };
                    bm.set(x as i32, y as i32, key);
                }
            }
            let empty = bm.pixels().iter().all(|&c| c == TRANSPARENT);
            let Some(name) = cell_name(row, col) else {
                if !empty {
                    let _ = writeln!(
                        out,
                        "\n// 경고: {row}행 {col}열은 클립 밖인데 그림이 있다 — 버렸음"
                    );
                }
                continue;
            };
            if empty && name.starts_with('r') {
                continue; // 클립 밖의 빈 칸은 적지 않는다.
            }

            out.push('\n');
            let mut flipped = bm.clone();
            flipped.flip_h();
            if empty {
                let _ = writeln!(out, "frame {name}\nend");
            } else if let Some((base, _)) = seen.iter().find(|(_, b)| *b == bm) {
                let _ = writeln!(out, "frame {name} = {base}\nend");
            } else if let Some((base, _)) = seen.iter().find(|(_, b)| *b == flipped) {
                let _ = writeln!(out, "frame {name} = {base}\n    flip_h\nend");
            } else {
                let grid = Op::Grid(bm.rows().into_iter().map(String::into_bytes).collect());
                let body = grid.to_string().replace('\n', "\n    ");
                let _ = writeln!(out, "frame {name}\n    {body}\nend");
                seen.push((name, bm));
            }
        }
    }

    if !opt.clips.is_empty() {
        let _ = writeln!(out, "\nsheet\n    directions {dirs}");
        for (k, c) in opt.clips.iter().enumerate() {
            let mode = if c.looping { "loop" } else { "once" };
            let _ = writeln!(out, "    clip {} {} {mode}", c.state, c.frame_ms);
            for d in 0..dirs {
                let row = k as u32 * dirs + d;
                let names: Vec<String> = (0..c.frames)
                    .filter_map(|col| cell_name(row, col))
                    .collect();
                let _ = writeln!(out, "        row {}", names.join(" "));
            }
            out.push_str("    end\n");
        }
        out.push_str("end\n");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::build_sheet;
    use crate::parse::parse;

    #[test]
    fn clip_spec_parsing() {
        let c = ClipSpec::parse("Walk:4:150:loop").unwrap();
        assert_eq!(
            (c.state.as_str(), c.frames, c.frame_ms, c.looping),
            ("Walk", 4, 150, true)
        );
        assert!(ClipSpec::parse("Run:4:150:loop").is_err());
        assert!(ClipSpec::parse("Walk:0:150:loop").is_err());
        assert!(ClipSpec::parse("Walk:4:150").is_err());
    }

    /// 가져온 원본을 다시 빌드하면 같은 그림이어야 한다.
    #[test]
    fn import_then_build_round_trips() {
        let mut img = Rgba8::new(4, 4); // 2×2 칸, 칸당 2×2
        img.put(0, 0, [255, 0, 0, 255]); // 동
        img.put(3, 0, [255, 0, 0, 255]); // 동 프레임 1 = 좌우 반전
        img.put(0, 2, [255, 0, 0, 255]); // 서 프레임 0 = 동 0 과 같음
        img.put(3, 3, [0, 0, 255, 128]);
        img.put(1, 1, [9, 9, 9, 0]); // 알파 0 은 투명으로
        let opt = Options {
            cell: (2, 2),
            directions: 2,
            clips: vec![ClipSpec::parse("Idle:2:100:loop").unwrap()],
            source: String::from("test"),
        };
        let text = import(&img, &opt).unwrap();
        assert!(text.contains("frame idle_d1_0 = idle_d0_0"), "{text}");
        assert!(
            text.contains("frame idle_d0_1 = idle_d0_0\n    flip_h"),
            "{text}"
        );

        let doc = parse(&text).unwrap();
        let mut expected = img.clone();
        expected.put(1, 1, [0, 0, 0, 0]);
        assert_eq!(build_sheet(&doc).unwrap(), expected);
    }
}
