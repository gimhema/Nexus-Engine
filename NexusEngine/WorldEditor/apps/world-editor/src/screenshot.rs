//! 스크린샷 — 자동 검증과 수동 저장.
//!
//! # 자동 모드 (사람 없이 화면 확인)
//!
//! ```text
//! NEXUS_SCREENSHOT=out.png cargo run -p world-editor
//! ```
//!
//! 지정한 프레임(기본 10, `NEXUS_SCREENSHOT_FRAME` 으로 변경)을 PNG 로 저장하고 종료한다.
//! egui 는 첫 프레임에 레이아웃을 잡고 두 번째 프레임부터 안정되므로 너무 이르게 찍지 않는다.
//!
//! # 수동 모드
//!
//! 실행 중 F12 → `screenshots/nexus-<시각>.png`.

use std::fs::File;
use std::io::{BufWriter, Error as IoError};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use nexus_render::Capture;

const ENV_PATH: &str = "NEXUS_SCREENSHOT";
const ENV_FRAME: &str = "NEXUS_SCREENSHOT_FRAME";
const DEFAULT_FRAME: u64 = 10;

/// 수동 스크린샷 저장 디렉터리 (작업 디렉터리 기준).
const MANUAL_DIR: &str = "screenshots";

/// 자동 스크린샷 계획.
#[derive(Debug, Clone)]
pub(crate) struct AutoPlan {
    pub(crate) path: PathBuf,
    pub(crate) at_frame: u64,
}

/// 환경변수에서 자동 스크린샷 계획을 읽는다.
pub(crate) fn plan_from_env() -> Option<AutoPlan> {
    let path = std::env::var_os(ENV_PATH).map(PathBuf::from)?;
    let at_frame = std::env::var(ENV_FRAME)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_FRAME)
        .max(1);
    Some(AutoPlan { path, at_frame })
}

/// 수동 스크린샷 경로. 초 단위 시각 + 밀리초로 충돌을 피한다.
pub(crate) fn manual_path() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    Path::new(MANUAL_DIR).join(format!(
        "nexus-{}-{:03}.png",
        now.as_secs(),
        now.subsec_millis()
    ))
}

/// 캡처를 PNG 로 저장한다. 상위 디렉터리가 없으면 만든다.
pub(crate) fn save_png(path: &Path, capture: &Capture) -> Result<(), IoError> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }

    let file = BufWriter::new(File::create(path)?);
    let mut encoder = png::Encoder::new(file, capture.width, capture.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    // 스왑체인이 sRGB 이므로 읽은 바이트도 sRGB 다. 뷰어가 색을 올바르게 해석하도록 표시한다.
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);

    let mut writer = encoder.write_header().map_err(IoError::other)?;
    writer
        .write_image_data(&capture.rgba)
        .map_err(IoError::other)?;
    writer.finish().map_err(IoError::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_round_trip_preserves_pixels() {
        let capture = Capture {
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 128, 255, 200],
        };
        let dir = std::env::temp_dir().join(format!("nexus-shot-test-{}", std::process::id()));
        let path = dir.join("nested/shot.png");

        save_png(&path, &capture).expect("저장 실패");

        let decoder = png::Decoder::new(std::io::BufReader::new(File::open(&path).unwrap()));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();

        assert_eq!((info.width, info.height), (2, 1));
        assert_eq!(&buf[..info.buffer_size()], &capture.rgba[..]);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn manual_path_is_under_screenshots_dir() {
        let p = manual_path();
        assert!(p.starts_with(MANUAL_DIR));
        assert_eq!(p.extension().and_then(|e| e.to_str()), Some("png"));
    }
}
