//! 애셋 — 이미지 디코딩, 스프라이트 아틀라스, 애니메이션 재생.
//!
//! **이 크레이트는 파일 시스템을 건드리지 않는다.** 입력은 항상 바이트 슬라이스이고,
//! 어디서 읽어 왔는지는 앱이 정한다. 그래야 단위 테스트가 가능하고, 나중에
//! 애셋을 아카이브나 네트워크에서 가져오게 되어도 이 크레이트가 바뀌지 않는다.
//!
//! GPU 도 모른다 — [`Image::desc`] 로 [`TextureDesc`] 를 만들어 렌더러에 넘길 뿐이다.

#![forbid(unsafe_code)]

mod anim;
mod font;
mod glyphs;

pub use anim::{AnimState, Clip, SpriteAnimator, SpriteSheet};
pub use font::{BitmapFont, FontError, Glyph};
pub use glyphs::{GlyphError, GlyphSheet, RasterGlyph};

use nexus_core::Vec2;
use nexus_render::{TextureDesc, UvRect};

/// 애셋을 읽지 못한 이유.
#[derive(Debug)]
pub enum AssetError {
    /// 이미지 디코딩 실패.
    Decode(String),
    /// 디코딩은 됐지만 이 엔진이 다루지 못하는 형식.
    Unsupported(String),
    /// 아틀라스 격자가 이미지 크기와 맞지 않음.
    BadGrid(String),
}

impl core::fmt::Display for AssetError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Decode(m) => write!(f, "이미지 디코딩 실패: {m}"),
            Self::Unsupported(m) => write!(f, "지원하지 않는 이미지 형식: {m}"),
            Self::BadGrid(m) => write!(f, "아틀라스 격자 오류: {m}"),
        }
    }
}

impl std::error::Error for AssetError {}

/// CPU 쪽 이미지. 8bit RGBA, sRGB, 좌상단부터 행 우선.
#[derive(Clone)]
pub struct Image {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl core::fmt::Debug for Image {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Image")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("bytes", &self.rgba.len())
            .finish()
    }
}

impl Image {
    /// PNG 바이트를 RGBA8 로 디코딩한다.
    ///
    /// 회색조·팔레트·RGB 입력도 RGBA 로 확장해 받는다 — 스프라이트 작업에서
    /// 툴마다 다른 포맷으로 저장되는 일이 흔하기 때문이다.
    ///
    /// # Errors
    /// 디코딩에 실패하거나 16bit 채널처럼 다루지 않는 형식이면 오류.
    pub fn decode_png(bytes: &[u8]) -> Result<Self, AssetError> {
        // png 0.18 의 Decoder 는 Read + Seek 을 요구한다.
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        // 팔레트를 풀고, 회색조·저비트를 8bit 로 맞추고, tRNS 를 알파 채널로 바꾼다.
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);

        let mut reader = decoder
            .read_info()
            .map_err(|e| AssetError::Decode(e.to_string()))?;

        let mut buf = vec![
            0;
            reader
                .output_buffer_size()
                .ok_or_else(|| AssetError::Decode(String::from(
                    "출력 버퍼 크기를 알 수 없음"
                )))?
        ];
        let info = reader
            .next_frame(&mut buf)
            .map_err(|e| AssetError::Decode(e.to_string()))?;

        if info.bit_depth != png::BitDepth::Eight {
            return Err(AssetError::Unsupported(format!(
                "채널당 8bit 만 지원 (입력: {:?})",
                info.bit_depth
            )));
        }

        let pixels = info.width as usize * info.height as usize;
        let src = &buf[..info.buffer_size()];

        // 어떤 색 타입으로 들어오든 RGBA 로 펴 둔다. 이후 경로가 하나로 유지된다.
        let rgba = match info.color_type {
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
            png::ColorType::Indexed => {
                // EXPAND 를 켰으므로 여기까지 오면 안 된다.
                return Err(AssetError::Unsupported(String::from(
                    "팔레트가 펼쳐지지 않음",
                )));
            }
        };

        if rgba.len() != pixels * 4 {
            return Err(AssetError::Decode(format!(
                "픽셀 수 불일치 — {}x{} 인데 {} 바이트",
                info.width,
                info.height,
                rgba.len()
            )));
        }

        Ok(Self {
            width: info.width,
            height: info.height,
            rgba,
        })
    }

    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// 픽셀을 그대로 받아 이미지를 만든다 — 래스터한 글자 아틀라스처럼 **디코딩 없이**
    /// 만드는 그림에 쓴다. 픽셀 수가 크기와 맞지 않으면 거부한다.
    pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, AssetError> {
        let want = width as usize * height as usize * 4;
        if width == 0 || height == 0 || rgba.len() != want {
            return Err(AssetError::BadGrid(format!(
                "{width}x{height} 그림에 픽셀 {}바이트 (필요: {want})",
                rgba.len()
            )));
        }
        Ok(Self {
            width,
            height,
            rgba,
        })
    }

    /// RGBA 픽셀 — 같은 입력이 같은 그림을 만드는지 보는 테스트가 쓴다.
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.rgba
    }

    /// 격자 계산 테스트 전용 — 디코딩 없이 빈 이미지를 만든다.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn blank_for_test(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            rgba: vec![0; width as usize * height as usize * 4],
        }
    }

    /// 렌더러에 넘길 업로드 서술자.
    #[must_use]
    pub fn desc<'a>(&'a self, label: &'a str) -> TextureDesc<'a> {
        TextureDesc {
            label,
            width: self.width,
            height: self.height,
            rgba: &self.rgba,
        }
    }
}

/// 균일 격자 아틀라스 — 모든 칸이 같은 크기다.
///
/// 스프라이트 시트의 가장 흔한 형태이고, 불규칙 패킹보다 먼저 필요하다.
/// 애니메이션 프레임·방향 정보는 S4 에서 이 위에 올린다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridAtlas {
    image_width: u32,
    image_height: u32,
    cell_width: u32,
    cell_height: u32,
}

impl GridAtlas {
    /// 이미지를 `cell_width` × `cell_height` 칸으로 나눈다.
    ///
    /// # Errors
    /// 칸 크기가 0 이거나 이미지 크기를 정확히 나누지 못하면 [`AssetError::BadGrid`].
    /// 나누어떨어지지 않으면 UV 가 미세하게 어긋나 옆 칸이 비쳐 보이므로 오류로 막는다.
    pub fn new(image: &Image, cell_width: u32, cell_height: u32) -> Result<Self, AssetError> {
        if cell_width == 0 || cell_height == 0 {
            return Err(AssetError::BadGrid(String::from("칸 크기가 0")));
        }
        if !image.width.is_multiple_of(cell_width) || !image.height.is_multiple_of(cell_height) {
            return Err(AssetError::BadGrid(format!(
                "{}x{} 이미지를 {cell_width}x{cell_height} 칸으로 나눌 수 없음",
                image.width, image.height
            )));
        }
        Ok(Self {
            image_width: image.width,
            image_height: image.height,
            cell_width,
            cell_height,
        })
    }

    #[must_use]
    pub fn columns(&self) -> u32 {
        self.image_width / self.cell_width
    }

    #[must_use]
    pub fn rows(&self) -> u32 {
        self.image_height / self.cell_height
    }

    #[must_use]
    pub fn cell_count(&self) -> u32 {
        self.columns() * self.rows()
    }

    /// 칸 하나의 UV 영역. `(0, 0)` 이 좌상단이다.
    ///
    /// 범위를 벗어나면 가장자리 칸으로 잘라 준다 — 프레임 번호가 넘쳐도
    /// 화면이 깨지는 대신 마지막 칸이 보인다.
    ///
    /// 경계값을 정확히 쓰고 여유(인셋)를 두지 않는다. 샘플러가 `Nearest` 이고
    /// 칸 크기가 이미지를 정확히 나누므로, 프래그먼트 중심이 옆 칸 텍셀로 넘어가지 않는다.
    #[must_use]
    pub fn uv(&self, col: u32, row: u32) -> UvRect {
        let col = col.min(self.columns().saturating_sub(1));
        let row = row.min(self.rows().saturating_sub(1));

        let w = self.cell_width as f32 / self.image_width as f32;
        let h = self.cell_height as f32 / self.image_height as f32;
        let min = Vec2::new(col as f32 * w, row as f32 * h);

        UvRect {
            min,
            max: min + Vec2::new(w, h),
        }
    }

    /// 행 우선 순번으로 칸을 고른다. 애니메이션 프레임 번호에 쓴다.
    #[must_use]
    pub fn uv_index(&self, index: u32) -> UvRect {
        let cols = self.columns().max(1);
        self.uv(index % cols, index / cols)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 테스트용 RGBA PNG 를 인코딩한다.
    fn encode_png(width: u32, height: u32, rgba: &[u8], color: png::ColorType) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, width, height);
            encoder.set_color(color);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(rgba).unwrap();
            writer.finish().unwrap();
        }
        out
    }

    fn image_2x1() -> Image {
        let bytes = encode_png(
            2,
            1,
            &[255, 0, 0, 255, 0, 128, 255, 200],
            png::ColorType::Rgba,
        );
        Image::decode_png(&bytes).expect("디코딩 실패")
    }

    #[test]
    fn rgba_png_round_trips() {
        let img = image_2x1();
        assert_eq!((img.width(), img.height()), (2, 1));
        assert_eq!(img.rgba, [255, 0, 0, 255, 0, 128, 255, 200]);
    }

    #[test]
    fn rgb_png_gains_opaque_alpha() {
        let bytes = encode_png(2, 1, &[10, 20, 30, 40, 50, 60], png::ColorType::Rgb);
        let img = Image::decode_png(&bytes).expect("디코딩 실패");
        assert_eq!(img.rgba, [10, 20, 30, 255, 40, 50, 60, 255]);
    }

    #[test]
    fn grayscale_png_expands_to_rgba() {
        let bytes = encode_png(2, 1, &[0, 255], png::ColorType::Grayscale);
        let img = Image::decode_png(&bytes).expect("디코딩 실패");
        assert_eq!(img.rgba, [0, 0, 0, 255, 255, 255, 255, 255]);
    }

    #[test]
    fn garbage_is_rejected_not_panicked() {
        assert!(Image::decode_png(b"not a png at all").is_err());
        assert!(Image::decode_png(&[]).is_err());
    }

    #[test]
    fn desc_matches_image() {
        let img = image_2x1();
        let d = img.desc("test");
        assert_eq!((d.width, d.height), (2, 1));
        assert_eq!(d.rgba.len(), 2 * 4);
    }

    fn blank(width: u32, height: u32) -> Image {
        Image::blank_for_test(width, height)
    }

    #[test]
    fn grid_splits_evenly() {
        let atlas = GridAtlas::new(&blank(128, 128), 32, 32).unwrap();
        assert_eq!((atlas.columns(), atlas.rows()), (4, 4));
        assert_eq!(atlas.cell_count(), 16);
    }

    #[test]
    fn grid_rejects_uneven_split() {
        // 나누어떨어지지 않으면 UV 가 어긋나 옆 칸이 비친다 — 미리 막는다.
        assert!(GridAtlas::new(&blank(100, 128), 32, 32).is_err());
        assert!(GridAtlas::new(&blank(128, 128), 0, 32).is_err());
    }

    #[test]
    fn uv_covers_full_texture_across_all_cells() {
        let atlas = GridAtlas::new(&blank(128, 128), 32, 32).unwrap();

        let first = atlas.uv(0, 0);
        assert!((first.min - Vec2::ZERO).length() < 1e-6);
        assert!((first.max - Vec2::splat(0.25)).length() < 1e-6);

        let last = atlas.uv(3, 3);
        assert!((last.min - Vec2::splat(0.75)).length() < 1e-6);
        assert!((last.max - Vec2::ONE).length() < 1e-6, "{:?}", last.max);
    }

    #[test]
    fn uv_cells_do_not_overlap() {
        let atlas = GridAtlas::new(&blank(128, 128), 32, 32).unwrap();
        let a = atlas.uv(0, 0);
        let b = atlas.uv(1, 0);
        assert!(
            (a.max.x - b.min.x).abs() < 1e-6,
            "칸 경계가 어긋남: {a:?} / {b:?}"
        );
    }

    #[test]
    fn uv_index_is_row_major() {
        let atlas = GridAtlas::new(&blank(128, 128), 32, 32).unwrap();
        assert_eq!(atlas.uv_index(0), atlas.uv(0, 0));
        assert_eq!(atlas.uv_index(3), atlas.uv(3, 0));
        assert_eq!(atlas.uv_index(4), atlas.uv(0, 1));
    }

    #[test]
    fn out_of_range_clamps_instead_of_panicking() {
        let atlas = GridAtlas::new(&blank(128, 128), 32, 32).unwrap();
        assert_eq!(atlas.uv(99, 99), atlas.uv(3, 3));
        assert_eq!(atlas.uv_index(9999), atlas.uv(3, 3));
    }
}
