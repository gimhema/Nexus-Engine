//! wgpu 기반 [`Renderer`] 구현.
//!
//! Windows / Linux 에서 wgpu 는 기본적으로 **Vulkan 백엔드**로 동작한다.
//! 즉 Vulkan 을 쓰되 인스턴스·디바이스·스왑체인·동기화 보일러플레이트를
//! 직접 작성하지 않는 것이다. 서피스 생성의 플랫폼 분기도 wgpu 내부에 있으므로
//! 이 크레이트에는 `#[cfg(target_os)]` 가 하나도 없다.
//!
//! # 파이프라인
//!
//! 인스턴스 기반 텍스처 쿼드다. 그리드 선·오브젝트·스프라이트를 전부 이것으로 그린다 —
//! 선은 얇은 쿼드다. 정점 버퍼는 없고 `vertex_index` 로 쿼드를 만든다.
//!
//! 셰이더는 하나이고, **블렌드 상태와 프래그먼트 진입점만 다른 파이프라인 두 개**가 있다:
//!
//! | | 용도 | 알파 |
//! |---|---|---|
//! | `pipeline_blend` | 그리드·에디터 표시 | 알파 블렌딩 |
//! | `pipeline_cutout` | 스프라이트 | `discard` 컷아웃, 블렌딩 없음 |
//!
//! 스프라이트에 블렌딩을 쓰지 않는 이유는 **블렌딩이 뒤→앞 정렬을 요구하기 때문**이다.
//! 이 렌더러는 깊이 버퍼로 정렬하므로 그 순서를 보장할 수 없다. 컷아웃은 깊이 쓰기를
//! 켠 채로도 결과가 맞는다. 진짜 반투명(그림자·이펙트)은 나중에 별도 패스로 분리한다.
//!
//! 단색 쿼드는 1×1 흰색 텍스처([`nexus_render::TextureId::WHITE`])를 샘플링한다.
//! 덕분에 "텍스처 있음/없음"으로 셰이더가 갈리지 않는다.
//!
//! 드로우 콜은 **파이프라인이나 텍스처가 바뀌는 지점에서만** 나뉜다. 제출 순서를
//! 유지해야 블렌딩이 맞으므로 정렬하지 않는다 — 같은 아틀라스를 연속 제출할수록 싸다.
//!
//! **깊이 버퍼를 2D 단계에서도 켜 둔다.** 스프라이트를 실제 3D 위치에 세우면
//! Y-정렬을 CPU 에서 할 필요가 없어진다(S3).

#![forbid(unsafe_code)]

mod capture;
#[cfg(feature = "ui")]
mod ui;

#[cfg(feature = "ui")]
pub use ui::{TextureCarry, UiFrame, egui};

use bytemuck::{Pod, Zeroable};
use nexus_core::{Mat4, Vec3};
use nexus_render::{
    Capture, DrawLayer, FrameStatus, RenderBackend, RenderCommand, RenderDeviceInfo, RenderError,
    Renderer, TextureDesc, TextureId,
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

/// GPU 로 넘어가는 카메라 유니폼. WGSL `Camera` 구조체와 레이아웃이 일치해야 한다.
///
/// `right` / `up` 은 빌보드 축이다. 유니폼 버퍼는 16바이트 정렬이라 `vec3` 를 그대로
/// 넣을 수 없어 `vec4` 로 맞춘다 — w 는 쓰이지 않는다.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct CameraUniform {
    view_proj: [[f32; 4]; 4],
    right: [f32; 4],
    up: [f32; 4],
}

const _: () = {
    assert!(core::mem::size_of::<CameraUniform>() == 96);
};

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
    /// 아틀라스 영역 좌상단 (정규화 UV).
    uv_min: [f32; 2],
    /// 아틀라스 영역 우하단 (정규화 UV).
    uv_max: [f32; 2],
    /// NDC 깊이 편향. 양수가 앞. 화면 위치에는 영향이 없다.
    depth_bias: f32,
    /// `0.0` = 지면에 눕는 쿼드, `1.0` = 카메라를 향해 서는 빌보드.
    ///
    /// 파이프라인(블렌드/컷아웃)과 **독립**이다 — 텍스처 지면 타일(S5)처럼
    /// 컷아웃이면서 눕는 조합이 생긴다.
    billboard: f32,
    /// 빌보드 전용 앵커 오프셋. 지면 쿼드에서는 쓰이지 않는다.
    anchor: f32,
    /// 층이 쓰는 NDC 깊이 구간 `(시작, 크기)`.
    depth_span: [f32; 2],
}

/// 레이아웃이 셰이더 속성 오프셋과 맞는지 컴파일 타임에 확인한다.
const _: () = {
    assert!(core::mem::size_of::<QuadInstance>() == 76);
    assert!(core::mem::offset_of!(QuadInstance, center) == 0);
    assert!(core::mem::offset_of!(QuadInstance, size) == 8);
    assert!(core::mem::offset_of!(QuadInstance, z) == 16);
    assert!(core::mem::offset_of!(QuadInstance, rotation) == 20);
    assert!(core::mem::offset_of!(QuadInstance, color) == 24);
    assert!(core::mem::offset_of!(QuadInstance, uv_min) == 40);
    assert!(core::mem::offset_of!(QuadInstance, uv_max) == 48);
    assert!(core::mem::offset_of!(QuadInstance, depth_bias) == 56);
    assert!(core::mem::offset_of!(QuadInstance, billboard) == 60);
    assert!(core::mem::offset_of!(QuadInstance, anchor) == 64);
    assert!(core::mem::offset_of!(QuadInstance, depth_span) == 68);
};

/// 인스턴스를 어느 파이프라인으로 그릴지.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PipelineKind {
    /// 알파 블렌딩 — 그리드·에디터 표시.
    Blend,
    /// 알파 컷아웃 — 스프라이트.
    Cutout,
}

/// 파이프라인과 텍스처가 같아 한 번에 그릴 수 있는 인스턴스 구간.
///
/// 제출 순서를 유지해야 블렌딩 결과가 맞으므로, 정렬하지 않고
/// **바뀌는 지점에서만** 끊는다.
#[derive(Clone, Copy, Debug)]
struct Batch {
    kind: PipelineKind,
    texture: TextureId,
    /// `Frame::quads` 안의 시작 인덱스.
    start: u32,
    count: u32,
}

/// GPU 에 올라간 텍스처 하나. `bind_group` 이 실제로 쓰이고,
/// 나머지 둘은 수명을 붙잡아 두기 위해 보관한다.
#[derive(Debug)]
struct TextureEntry {
    _texture: wgpu::Texture,
    _view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
}

/// 이미지를 GPU 에 올리고 바인드 그룹까지 만든다.
///
/// 포맷은 `Rgba8UnormSrgb` 다 — 입력이 sRGB 이므로 GPU 가 샘플링할 때 선형으로 바꿔 준다.
/// 인스턴스 색도 이미 선형이라, 셰이더의 곱셈이 선형 공간에서 일어난다.
fn create_texture_entry(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    desc: &TextureDesc<'_>,
) -> Result<TextureEntry, RenderError> {
    if desc.width == 0 || desc.height == 0 {
        return Err(RenderError::TextureFailed(format!(
            "'{}': 크기가 0 ({}x{})",
            desc.label, desc.width, desc.height
        )));
    }

    let expected = desc.width as usize * desc.height as usize * 4;
    if desc.rgba.len() != expected {
        return Err(RenderError::TextureFailed(format!(
            "'{}': 바이트 수가 맞지 않음 — {}x{} 이면 {expected} 이어야 하는데 {} 임",
            desc.label,
            desc.width,
            desc.height,
            desc.rgba.len()
        )));
    }

    let size = wgpu::Extent3d {
        width: desc.width,
        height: desc.height,
        depth_or_array_layers: 1,
    };

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(desc.label),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    queue.write_texture(
        texture.as_image_copy(),
        desc.rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(desc.width * 4),
            rows_per_image: Some(desc.height),
        },
        size,
    );

    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(desc.label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });

    Ok(TextureEntry {
        _texture: texture,
        _view: view,
        bind_group,
    })
}

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
    /// 빌보드 축. `SetCamera` 로 갱신된다.
    right: Vec3,
    up: Vec3,
    /// 현재 층. `SetLayer` 로 갱신된다.
    layer: DrawLayer,
    viewport: Option<ViewportRect>,
    quads: Vec<QuadInstance>,
    batches: Vec<Batch>,
    #[cfg(feature = "ui")]
    ui: Option<UiFrame>,
}

impl Frame {
    /// 인스턴스를 쌓으면서 배치를 잇는다.
    ///
    /// 직전과 파이프라인·텍스처가 같으면 배치를 늘리고, 다르면 새로 연다.
    fn push(&mut self, kind: PipelineKind, texture: TextureId, instance: QuadInstance) {
        let start = u32::try_from(self.quads.len()).unwrap_or(u32::MAX);
        self.quads.push(instance);

        match self.batches.last_mut() {
            Some(last) if last.kind == kind && last.texture == texture => last.count += 1,
            _ => self.batches.push(Batch {
                kind,
                texture,
                start,
                count: 1,
            }),
        }
    }
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

    /// 알파 블렌딩 — 그리드·에디터 표시용.
    pipeline_blend: wgpu::RenderPipeline,
    /// 알파 컷아웃 — 스프라이트용. 블렌딩과 깊이 정렬은 같이 갈 수 없어 나눴다.
    pipeline_cutout: wgpu::RenderPipeline,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    /// 텍스처 + 샘플러 바인드 그룹 레이아웃. 올릴 때마다 이것으로 그룹을 만든다.
    texture_layout: wgpu::BindGroupLayout,
    /// 픽셀아트용 Nearest 샘플러. 모든 텍스처가 공유한다.
    sampler: wgpu::Sampler,
    /// [`TextureId`] 의 인덱스로 찾는다. 0 번은 항상 1×1 흰색.
    textures: Vec<TextureEntry>,
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
            size: core::mem::size_of::<CameraUniform>() as u64,
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

        // ── 텍스처 바인딩 ────────────────────────────────────────────────────
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("nexus-texture-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        // 픽셀아트는 확대해도 픽셀 경계가 또렷해야 한다.
        // Linear 로 두면 스프라이트가 뿌옇게 뭉개진다 — 이 장르에서 가장 눈에 띄는 실수다.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nexus-sampler-nearest"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("nexus-quad-layout"),
            bind_group_layouts: &[Some(&camera_layout), Some(&texture_layout)],
            immediate_size: 0,
        });

        // 두 파이프라인은 프래그먼트 진입점과 블렌드 상태만 다르다.
        let make_pipeline = |label: &str, entry: &str, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
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
                            4 => Float32x4,  // color  — 오프셋 24 (위 const 단언으로 검증)
                            5 => Float32x2,  // uv_min — 오프셋 40
                            6 => Float32x2,  // uv_max — 오프셋 48
                            7 => Float32,    // depth_bias — 오프셋 56
                            8 => Float32,    // billboard  — 오프셋 60
                            9 => Float32,    // anchor     — 오프셋 64
                            10 => Float32x2, // depth_span — 오프셋 68
                        ],
                    })],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    // 2D 쿼드는 양면이 다 보여야 한다. 빌보드가 들어오는 S3 에서 다시 본다.
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
            })
        };

        let pipeline_blend = make_pipeline(
            "nexus-quad-blend",
            "fs_blend",
            Some(wgpu::BlendState::ALPHA_BLENDING),
        );
        // 컷아웃은 블렌딩을 끈다 — discard 로 이미 구멍을 뚫었으므로 섞을 것이 없다.
        let pipeline_cutout = make_pipeline("nexus-quad-cutout", "fs_cutout", None);

        // TextureId::WHITE — 단색 쿼드가 샘플링할 1×1 흰색.
        // 이것이 있어야 DrawRect 와 DrawSprite 가 같은 셰이더를 지난다.
        let white = create_texture_entry(
            &device,
            &queue,
            &texture_layout,
            &sampler,
            &TextureDesc {
                label: "nexus-white",
                width: 1,
                height: 1,
                rgba: &[0xFF; 4],
            },
        )?;

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
            pipeline_blend,
            pipeline_cutout,
            camera_buffer,
            camera_bind_group,
            texture_layout,
            sampler,
            textures: vec![white],
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
            right: Vec3::X,
            up: Vec3::Y,
            layer: DrawLayer::default(),
            viewport: None,
            quads: Vec::new(),
            batches: Vec::new(),
            #[cfg(feature = "ui")]
            ui: None,
        });

        Ok(FrameStatus::Acquired)
    }

    fn load_texture(&mut self, desc: &TextureDesc<'_>) -> Result<TextureId, RenderError> {
        let entry = create_texture_entry(
            &self.device,
            &self.queue,
            &self.texture_layout,
            &self.sampler,
            desc,
        )?;

        let index = u32::try_from(self.textures.len())
            .map_err(|_| RenderError::TextureFailed(String::from("텍스처 수가 u32 를 넘음")))?;
        self.textures.push(entry);
        Ok(TextureId::from_index(index))
    }

    fn submit(&mut self, command: RenderCommand) -> Result<(), RenderError> {
        // frame 을 빌리기 전에 읽어 둔다 — 아래에서 self 를 다시 빌릴 수 없다.
        let texture_count = self.textures.len();
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
            RenderCommand::SetLayer(layer) => {
                frame.layer = layer;
            }
            RenderCommand::SetCamera {
                view_proj,
                right,
                up,
            } => {
                frame.view_proj = view_proj;
                frame.right = right;
                frame.up = up;
            }
            RenderCommand::DrawRect {
                center,
                size,
                rotation,
                z,
                depth_bias,
                color,
                uv,
                texture,
            } => {
                // 없는 텍스처는 흰색으로 떨어뜨린다 — 스프라이트와 같은 이유(아래 참고).
                let texture = if (texture.index() as usize) < texture_count {
                    texture
                } else {
                    TextureId::WHITE
                };
                let (o, s) = frame.layer.depth_range();
                let span = [o, s];
                frame.push(
                    PipelineKind::Blend,
                    texture,
                    QuadInstance {
                        center: center.to_array(),
                        size: size.to_array(),
                        z,
                        rotation,
                        color: linear_rgba(color),
                        uv_min: uv.min.to_array(),
                        uv_max: uv.max.to_array(),
                        depth_bias,
                        billboard: 0.0,
                        anchor: 0.0,
                        depth_span: span,
                    },
                );
            }
            RenderCommand::DrawSprite {
                pos,
                size,
                anchor,
                depth_bias,
                uv,
                texture,
                tint,
            } => {
                // 없는 텍스처는 흰색으로 떨어뜨린다. 프레임 도중이라 오류를 반환할 수 없고,
                // 화면이 통째로 사라지는 것보다 단색으로 눈에 띄는 편이 낫다.
                let texture = if (texture.index() as usize) < texture_count {
                    texture
                } else {
                    TextureId::WHITE
                };
                let (o, s) = frame.layer.depth_range();
                let span = [o, s];
                frame.push(
                    PipelineKind::Cutout,
                    texture,
                    QuadInstance {
                        center: [pos.x, pos.y],
                        size: size.to_array(),
                        z: pos.z,
                        // 빌보드는 화면에 대해 항상 똑바로 선다.
                        rotation: 0.0,
                        color: linear_rgba(tint),
                        uv_min: uv.min.to_array(),
                        uv_max: uv.max.to_array(),
                        depth_bias,
                        billboard: 1.0,
                        anchor: anchor.offset(),
                        depth_span: span,
                    },
                );
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
        let camera = CameraUniform {
            view_proj: frame.view_proj.to_cols_array_2d(),
            right: frame.right.extend(0.0).to_array(),
            up: frame.up.extend(0.0).to_array(),
        };
        self.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&camera));

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
                pass.set_bind_group(0, &self.camera_bind_group, &[]);
                pass.set_vertex_buffer(0, self.instance_buffer.slice(..used));

                // 배치마다 드로우 콜 하나. 같은 파이프라인·텍스처가 이어지는 동안은
                // 인스턴싱으로 묶이므로, 단색만 그리던 때와 드로우 콜 수가 같다.
                let mut bound: Option<(PipelineKind, TextureId)> = None;
                for batch in &frame.batches {
                    if bound != Some((batch.kind, batch.texture)) {
                        pass.set_pipeline(match batch.kind {
                            PipelineKind::Blend => &self.pipeline_blend,
                            PipelineKind::Cutout => &self.pipeline_cutout,
                        });
                        let entry = &self.textures[batch.texture.index() as usize];
                        pass.set_bind_group(1, &entry.bind_group, &[]);
                        bound = Some((batch.kind, batch.texture));
                    }
                    // 정점 6개(쿼드 2삼각형) × 인스턴스 N개.
                    pass.draw(0..6, batch.start..batch.start + batch.count);
                }
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
