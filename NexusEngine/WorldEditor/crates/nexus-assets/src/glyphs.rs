//! 시스템 폰트 래스터 — TTF 글리프를 아틀라스 한 장으로 구워 낸다 (단계 2 P7-E).
//!
//! 받아온 비트맵 폰트(`BitmapFont`)에는 **한글도 `/` 도 없다.** 한글 UI 를 쓰려면 글자를
//! 그때그때 그려야 하는데, 이 모듈이 그 일을 한다: **글자 목록을 받아** 각 글리프를 픽셀로
//! 래스터해 한 장의 [`Image`] 에 담고, 글자마다 UV·크기·오프셋·전진폭을 기록한다.
//!
//! - **I/O 는 없다.** 폰트 파일 바이트를 받는다 (크레이트 규칙).
//! - 래스터는 `ab_glyph` 가 한다. 이미 egui 가 쓰는 크레이트라 의존성이 늘지 않는다.
//! - 결과 그림은 **흰색 + 알파(커버리지)** 다 — 색은 그릴 때 곱한다(`tint`). 그래서 같은
//!   아틀라스로 흰 글자·회색 글자를 다 쓸 수 있다.
//! - 글자 하나의 **자리**는 `offset` 이다: 줄의 왼쪽 위에서 오른쪽·아래로 얼마나 떨어진
//!   곳에 그림을 놓아야 하는가. 베이스라인 계산을 부르는 쪽이 하지 않게 여기서 흡수한다.
//!
//! ⚠ 아틀라스는 **만들 때 정한 글자만** 담는다. 새 글자가 필요하면 다시 구워 새 텍스처로
//! 올려야 한다 (렌더러에 텍스처 해제 API 가 없어 이전 아틀라스는 그대로 남는다).

use std::collections::BTreeMap;

use ab_glyph::{Font, FontRef, ScaleFont, point};
use nexus_render::UvRect;

use crate::Image;

/// 아틀라스 한 장의 최대 가로 폭 (픽셀). 넘으면 다음 줄로 넘긴다.
const ATLAS_WIDTH: u32 = 1024;
/// 글리프 사이 여백 — 이웃 글자가 비쳐 들어오지 않게 (선형 보간을 쓰지 않아도 1px 는 둔다).
const PAD: u32 = 1;

/// 글자 하나의 아틀라스 정보.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RasterGlyph {
    /// 아틀라스에서의 UV (좌상단 원점).
    pub uv: UvRect,
    /// 그림 크기 (픽셀).
    pub size: (u32, u32),
    /// 줄의 왼쪽 위 기준으로 그림을 놓을 자리 (픽셀, 오른쪽·아래가 +).
    pub offset: (i32, i32),
    /// 다음 글자까지의 전진폭 (픽셀).
    pub advance: u32,
}

/// 래스터한 글자 아틀라스.
#[derive(Clone, Debug)]
pub struct GlyphSheet {
    image: Image,
    glyphs: BTreeMap<char, RasterGlyph>,
    line_height: u32,
    /// 그림이 없는 글자(공백 등)의 전진폭.
    space: u32,
}

/// 래스터 실패 이유.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GlyphError {
    /// 폰트 파일을 읽을 수 없다 (형식이 아니거나 깨졌다).
    BadFont,
    /// 글자 크기가 0 이거나 너무 크다.
    BadSize(u32),
    /// 아틀라스가 너무 커진다 — 글자 수를 줄여야 한다.
    TooManyGlyphs { glyphs: usize, height: u32 },
}

impl std::fmt::Display for GlyphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadFont => write!(f, "폰트 파일을 읽을 수 없음"),
            Self::BadSize(px) => write!(f, "글자 크기 {px}px 는 쓸 수 없음 (1~256)"),
            Self::TooManyGlyphs { glyphs, height } => write!(
                f,
                "글자 {glyphs}개가 아틀라스에 들어가지 않음 (높이 {height}px)"
            ),
        }
    }
}

impl std::error::Error for GlyphError {}

impl GlyphSheet {
    /// 폰트 바이트에서 `chars` 를 `px` 크기로 구워 아틀라스를 만든다.
    ///
    /// 같은 글자가 여러 번 들어와도 한 번만 굽는다 (`BTreeMap` 이라 순서도 고정된다 —
    /// 같은 입력이면 같은 아틀라스가 나온다).
    pub fn rasterize(
        bytes: &[u8],
        px: u32,
        chars: impl IntoIterator<Item = char>,
    ) -> Result<Self, GlyphError> {
        if px == 0 || px > 256 {
            return Err(GlyphError::BadSize(px));
        }
        let font = FontRef::try_from_slice(bytes).map_err(|_| GlyphError::BadFont)?;
        let scaled = font.as_scaled(px as f32);
        let line_height = (scaled.ascent() - scaled.descent() + scaled.line_gap()).ceil() as u32;
        let ascent = scaled.ascent();
        let space = scaled.h_advance(scaled.glyph_id(' ')).round().max(1.0) as u32;

        // 먼저 글리프를 모으고(같은 글자 한 번만), 자리를 정한 뒤 한꺼번에 그린다.
        let wanted: BTreeMap<char, ()> = chars.into_iter().map(|c| (c, ())).collect();
        struct Pending {
            ch: char,
            outline: Option<ab_glyph::OutlinedGlyph>,
            advance: u32,
        }
        let mut pending: Vec<Pending> = Vec::with_capacity(wanted.len());
        for ch in wanted.into_keys() {
            let id = scaled.glyph_id(ch);
            let advance = scaled.h_advance(id).round().max(0.0) as u32;
            let glyph = id.with_scale_and_position(px as f32, point(0.0, ascent));
            pending.push(Pending {
                ch,
                outline: font.outline_glyph(glyph),
                advance,
            });
        }

        // 선반(shelf) 배치 — 줄마다 가장 높은 글리프에 맞춘다.
        let mut glyphs: BTreeMap<char, RasterGlyph> = BTreeMap::new();
        let mut places: Vec<(usize, u32, u32)> = Vec::new(); // (pending 번호, x, y)
        let (mut pen_x, mut pen_y, mut row_h) = (PAD, PAD, 0);
        for (i, p) in pending.iter().enumerate() {
            let Some(outline) = p.outline.as_ref() else {
                continue; // 공백 등 — 그림이 없다
            };
            let b = outline.px_bounds();
            let (w, h) = (b.width().ceil() as u32, b.height().ceil() as u32);
            if w == 0 || h == 0 {
                continue;
            }
            if pen_x + w + PAD > ATLAS_WIDTH {
                pen_x = PAD;
                pen_y += row_h + PAD;
                row_h = 0;
            }
            places.push((i, pen_x, pen_y));
            pen_x += w + PAD;
            row_h = row_h.max(h);
        }
        let height = pen_y + row_h + PAD;
        if height > 4096 {
            return Err(GlyphError::TooManyGlyphs {
                glyphs: pending.len(),
                height,
            });
        }
        let width = ATLAS_WIDTH;
        let mut pixels = vec![0u8; (width * height * 4) as usize];

        for &(i, x, y) in &places {
            let p = &pending[i];
            let outline = p.outline.as_ref().expect("자리를 잡은 것은 그림이 있다");
            let b = outline.px_bounds();
            let (w, h) = (b.width().ceil() as u32, b.height().ceil() as u32);
            outline.draw(|gx, gy, coverage| {
                if gx >= w || gy >= h {
                    return;
                }
                let at = (((y + gy) * width + x + gx) * 4) as usize;
                // 흰색 + 알파(커버리지) — 색은 그릴 때 곱한다.
                let a = (coverage.clamp(0.0, 1.0) * 255.0).round() as u8;
                pixels[at] = 255;
                pixels[at + 1] = 255;
                pixels[at + 2] = 255;
                pixels[at + 3] = a;
            });
            glyphs.insert(
                p.ch,
                RasterGlyph {
                    uv: UvRect::from_pixels(x, y, w, h, (width, height)),
                    size: (w, h),
                    // px_bounds 는 펜 원점(베이스라인 y = ascent) 기준이다 →
                    // 줄의 왼쪽 위 기준으로 바꾸려면 그대로 쓰면 된다 (원점 y 가 0 이므로).
                    offset: (b.min.x.round() as i32, b.min.y.round() as i32),
                    advance: p.advance,
                },
            );
        }
        // 그림이 없는 글자도 전진폭은 기록해 둔다 (공백).
        for p in &pending {
            if p.outline.is_none() {
                glyphs.insert(
                    p.ch,
                    RasterGlyph {
                        uv: UvRect::FULL,
                        size: (0, 0),
                        offset: (0, 0),
                        advance: p.advance.max(space),
                    },
                );
            }
        }

        Ok(Self {
            image: Image::from_rgba(width, height, pixels).expect("크기가 맞는 픽셀"),
            glyphs,
            line_height,
            space,
        })
    }

    /// 아틀라스 그림 — 부르는 쪽이 GPU 에 올린다.
    pub fn image(&self) -> &Image {
        &self.image
    }

    pub fn glyph(&self, ch: char) -> Option<&RasterGlyph> {
        self.glyphs.get(&ch)
    }

    /// 담고 있는 글자 수.
    pub fn len(&self) -> usize {
        self.glyphs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    /// 이 글자들을 다 담고 있는가 — 새로 구워야 하는지 판단에 쓴다.
    pub fn covers(&self, chars: impl IntoIterator<Item = char>) -> bool {
        chars.into_iter().all(|c| self.glyphs.contains_key(&c))
    }

    pub fn line_height(&self) -> u32 {
        self.line_height
    }

    /// 한 줄의 가로 폭 (픽셀). 없는 글자는 공백만큼 자리를 둔다.
    pub fn width(&self, text: &str) -> u32 {
        text.chars()
            .map(|c| self.glyphs.get(&c).map_or(self.space, |g| g.advance))
            .sum()
    }

    /// 한 줄을 왼쪽부터 늘어놓는다 — `(줄 왼쪽에서의 x, 글리프)`.
    /// 아틀라스에 없는 글자는 건너뛰고 자리만 넘긴다.
    pub fn layout<'a>(&'a self, text: &'a str) -> impl Iterator<Item = (i32, &'a RasterGlyph)> {
        let mut x = 0_i32;
        text.chars().filter_map(move |c| {
            let here = x;
            match self.glyphs.get(&c) {
                Some(g) => {
                    x += g.advance as i32;
                    (g.size.0 > 0 && g.size.1 > 0).then_some((here, g))
                }
                None => {
                    x += self.space as i32;
                    None
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 시스템 한글 폰트 — 없으면 테스트를 건너뛴다 (CI 머신에 폰트가 없을 수 있다).
    fn font_bytes() -> Option<Vec<u8>> {
        const CANDIDATES: &[&str] = &[
            "C:/Windows/Fonts/malgun.ttf",
            "/usr/share/fonts/truetype/nanum/NanumGothic.ttf",
            "/usr/share/fonts/nanum/NanumGothic.ttf",
            "C:/Windows/Fonts/arial.ttf",
            "/usr/share/fonts/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        ];
        CANDIDATES.iter().find_map(|p| std::fs::read(p).ok())
    }

    #[test]
    fn a_bad_font_or_size_is_refused() {
        assert_eq!(
            GlyphSheet::rasterize(&[0u8; 32], 16, ['A']).unwrap_err(),
            GlyphError::BadFont
        );
        let Some(bytes) = font_bytes() else { return };
        assert_eq!(
            GlyphSheet::rasterize(&bytes, 0, ['A']).unwrap_err(),
            GlyphError::BadSize(0)
        );
        assert_eq!(
            GlyphSheet::rasterize(&bytes, 999, ['A']).unwrap_err(),
            GlyphError::BadSize(999)
        );
    }

    #[test]
    fn glyphs_land_inside_the_atlas_and_measure_a_line() {
        let Some(bytes) = font_bytes() else { return };
        let sheet = GlyphSheet::rasterize(&bytes, 16, "AB 12".chars()).unwrap();
        assert!(sheet.line_height() >= 16, "줄 높이는 글자 크기 이상");
        assert_eq!(sheet.len(), 5, "같은 글자는 한 번만");

        let image = sheet.image();
        for ch in ['A', 'B', '1', '2'] {
            let g = sheet.glyph(ch).unwrap_or_else(|| panic!("{ch} 글리프"));
            assert!(g.size.0 > 0 && g.size.1 > 0, "{ch} 는 그림이 있다");
            assert!(g.advance > 0, "{ch} 전진폭");
            // UV 가 그림 안에 있어야 한다 — 벗어나면 옆 글자가 비친다.
            assert!(g.uv.min.x >= 0.0 && g.uv.max.x <= 1.0);
            assert!(g.uv.min.y >= 0.0 && g.uv.max.y <= 1.0);
            let right = (g.uv.max.x * image.width() as f32).round() as u32;
            let bottom = (g.uv.max.y * image.height() as f32).round() as u32;
            assert!(right <= image.width() && bottom <= image.height());
        }
        // 공백은 그림이 없고 전진폭만 있다.
        let space = sheet.glyph(' ').unwrap();
        assert_eq!(space.size, (0, 0));
        assert!(space.advance > 0);

        // 폭 = 전진폭의 합. 없는 글자는 공백만큼.
        let sum: u32 = "AB 12"
            .chars()
            .map(|c| sheet.glyph(c).unwrap().advance)
            .sum();
        assert_eq!(sheet.width("AB 12"), sum);
        assert!(sheet.width("없는글자") > 0, "모르는 글자도 자리는 둔다");

        // 늘어놓기 — x 가 앞으로만 간다, 그림 없는 글자는 빠진다.
        let laid: Vec<i32> = sheet.layout("AB 12").map(|(x, _)| x).collect();
        assert_eq!(laid.len(), 4, "공백은 그리지 않는다");
        assert!(laid.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn covers_says_whether_the_atlas_must_be_rebuilt() {
        let Some(bytes) = font_bytes() else { return };
        let sheet = GlyphSheet::rasterize(&bytes, 16, "AB".chars()).unwrap();
        assert!(sheet.covers("BA".chars()));
        assert!(!sheet.covers("ABC".chars()));
    }

    /// 같은 입력이면 같은 아틀라스 — 순서가 흔들리면 스크린샷 비교가 무의미해진다.
    #[test]
    fn the_same_characters_give_the_same_atlas() {
        let Some(bytes) = font_bytes() else { return };
        let a = GlyphSheet::rasterize(&bytes, 16, "BA1".chars()).unwrap();
        let b = GlyphSheet::rasterize(&bytes, 16, "1AB".chars()).unwrap();
        assert_eq!(a.image().pixels(), b.image().pixels());
        assert_eq!(a.glyph('A'), b.glyph('A'));
    }
}
