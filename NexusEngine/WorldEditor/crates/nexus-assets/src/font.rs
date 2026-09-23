//! 비트맵 폰트 — 그림 한 장에서 글자 칸을 잘라 쓴다 (단계 2 P5).
//!
//! HUD 는 게임 런타임에서 돌아야 하므로 egui 를 쓸 수 없다. 글자도 결국 **쿼드**이고,
//! 필요한 것은 "글자 → 아틀라스의 어느 칸인가" 뿐이다.
//!
//! # 격자가 아니라 **구간(run)** 으로 정의한다
//!
//! 받아온 폰트 그림은 균일 격자가 아니다 — 알파벳은 8×16 칸에 있는데 숫자는 8×8 칸으로
//! 오른쪽 구석에 따로 박혀 있다. 그래서 [`GridAtlas`](crate::GridAtlas) 대신
//! **`(글자들, 시작 픽셀, 칸 크기)`** 를 여러 번 등록하는 방식을 쓴다. 정의는 데이터로 둔다.
//!
//! 고정폭으로 그린다 (`advance`) — 픽셀 폰트 HUD 에서는 이것이 자연스럽고, 자간 표를
//! 따로 두지 않아도 된다.

use std::collections::BTreeMap;

use nexus_core::Vec2;
use nexus_render::UvRect;

/// 글자 하나 — 아틀라스에서 잘라 쓸 사각형과 픽셀 크기.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    pub uv: UvRect,
    /// 픽셀 크기 — 그릴 때의 쿼드 크기 (배율은 부르는 쪽이 곱한다).
    pub size: (u32, u32),
}

/// 비트맵 폰트 정의 하나.
#[derive(Clone, Debug)]
pub struct BitmapFont {
    image: (u32, u32),
    glyphs: BTreeMap<char, Glyph>,
    advance: u32,
    line_height: u32,
}

/// 폰트를 만들 때 생기는 오류.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontError {
    /// 그림 크기가 0 이다.
    EmptyImage,
    /// 칸 크기가 0 이다.
    EmptyCell,
    /// 구간이 그림 밖으로 나간다 — 벗어난 UV 는 엉뚱한 칸을 비춘다.
    OutsideImage { chars: String, x: u32, y: u32 },
    /// 같은 글자를 두 번 정의했다.
    Duplicate(char),
}

impl core::fmt::Display for FontError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyImage => write!(f, "폰트 그림 크기가 0"),
            Self::EmptyCell => write!(f, "칸 크기가 0"),
            Self::OutsideImage { chars, x, y } => {
                write!(f, "'{chars}' 구간({x}, {y})이 그림 밖으로 나감")
            }
            Self::Duplicate(c) => write!(f, "글자 '{c}' 가 두 번 정의됨"),
        }
    }
}

impl std::error::Error for FontError {}

impl BitmapFont {
    /// 빈 폰트. `advance` 는 글자 하나가 차지하는 가로 폭, `line_height` 는 줄 높이 (픽셀).
    ///
    /// # Errors
    /// 그림이나 칸 크기가 0 이면 만들지 않는다.
    pub fn new(image: (u32, u32), advance: u32, line_height: u32) -> Result<Self, FontError> {
        if image.0 == 0 || image.1 == 0 {
            return Err(FontError::EmptyImage);
        }
        if advance == 0 || line_height == 0 {
            return Err(FontError::EmptyCell);
        }
        Ok(Self {
            image,
            glyphs: BTreeMap::new(),
            advance,
            line_height,
        })
    }

    /// `chars` 를 `(x, y)` 부터 `size` 칸 간격으로 **가로로** 등록한다.
    ///
    /// # Errors
    /// 칸이 그림 밖으로 나가거나 같은 글자를 두 번 등록하면 오류다.
    pub fn add_run(
        &mut self,
        chars: &str,
        x: u32,
        y: u32,
        size: (u32, u32),
    ) -> Result<(), FontError> {
        if size.0 == 0 || size.1 == 0 {
            return Err(FontError::EmptyCell);
        }
        let (iw, ih) = self.image;
        for (i, c) in chars.chars().enumerate() {
            let left = x + size.0 * u32::try_from(i).unwrap_or(u32::MAX);
            if left + size.0 > iw || y + size.1 > ih {
                return Err(FontError::OutsideImage {
                    chars: chars.to_owned(),
                    x: left,
                    y,
                });
            }
            let glyph = Glyph {
                uv: UvRect {
                    min: Vec2::new(left as f32 / iw as f32, y as f32 / ih as f32),
                    max: Vec2::new(
                        (left + size.0) as f32 / iw as f32,
                        (y + size.1) as f32 / ih as f32,
                    ),
                },
                size,
            };
            if self.glyphs.insert(c, glyph).is_some() {
                return Err(FontError::Duplicate(c));
            }
        }
        Ok(())
    }

    /// 글자 하나. 없는 글자는 `None` — 부르는 쪽이 건너뛴다 (빈칸처럼 보인다).
    #[must_use]
    pub fn glyph(&self, c: char) -> Option<Glyph> {
        self.glyphs.get(&c).copied()
    }

    /// 등록된 글자 수.
    #[must_use]
    pub fn len(&self) -> usize {
        self.glyphs.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    #[must_use]
    pub fn advance(&self) -> u32 {
        self.advance
    }

    #[must_use]
    pub fn line_height(&self) -> u32 {
        self.line_height
    }

    /// 글자 수 × `advance` — 한 줄만 다룬다 (HUD 문구는 한 줄이다).
    #[must_use]
    pub fn width(&self, text: &str) -> u32 {
        self.advance
            .saturating_mul(u32::try_from(text.chars().count()).unwrap_or(u32::MAX))
    }

    /// `(왼쪽 오프셋, 글자)` — 없는 글자는 자리만 차지하고 건너뛴다.
    pub fn layout<'a>(&'a self, text: &'a str) -> impl Iterator<Item = (u32, Glyph)> + 'a {
        text.chars().enumerate().filter_map(move |(i, c)| {
            let x = self.advance * u32::try_from(i).unwrap_or(u32::MAX);
            self.glyph(c).map(|g| (x, g))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font() -> BitmapFont {
        BitmapFont::new((240, 144), 8, 16).unwrap()
    }

    #[test]
    fn a_run_cuts_cells_left_to_right() {
        let mut f = font();
        f.add_run("Aa", 0, 0, (8, 16)).unwrap();
        let a = f.glyph('A').unwrap();
        assert_eq!(a.uv.min, Vec2::ZERO);
        assert_eq!(a.uv.max, Vec2::new(8.0 / 240.0, 16.0 / 144.0));
        assert_eq!(f.glyph('a').unwrap().uv.min, Vec2::new(8.0 / 240.0, 0.0));
        assert_eq!(f.glyph('B'), None, "등록하지 않은 글자는 없다");
    }

    #[test]
    fn runs_with_different_cell_sizes_live_in_one_font() {
        // 받아온 폰트가 실제로 이렇다 — 글자는 8×16, 숫자는 8×8.
        let mut f = font();
        f.add_run("AaBb", 0, 0, (8, 16)).unwrap();
        f.add_run("012", 216, 0, (8, 8)).unwrap();
        assert_eq!(f.glyph('0').unwrap().size, (8, 8));
        assert_eq!(f.glyph('A').unwrap().size, (8, 16));
        assert_eq!(f.len(), 7);
    }

    #[test]
    fn a_run_outside_the_image_is_refused() {
        let mut f = font();
        // 30칸이 정확히 들어간다 (240 / 8). 31칸째는 밖이다.
        let thirty: String = ('a'..).take(30).collect();
        assert!(
            f.add_run(&thirty, 0, 0, (8, 16)).is_ok(),
            "30칸은 꼭 맞는다"
        );
        let mut f = font();
        let thirty_one: String = ('a'..).take(31).collect();
        let err = f.add_run(&thirty_one, 0, 0, (8, 16)).unwrap_err();
        assert!(
            matches!(err, FontError::OutsideImage { x: 240, .. }),
            "{err}"
        );
        let mut f = font();
        assert_eq!(
            f.add_run("z", 0, 140, (8, 16)).unwrap_err(),
            FontError::OutsideImage {
                chars: String::from("z"),
                x: 0,
                y: 140
            },
            "아래로 넘치는 것도 막는다"
        );
    }

    #[test]
    fn the_same_character_twice_is_a_mistake() {
        let mut f = font();
        f.add_run("Aa", 0, 0, (8, 16)).unwrap();
        assert_eq!(
            f.add_run("A", 0, 16, (8, 16)).unwrap_err(),
            FontError::Duplicate('A')
        );
    }

    #[test]
    fn layout_skips_unknown_characters_but_keeps_their_place() {
        let mut f = font();
        f.add_run("AB", 0, 0, (8, 16)).unwrap();
        let placed: Vec<u32> = f.layout("A?B").map(|(x, _)| x).collect();
        assert_eq!(placed, [0, 16], "'?' 자리는 비고 B 는 세 번째 자리");
        assert_eq!(f.width("A?B"), 24, "폭은 글자 수로 센다");
    }
}
