//! wgpu 기반 [`Renderer`] 구현.
//!
//! Windows / Linux 에서 wgpu 는 기본적으로 **Vulkan 백엔드**로 동작한다.
//! 즉 Vulkan 을 쓰되 인스턴스·디바이스·스왑체인·동기화 보일러플레이트를
//! 직접 작성하지 않는 것이다. 서피스 생성의 플랫폼 분기도 wgpu 내부에 있으므로
//! 이 크레이트에는 `#[cfg(target_os)]` 가 하나도 없다.
//!
//! # 파이프라인
//!
//! 인스턴스 기반 단색 쿼드 하나뿐이다. 그리드 선도 얇은 쿼드로 그리므로
//! 파이프라인이 더 필요하지 않다. 정점 버퍼는 없고 `vertex_index` 로 쿼드를 만든다.
//!
//! **깊이 버퍼를 2D 단계에서도 켜 둔다.** 지금은 Z 로 그리기 순서를 정하는 용도지만,
//! M8 에서 원근 투영으로 바뀔 때 파이프라인을 다시 만들 필요가 없어진다.

#![forbid(unsafe_code)]

mod capture;
#[cfg(feature = "ui")]
mod ui;

#[cfg(feature = "ui")]
pub use ui::{TextureCarry, UiFrame, egui};

use bytemuck::{Pod, Zeroable};
use nexus_core::Mat4;
use nexus_render::{
    Capture, FrameStatus, RenderBackend, RenderCommand, RenderDeviceInfo, RenderError, Renderer,
};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

/// 씬을 그릴 화면 사각형 (물리 픽셀).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ViewportRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl ViewportRect {
    /// 서피스 범위 안으로 자른다. 벗어난 스시저 사각형은 wgpu 검증 오류가 된다.
    fn clamped(self, surface_w: u32, surface_h: u32) -> Self {
        let x = self.x.min(surface_w);
        let y = self.y.min(surface_h);
        Self {
            x,
            y,
            width: self.width.min(surface_w - x),
            height: self.height.min(surface_h - y),
        }
    }

    fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// 깊이 버퍼 포맷. 스텐실 없이 32bit float — 데스크톱 어디서나 지원된다.
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// 인스턴스 버퍼 초기 용량 (쿼드 개수).
const INITIAL_INSTANCE_CAPACITY: usize = 4096;

/// GPU 로 넘어가는 쿼드 인스턴스. WGSL `Instance` 구조체와 레이아웃이 일치해야 한다.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct QuadInstance {
    center: [f32; 2],
    size: [f32; 2],
    z: f32,
    /// Z 축 회전 (라디안, 반시계 +).
    rotation: f32,
    // 패딩을 넣지 않는다. 정점 버퍼에는 유니폼 버퍼(std140) 같은 정렬 규칙이 없고,
    // `vertex_attr_array!` 는 필드를 빈틈 없이 이어 붙인 오프셋(0, 8, 16, 20, 24)으로 읽는다.
    // 여기에 패딩을 넣으면 color 를 엉뚱한 위치에서 읽는다 — 실제로 겪은 버그다.
    /// 선형 색 공간 RGBA (sRGB 입력을 변환해 넣는다).
    color: [f32; 4],
}

/// 레이아웃이 셰이더 속성 오프셋과 맞는지 컴파일 타임에 확인한다.
const _: () = {
    assert!(core::mem::size_of::<QuadInstance>() == 40);
    assert!(core::mem::offset_of!(QuadInstance, center) == 0);
    assert!(core::mem::offset_of!(QuadInstance, size) == 8);
    assert!(core::mem::offset_of!(QuadInstance, z) == 16);
    assert!(core::mem::offset_of!(QuadInstance, rotation) == 20);
    assert!(core::mem::offset_of!(QuadInstance, color) == 24);
};

/// sRGB 한 채널을 선형으로 변환한다.
///
/// 스왑체인이 sRGB 포맷이면 GPU 가 출력 시 선형 → sRGB 변환을 한다. 따라서 셰이더에는
/// 선형 값을 넘겨야 화면에 의도한 색이 나온다. egui 도 같은 규약을 따르므로
/// 씬과 UI 의 색이 일치한다.
fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// sRGB RGBA(알파는 선형 그대로)를 선형 RGBA 로.
fn linear_rgba(srgb: [f32; 4]) -> [f32; 4] {
    [
        srgb_to_linear(srgb[0]),
        srgb_to_linear(srgb[1]),
        srgb_to_linear(srgb[2]),
        srgb[3],
    ]
}

/// 프레임 중간 상태 — `begin_frame` 과 `end_frame` 사이에만 존재한다.
struct Frame {
    surface_texture: wgpu::SurfaceTexture,
    view: wgpu::TextureView,
    encoder: wgpu::CommandEncoder,
    clear: wgpu::Color,
    view_proj: Mat4,
    viewport: Option<ViewportRect>,
    quads: Vec<QuadInstance>,
    #[cfg(feature = "ui")]
    ui: Option<UiFrame>,
}

impl core::fmt::Debug for Frame {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Frame")
            .field("clear", &self.clear)
            .field("quads", &self.quads.len())
            .finish()
    }
}

/// wgpu 렌더러.
#[derive(Debug)]
pub struct WgpuRenderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    info: RenderDeviceInfo,

    pipeline: wgpu::RenderPipeline,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    depth_view: wgpu::TextureView,

    frame: Option<Frame>,
    /// 직전 프레임에 그린 쿼드 수 (디버그용).
    last_quad_count: usize,

    /// 서피스가 `COPY_SRC` 를 지원해 화면 캡처가 가능한가.
    capture_supported: bool,
    capture_requested: bool,
    capture_result: Option<Result<Capture, RenderError>>,

    #[cfg(feature = "ui")]
    ui: ui::UiLayer,
}

impl WgpuRenderer {
    /// 창 핸들로부터 렌더러를 만든다.
    ///
    /// `target` 은 `nexus_platform::WindowTarget` 처럼 `raw-window-handle` 을 구현하고
    /// `'static` 수명을 갖는 타입이어야 한다.
    ///
    /// # Errors
    /// 서피스·어댑터·디바이스 획득에 실패하면 [`RenderError::InitializationFailed`].
    pub fn new<T>(target: T, width: u32, height: u32) -> Result<Self, RenderError>
    where
        T: HasWindowHandle + HasDisplayHandle + Send + Sync + 'static,
    {
        // PRIMARY = Vulkan / DX12 / Metal. GL 백엔드는 제외하므로 display 핸들이 필요 없다.
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::PRIMARY;
        let instance = wgpu::Instance::new(desc);

        let surface = instance
            .create_surface(target)
            .map_err(|e| RenderError::InitializationFailed(format!("서피스 생성: {e}")))?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|e| {
            RenderError::InitializationFailed(format!(
                "적합한 GPU 어댑터 없음 ({e}) — 그래픽 드라이버 또는 Vulkan 로더를 확인하세요"
            ))
        })?;

        let adapter_info = adapter.get_info();

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("nexus-device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| RenderError::InitializationFailed(format!("디바이스 생성: {e}")))?;

        let mut config = surface
            .get_default_config(&adapter, width.max(1), height.max(1))
            .ok_or_else(|| {
                RenderError::InitializationFailed(String::from(
                    "어댑터가 이 서피스를 지원하지 않음",
                ))
            })?;

        // sRGB 포맷을 우선한다. 색 공간이 어긋나면 화면 전체 톤이 뜨거나 어두워진다.
        let caps = surface.get_capabilities(&adapter);
        if let Some(srgb) = caps
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
        {
            config.format = srgb;
        }
        // vsync. 모든 플랫폼에서 항상 지원되는 유일한 모드다.
        config.present_mode = wgpu::PresentMode::Fifo;

        // 화면 캡처를 위해 스왑체인 이미지를 읽을 수 있게 한다.
        // 지원하지 않는 플랫폼에서는 캡처만 비활성화되고 렌더링은 그대로 동작한다.
        let capture_supported = caps.usages.contains(wgpu::TextureUsages::COPY_SRC)
            && capture::supported_format(config.format).is_some();
        if capture_supported {
            config.usage |= wgpu::TextureUsages::COPY_SRC;
        }

        surface.configure(&device, &config);

        // ── 카메라 유니폼 ────────────────────────────────────────────────────
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nexus-camera"),
            size: core::mem::size_of::<[[f32; 4]; 4]>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("nexus-camera-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nexus-camera-bind-group"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        // ── 파이프라인 ───────────────────────────────────────────────────────
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("nexus-quad-shader"),
            // wgpu 는 WGSL 을 naga 로 런타임 컴파일한다.
            // shaderc / SPIR-V 사전 컴파일 단계가 필요 없다.
            source: wgpu::ShaderSource::Wgsl(include_str!("quad.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("nexus-quad-layout"),
            bind_group_layouts: &[Some(&camera_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("nexus-quad-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: core::mem::size_of::<QuadInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x2,  // center
                        1 => Float32x2,  // size
                        2 => Float32,    // z
                        3 => Float32,    // rotation
                        4 => Float32x4,  // color — 오프셋 24 (위 const 단언으로 검증)
                    ],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                // 2D 쿼드는 양면이 다 보여야 한다. 3D 메시가 들어오는 M8 에서 조정한다.
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                // 일반 Z 투영: 카메라에서 멀수록 깊이 값이 크다. 월드 Z 가 큰(= 카메라에 가까운)
                // 쪽이 작은 깊이를 가지므로 LessEqual 로 위에 그려진다.
                // (GreaterEqual 은 reverse-Z 투영용이다 — 섞어 쓰면 먼 것이 이긴다. 실제로 겪은 버그)
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nexus-instances"),
            size: (INITIAL_INSTANCE_CAPACITY * core::mem::size_of::<QuadInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let depth_view = create_depth_view(&device, config.width, config.height);

        let info = RenderDeviceInfo {
            name: adapter_info.name.clone(),
            backend: map_backend(adapter_info.backend),
            is_integrated: adapter_info.device_type == wgpu::DeviceType::IntegratedGpu,
        };

        #[cfg(feature = "ui")]
        let ui = ui::UiLayer::new(&device, config.format);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            info,
            pipeline,
            camera_buffer,
            camera_bind_group,
            instance_buffer,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            depth_view,
            frame: None,
            last_quad_count: 0,
            capture_supported,
            capture_requested: false,
            capture_result: None,
            #[cfg(feature = "ui")]
            ui,
        })
    }

    /// 이번 프레임에 합성할 egui 출력을 넘긴다. `begin_frame` 과 `end_frame` 사이에 호출한다.
    ///
    /// # Errors
    /// 프레임이 열려 있지 않으면 [`RenderError::InvalidFrameState`].
    #[cfg(feature = "ui")]
    pub fn submit_ui(&mut self, ui: UiFrame) -> Result<(), RenderError> {
        let frame = self
            .frame
            .as_mut()
            .ok_or(RenderError::InvalidFrameState("프레임이 열려 있지 않음"))?;
        frame.ui = Some(ui);
        Ok(())
    }

    /// 현재 서피스 크기.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// 직전 프레임에 그린 쿼드 수.
    #[must_use]
    pub fn last_quad_count(&self) -> usize {
        self.last_quad_count
    }

    /// 디바이스가 허용하는 최대 2D 텍스처 한 변 길이. UI 폰트 아틀라스 크기 제한에 쓴다.
    #[must_use]
    pub fn max_texture_side(&self) -> usize {
        self.device.limits().max_texture_dimension_2d as usize
    }

    fn reconfigure(&mut self) {
        self.surface.configure(&self.device, &self.config);
        self.depth_view = create_depth_view(&self.device, self.config.width, self.config.height);
    }

    /// 인스턴스 버퍼가 모자라면 2배씩 키운다.
    fn ensure_instance_capacity(&mut self, needed: usize) {
        if needed <= self.instance_capacity {
            return;
        }
        let mut cap = self.instance_capacity.max(1);
        while cap < needed {
            cap *= 2;
        }
        self.instance_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nexus-instances"),
            size: (cap * core::mem::size_of::<QuadInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.instance_capacity = cap;
    }

    /// 스왑체인 이미지를 획득한다. 재구성이 필요하면 한 번 재시도한다.
    fn acquire(&mut self) -> Result<Option<wgpu::SurfaceTexture>, RenderError> {
        use wgpu::CurrentSurfaceTexture as Cst;

        match self.surface.get_current_texture() {
            Cst::Success(t) => Ok(Some(t)),

            // 획득은 됐지만 서피스 속성과 어긋난다 — 이번 프레임은 그대로 쓰고
            // 다음 프레임을 위해 재구성한다.
            Cst::Suboptimal(t) => {
                self.reconfigure();
                Ok(Some(t))
            }

            // 창 크기 변경·모니터 전환 등으로 무효화됨 — 재구성 후 한 번 더 시도.
            Cst::Outdated | Cst::Lost => {
                self.reconfigure();
                match self.surface.get_current_texture() {
                    Cst::Success(t) | Cst::Suboptimal(t) => Ok(Some(t)),
                    other => Err(RenderError::FrameFailed(format!(
                        "재구성 후에도 서피스 획득 실패: {other:?}"
                    ))),
                }
            }

            // 오류가 아니다 — 최소화되었거나 가려졌거나 일시적으로 늦은 것뿐.
            Cst::Timeout | Cst::Occluded => Ok(None),

            Cst::Validation => Err(RenderError::FrameFailed(String::from(
                "서피스 획득 중 검증 오류",
            ))),
        }
    }
}

impl Renderer for WgpuRenderer {
    fn backend(&self) -> RenderBackend {
        self.info.backend
    }

    fn device_info(&self) -> &RenderDeviceInfo {
        &self.info
    }

    fn resize(&mut self, width: u32, height: u32) {
        // 최소화 시 0 이 들어온다. 0 크기 스왑체인은 만들 수 없다.
        if width == 0 || height == 0 {
            return;
        }
        if self.config.width == width && self.config.height == height {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.reconfigure();
    }

    fn begin_frame(&mut self) -> Result<FrameStatus, RenderError> {
        if self.frame.is_some() {
            return Err(RenderError::InvalidFrameState("이미 프레임이 열려 있음"));
        }

        let Some(surface_texture) = self.acquire()? else {
            return Ok(FrameStatus::Skipped);
        };

        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("nexus-frame-encoder"),
            });

        self.frame = Some(Frame {
            surface_texture,
            view,
            encoder,
            clear: wgpu::Color::BLACK,
            view_proj: Mat4::IDENTITY,
            viewport: None,
            quads: Vec::new(),
            #[cfg(feature = "ui")]
            ui: None,
        });

        Ok(FrameStatus::Acquired)
    }

    fn submit(&mut self, command: RenderCommand) -> Result<(), RenderError> {
        let frame = self
            .frame
            .as_mut()
            .ok_or(RenderError::InvalidFrameState("프레임이 열려 있지 않음"))?;

        match command {
            RenderCommand::Clear { color } => {
                let [r, g, b, a] = linear_rgba(color);
                frame.clear = wgpu::Color {
                    r: f64::from(r),
                    g: f64::from(g),
                    b: f64::from(b),
                    a: f64::from(a),
                };
            }
            RenderCommand::SetViewport {
                x,
                y,
                width,
                height,
            } => {
                frame.viewport = Some(ViewportRect {
                    x,
                    y,
                    width,
                    height,
                });
            }
            RenderCommand::SetCamera { view_proj } => {
                frame.view_proj = view_proj;
            }
            RenderCommand::DrawRect {
                center,
                size,
                rotation,
                z,
                color,
            } => {
                frame.quads.push(QuadInstance {
                    center: center.to_array(),
                    size: size.to_array(),
                    z,
                    rotation,
                    color: linear_rgba(color),
                });
            }
        }

        Ok(())
    }

    fn end_frame(&mut self) -> Result<(), RenderError> {
        let mut frame = self
            .frame
            .take()
            .ok_or(RenderError::InvalidFrameState("프레임이 열려 있지 않음"))?;

        self.last_quad_count = frame.quads.len();

        // ── GPU 로 올리기 ────────────────────────────────────────────────────
        self.queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::cast_slice(&frame.view_proj.to_cols_array()),
        );

        if !frame.quads.is_empty() {
            self.ensure_instance_capacity(frame.quads.len());
            self.queue
                .write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&frame.quads));
        }

        {
            let mut pass = frame
                .encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("nexus-main-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &frame.view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(frame.clear),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.depth_view,
                        depth_ops: Some(wgpu::Operations {
                            // LessEqual 비교이므로 "가장 먼" 값인 1.0 으로 초기화한다.
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });

            let full = ViewportRect {
                x: 0,
                y: 0,
                width: self.config.width,
                height: self.config.height,
            };
            let rect = frame
                .viewport
                .unwrap_or(full)
                .clamped(self.config.width, self.config.height);

            if !frame.quads.is_empty() && !rect.is_empty() {
                // 씬은 뷰포트 사각형 안에만 그린다. 카메라 투영은 이 사각형의 종횡비 기준이다.
                pass.set_viewport(
                    rect.x as f32,
                    rect.y as f32,
                    rect.width as f32,
                    rect.height as f32,
                    0.0,
                    1.0,
                );
                pass.set_scissor_rect(rect.x, rect.y, rect.width, rect.height);

                let used = (frame.quads.len() * core::mem::size_of::<QuadInstance>()) as u64;
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.camera_bind_group, &[]);
                pass.set_vertex_buffer(0, self.instance_buffer.slice(..used));
                // 정점 6개(쿼드 2삼각형) × 인스턴스 N개 — 드로우 콜 한 번.
                pass.draw(0..6, 0..frame.quads.len() as u32);
            }
        }

        // ── UI 합성 (에디터) ─────────────────────────────────────────────────
        #[cfg(feature = "ui")]
        let prepared = match frame.ui.take() {
            Some(ui_frame) => self.ui.paint(
                &self.device,
                &self.queue,
                &mut frame.encoder,
                &frame.view,
                [self.config.width, self.config.height],
                ui_frame,
            ),
            None => Vec::new(),
        };
        #[cfg(not(feature = "ui"))]
        let prepared: Vec<wgpu::CommandBuffer> = Vec::new();

        // ── 캡처 (present 전에 복사 명령을 기록해야 한다) ────────────────────
        let pending = if self.capture_requested {
            self.capture_requested = false;
            match capture::record(
                &self.device,
                &mut frame.encoder,
                &frame.surface_texture.texture,
                self.config.format,
            ) {
                Ok(p) => Some(p),
                Err(e) => {
                    self.capture_result = Some(Err(e));
                    None
                }
            }
        } else {
            None
        };

        // UI 준비 단계의 커맨드 버퍼가 메인 인코더보다 먼저 실행돼야 한다.
        self.queue.submit(
            prepared
                .into_iter()
                .chain(core::iter::once(frame.encoder.finish())),
        );
        // wgpu 30 부터 present 는 Queue 의 메서드다 (SurfaceTexture::present 아님).
        self.queue.present(frame.surface_texture);

        if let Some(p) = pending {
            self.capture_result = Some(p.finish(&self.device));
        }

        Ok(())
    }

    fn request_capture(&mut self) -> Result<(), RenderError> {
        if !self.capture_supported {
            return Err(RenderError::FrameFailed(format!(
                "이 서피스는 화면 캡처를 지원하지 않음 (포맷 {:?})",
                self.config.format
            )));
        }
        self.capture_requested = true;
        Ok(())
    }

    fn take_capture(&mut self) -> Option<Result<Capture, RenderError>> {
        self.capture_result.take()
    }
}

fn create_depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("nexus-depth"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

fn map_backend(backend: wgpu::Backend) -> RenderBackend {
    match backend {
        wgpu::Backend::Vulkan => RenderBackend::Vulkan,
        wgpu::Backend::Dx12 => RenderBackend::Dx12,
        wgpu::Backend::Metal => RenderBackend::Metal,
        wgpu::Backend::Gl => RenderBackend::Gl,
        _ => RenderBackend::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_endpoints_are_preserved() {
        assert!(srgb_to_linear(0.0).abs() < 1e-6);
        assert!((srgb_to_linear(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn srgb_mid_gray_is_darker_in_linear() {
        // sRGB 0.5 ≈ 선형 0.214 — 이 변환이 빠지면 화면이 전체적으로 밝게 뜬다
        assert!((srgb_to_linear(0.5) - 0.214).abs() < 1e-3);
    }

    #[test]
    fn alpha_is_not_converted() {
        let c = linear_rgba([0.5, 0.5, 0.5, 0.5]);
        assert!((c[3] - 0.5).abs() < 1e-6, "알파는 이미 선형이다");
    }

    #[test]
    fn viewport_is_clamped_to_surface() {
        let r = ViewportRect {
            x: 1000,
            y: 50,
            width: 500,
            height: 900,
        }
        .clamped(1280, 720);
        assert_eq!((r.x, r.y, r.width, r.height), (1000, 50, 280, 670));

        let outside = ViewportRect {
            x: 2000,
            y: 0,
            width: 10,
            height: 10,
        }
        .clamped(1280, 720);
        assert!(outside.is_empty());
    }
}
