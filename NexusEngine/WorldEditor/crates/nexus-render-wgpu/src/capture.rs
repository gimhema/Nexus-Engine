//! 스왑체인 이미지를 CPU 로 읽어 오는 화면 캡처.
//!
//! 흐름:
//!
//! ```text
//! end_frame 안 (present 전)  : 서피스 텍스처 → MAP_READ 버퍼 복사를 인코더에 기록
//! queue.submit / present     : 복사가 GPU 에서 실행된다
//! finish()                   : 버퍼 매핑 → 행 패딩 제거 → BGRA 면 RGBA 로 뒤집기
//! ```
//!
//! 스왑체인 포맷은 sRGB 이므로 읽어 온 바이트가 이미 sRGB 로 인코딩돼 있다.
//! 그대로 PNG 에 쓰면 화면과 같은 색이 나온다.

use std::time::Duration;

use nexus_render::{Capture, RenderError};

/// GPU 가 복사를 끝내기를 기다리는 최대 시간.
const MAP_TIMEOUT: Duration = Duration::from_secs(5);

/// 인코더에 기록된, 아직 읽지 않은 캡처.
#[derive(Debug)]
pub(crate) struct PendingCapture {
    buffer: wgpu::Buffer,
    width: u32,
    height: u32,
    padded_bytes_per_row: u32,
    bgra: bool,
}

/// 이 포맷을 캡처할 수 있으면 `Some(is_bgra)`.
pub(crate) fn supported_format(format: wgpu::TextureFormat) -> Option<bool> {
    use wgpu::TextureFormat as F;
    match format {
        F::Rgba8Unorm | F::Rgba8UnormSrgb => Some(false),
        F::Bgra8Unorm | F::Bgra8UnormSrgb => Some(true),
        _ => None,
    }
}

/// 복사 명령을 인코더에 기록한다. `present` 전에 호출해야 한다.
pub(crate) fn record(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    format: wgpu::TextureFormat,
) -> Result<PendingCapture, RenderError> {
    let bgra = supported_format(format).ok_or_else(|| {
        RenderError::FrameFailed(format!("캡처를 지원하지 않는 서피스 포맷: {format:?}"))
    })?;

    let width = texture.width();
    let height = texture.height();

    // 버퍼 복사는 행 길이가 256 바이트의 배수여야 한다. 남는 부분은 나중에 버린다.
    let unpadded = width * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bytes_per_row = unpadded.div_ceil(align) * align;

    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("nexus-capture"),
        size: u64::from(padded_bytes_per_row) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );

    Ok(PendingCapture {
        buffer,
        width,
        height,
        padded_bytes_per_row,
        bgra,
    })
}

impl PendingCapture {
    /// GPU 복사 완료를 기다려 픽셀을 꺼낸다. `queue.submit` 이후에 호출한다.
    pub(crate) fn finish(self, device: &wgpu::Device) -> Result<Capture, RenderError> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                // 수신 측이 이미 사라졌으면 결과를 버린다 — 타임아웃 경로.
                let _ = tx.send(result);
            });

        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(MAP_TIMEOUT),
            })
            .map_err(|e| RenderError::FrameFailed(format!("캡처 대기 실패: {e}")))?;

        rx.recv_timeout(MAP_TIMEOUT)
            .map_err(|_| RenderError::FrameFailed(String::from("캡처 매핑 시간 초과")))?
            .map_err(|e| RenderError::FrameFailed(format!("캡처 매핑 실패: {e}")))?;

        let rgba = {
            let view = self
                .buffer
                .slice(..)
                .get_mapped_range()
                .map_err(|e| RenderError::FrameFailed(format!("캡처 범위 접근 실패: {e}")))?;
            unpad_rows(
                &view,
                self.width,
                self.height,
                self.padded_bytes_per_row,
                self.bgra,
            )
        };
        self.buffer.unmap();

        Ok(Capture {
            width: self.width,
            height: self.height,
            rgba,
        })
    }
}

/// 행마다 붙은 정렬 패딩을 제거하고, 필요하면 BGRA → RGBA 로 바꾼다.
fn unpad_rows(data: &[u8], width: u32, height: u32, padded: u32, bgra: bool) -> Vec<u8> {
    let row_len = width as usize * 4;
    let mut out = Vec::with_capacity(row_len * height as usize);

    for row in data.chunks_exact(padded as usize).take(height as usize) {
        let pixels = &row[..row_len];
        if bgra {
            for px in pixels.chunks_exact(4) {
                out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
            }
        } else {
            out.extend_from_slice(pixels);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpad_strips_row_padding() {
        // 폭 2px, 높이 2, 행 길이 8 → 패딩 포함 12바이트/행
        let mut data = Vec::new();
        data.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 0xEE, 0xEE, 0xEE, 0xEE]);
        data.extend_from_slice(&[9, 10, 11, 12, 13, 14, 15, 16, 0xEE, 0xEE, 0xEE, 0xEE]);

        let out = unpad_rows(&data, 2, 2, 12, false);
        assert_eq!(out, (1..=16).collect::<Vec<u8>>());
    }

    #[test]
    fn unpad_swaps_bgra_to_rgba() {
        let data = [10, 20, 30, 255, 0, 0, 0, 0];
        let out = unpad_rows(&data, 1, 1, 8, true);
        assert_eq!(out, [30, 20, 10, 255]);
    }

    #[test]
    fn only_8bit_formats_are_supported() {
        assert_eq!(
            supported_format(wgpu::TextureFormat::Bgra8UnormSrgb),
            Some(true)
        );
        assert_eq!(
            supported_format(wgpu::TextureFormat::Rgba8UnormSrgb),
            Some(false)
        );
        assert_eq!(supported_format(wgpu::TextureFormat::Rgba16Float), None);
    }
}
