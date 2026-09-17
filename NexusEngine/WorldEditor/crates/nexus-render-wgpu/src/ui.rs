//! egui 합성 계층 (`ui` 피처).
//!
//! 씬을 먼저 그리고, 그 위에 egui 를 `LoadOp::Load` 로 덧그린다.
//! egui 는 깊이 버퍼를 쓰지 않으므로 씬의 깊이 상태와 무관하다.
//!
//! 에디터 전용이다. 게임 런타임은 이 피처를 끄고 빌드한다.

/// 앱이 쓰는 egui 와 버전을 맞추기 위해 재수출한다.
pub use egui;

/// 한 프레임 분량의 egui 출력.
///
/// `egui::Context::run_ui` 결과를 `tessellate` 한 것을 그대로 담는다.
///
/// **주의:** `textures_delta` 는 반드시 적용되어야 한다. 프레임을 건너뛰어 이 값을
/// 렌더러에 넘기지 못했다면 [`TextureCarry`] 로 다음 프레임에 넘길 것.
/// (적용하지 않고 드롭하면 egui 가 debug 빌드에서 panic 하고, 폰트 텍스처가 영영 올라가지 않는다)
pub struct UiFrame {
    pub primitives: Vec<egui::ClippedPrimitive>,
    pub textures_delta: egui::TexturesDelta,
    /// 논리 포인트 → 물리 픽셀 배율.
    pub pixels_per_point: f32,
}

/// 렌더러에 넘기지 못한 텍스처 변경분을 다음 프레임으로 넘기는 보관함.
///
/// 창이 최소화돼 프레임을 건너뛰는 동안에도 egui 는 폰트 아틀라스 같은 텍스처 변경을
/// 만든다. 이것을 버리면 복귀 후 글자가 깨진다.
#[derive(Default)]
pub struct TextureCarry(egui::TexturesDelta);

impl TextureCarry {
    /// 건너뛴 프레임의 변경분을 보관한다.
    pub fn stash(&mut self, mut frame: UiFrame) {
        self.0.append(std::mem::take(&mut frame.textures_delta));
    }

    /// 보관해 둔 변경분을 이번 프레임 앞에 붙인다.
    pub fn apply_to(&mut self, frame: &mut UiFrame) {
        if self.0.is_empty() {
            return;
        }
        let mut merged = std::mem::take(&mut self.0);
        merged.append(std::mem::take(&mut frame.textures_delta));
        frame.textures_delta = merged;
    }
}

impl core::fmt::Debug for TextureCarry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TextureCarry")
            .field("pending", &(self.0.set.len() + self.0.free.len()))
            .finish()
    }
}

impl Drop for TextureCarry {
    fn drop(&mut self) {
        // 종료 시점에 남은 변경분은 적용할 곳이 없다. 의도적으로 버린다.
        self.0.clear();
    }
}

impl core::fmt::Debug for UiFrame {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("UiFrame")
            .field("primitives", &self.primitives.len())
            .field("pixels_per_point", &self.pixels_per_point)
            .finish_non_exhaustive()
    }
}

pub(crate) struct UiLayer {
    renderer: egui_wgpu::Renderer,
}

impl core::fmt::Debug for UiLayer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("UiLayer").finish_non_exhaustive()
    }
}

impl UiLayer {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        // sRGB 타깃이면 egui-wgpu 가 알맞은 셰이더 진입점을 고른다.
        let renderer =
            egui_wgpu::Renderer::new(device, format, egui_wgpu::RendererOptions::default());
        Self { renderer }
    }

    /// egui 를 인코더에 기록한다.
    ///
    /// 반환된 커맨드 버퍼들은 메인 인코더보다 **먼저** 제출해야 한다
    /// (egui 페인트 콜백이 준비 단계에서 만든 것).
    pub(crate) fn paint(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size_in_pixels: [u32; 2],
        mut frame: UiFrame,
    ) -> Vec<wgpu::CommandBuffer> {
        // egui 0.36 부터 텍스처 하나에 여러 델타가 쌓일 수 있다 — 순서대로 적용한다.
        // `drain` 으로 비워야 한다: 적용하지 않은 델타가 남은 채 드롭되면 egui 가 panic 한다.
        for (id, deltas) in frame.textures_delta.set.drain() {
            for delta in &deltas {
                self.renderer.update_texture(device, queue, id, delta);
            }
        }

        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels,
            pixels_per_point: frame.pixels_per_point,
        };

        let prepared =
            self.renderer
                .update_buffers(device, queue, encoder, &frame.primitives, &screen);

        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("nexus-ui-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        // 씬 위에 덧그린다
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // egui-wgpu 는 'static 수명의 패스를 요구한다. 인코더와의 수명 연결을 끊는다.
            let mut pass = pass.forget_lifetime();
            self.renderer.render(&mut pass, &frame.primitives, &screen);
        }

        // 이미 인코딩된 패스가 텍스처를 붙잡고 있으므로 지금 해제해도 안전하다.
        for id in frame.textures_delta.free.drain() {
            self.renderer.free_texture(&id);
        }

        prepared
    }
}
