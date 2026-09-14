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

use bytemuck::{Pod, Zeroable};
use nexus_core::Mat4;
use nexus_render::{
    FrameStatus, RenderBackend, RenderCommand, RenderDeviceInfo, RenderError, Renderer,
};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

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
    _pad: [f32; 3], // 16바이트 정렬 — vec4 color 앞을 맞춘다
    color: [f32; 4],
}

/// 프레임 중간 상태 — `begin_frame` 과 `end_frame` 사이에만 존재한다.
struct Frame {
    surface_texture: wgpu::SurfaceTexture,
    view: wgpu::TextureView,
    encoder: wgpu::CommandEncoder,
    clear: wgpu::Color,
    view_proj: Mat4,
    quads: Vec<QuadInstance>,
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
                        3 => Float32x4,  // color (offset 은 _pad 로 16바이트 정렬됨)
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
                // Z 가 큰(= 카메라에 가까운) 쪽이 위에 그려진다.
                depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
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
        })
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
            quads: Vec::new(),
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
                frame.clear = wgpu::Color {
                    r: f64::from(color[0]),
                    g: f64::from(color[1]),
                    b: f64::from(color[2]),
                    a: f64::from(color[3]),
                };
            }
            RenderCommand::SetCamera { view_proj } => {
                frame.view_proj = view_proj;
            }
            RenderCommand::DrawRect {
                center,
                size,
                z,
                color,
            } => {
                frame.quads.push(QuadInstance {
                    center: center.to_array(),
                    size: size.to_array(),
                    z,
                    _pad: [0.0; 3],
                    color,
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
                            // GreaterEqual 비교이므로 "가장 먼" 값인 0.0 으로 초기화한다.
                            load: wgpu::LoadOp::Clear(0.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });

            if !frame.quads.is_empty() {
                let used = (frame.quads.len() * core::mem::size_of::<QuadInstance>()) as u64;
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.camera_bind_group, &[]);
                pass.set_vertex_buffer(0, self.instance_buffer.slice(..used));
                // 정점 6개(쿼드 2삼각형) × 인스턴스 N개 — 드로우 콜 한 번.
                pass.draw(0..6, 0..frame.quads.len() as u32);
            }
        }

        self.queue.submit(core::iter::once(frame.encoder.finish()));
        // wgpu 30 부터 present 는 Queue 의 메서드다 (SurfaceTexture::present 아님).
        self.queue.present(frame.surface_texture);

        Ok(())
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
