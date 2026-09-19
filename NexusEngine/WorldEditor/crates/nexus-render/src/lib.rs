//! 백엔드 중립 렌더 계약.
//!
//! 이 크레이트는 GPU API 를 전혀 알지 못한다. 구현은 `nexus-render-wgpu` 같은
//! 별도 크레이트가 제공한다. 앱은 가능하면 이 트레이트만 보고 작성한다.
//!
//! **이 크레이트는 네트워크 계층(`nexus-protocol` / `nexus-net`)에 절대 의존하지 않는다.**
//! CI 의 `boundary` 잡이 이를 검증한다.

#![forbid(unsafe_code)]

use nexus_core::{Mat4, Vec2};

/// 실제로 동작 중인 그래픽 백엔드.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderBackend {
    Vulkan,
    Dx12,
    Metal,
    Gl,
    /// 위 어디에도 해당하지 않음 (WebGPU 등).
    Other,
}

/// 선택된 물리 디바이스 정보. 초기화 시 실측값으로 채워진다.
#[derive(Clone, Debug)]
pub struct RenderDeviceInfo {
    /// 드라이버가 보고하는 어댑터 이름.
    pub name: String,
    pub backend: RenderBackend,
    /// 통합 GPU 인가. 전력/성능 프로파일 결정에 쓴다.
    pub is_integrated: bool,
}

/// 렌더러가 발급하는 불투명 텍스처 핸들.
///
/// 백엔드 타입(`wgpu::Texture` 등)은 이 계약 밖으로 나가지 않는다 — 앱은 핸들만 들고 다닌다.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TextureId(u32);

impl TextureId {
    /// 1×1 흰색 텍스처. 렌더러가 초기화 때 항상 만들어 둔다.
    ///
    /// [`RenderCommand::DrawRect`] 가 이것을 쓴다 — 덕분에 단색 쿼드와 스프라이트가
    /// 같은 파이프라인·같은 셰이더를 지난다.
    pub const WHITE: Self = Self(0);

    /// 렌더러 구현 전용 — 올린 순서대로 핸들을 만든다.
    #[must_use]
    pub fn from_index(index: u32) -> Self {
        Self(index)
    }

    #[must_use]
    pub fn index(self) -> u32 {
        self.0
    }
}

/// GPU 에 올릴 이미지 한 장.
///
/// 이미지 **디코딩은 이 계층의 일이 아니다** — `nexus-assets` 가 디코딩해서 이것을 만든다.
#[derive(Clone, Copy, Debug)]
pub struct TextureDesc<'a> {
    /// 디버거·프로파일러에 표시될 이름.
    pub label: &'a str,
    pub width: u32,
    pub height: u32,
    /// 8bit RGBA, **sRGB 인코딩**, 좌상단부터 행 우선. 길이는 정확히 `width * height * 4`.
    pub rgba: &'a [u8],
}

/// 아틀라스 안의 사각 영역. 정규화 UV(`0.0`~`1.0`), **좌상단 원점**.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UvRect {
    pub min: Vec2,
    pub max: Vec2,
}

impl UvRect {
    /// 텍스처 전체.
    pub const FULL: Self = Self {
        min: Vec2::ZERO,
        max: Vec2::ONE,
    };
}

impl Default for UvRect {
    fn default() -> Self {
        Self::FULL
    }
}

/// 한 프레임에 제출하는 그리기 명령.
///
/// 인스턴스 버퍼에 누적되며, **파이프라인과 텍스처가 바뀌는 지점에서만** 드로우 콜이 나뉜다.
/// 따라서 같은 아틀라스를 쓰는 스프라이트는 연속으로 제출할수록 싸다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RenderCommand {
    /// 프레임 배경색. 렌더 패스의 load 연산으로 처리되므로 프레임당 한 번만 유효하다.
    Clear { color: [f32; 4] },

    /// 씬을 그릴 화면 영역 (물리 픽셀, 좌상단 원점).
    ///
    /// 에디터처럼 패널이 화면을 나눠 쓸 때, 씬은 이 사각형 안에만 그려진다.
    /// 제출하지 않으면 화면 전체를 쓴다. 화면 밖으로 나간 부분은 잘린다.
    SetViewport {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },

    /// 카메라 행렬 설정. 드로우 명령보다 먼저 제출해야 한다.
    ///
    /// M8 에서 이 행렬을 만드는 쪽이 정사영에서 원근으로 바뀔 뿐,
    /// 렌더러는 아무것도 달라지지 않는다.
    SetCamera { view_proj: Mat4 },

    /// 월드 공간 사각형. `center` / `size` 는 미터.
    ///
    /// `rotation` 은 Z 축 기준 라디안 (`0` = 축 정렬, 위에서 볼 때 반시계가 +) —
    /// `nexus_core::units` 의 방향 규약과 같다. `size.x` 가 회전 후 방향 쪽 길이다.
    ///
    /// `z` 는 **월드 높이(m)** 다. 카메라가 기울어져 있으면(쿼터뷰) 화면 세로 위치를
    /// 밀어 올린다 — 키 큰 것이 위로 솟아 보이는, 의도된 동작이다.
    ///
    /// 따라서 **단순히 그리는 순서를 정하려고 `z` 를 쓰면 안 된다.** 같은 높이에 있는
    /// 것들의 앞뒤를 정할 때는 `depth_bias` 를 쓴다 — 화면 위치는 건드리지 않고
    /// 깊이만 민다. 단위는 NDC 깊이([`DEPTH_LAYER`] 참고)이고 **양수가 앞**이다.
    DrawRect {
        center: Vec2,
        size: Vec2,
        rotation: f32,
        z: f32,
        depth_bias: f32,
        color: [f32; 4],
    },

    /// 텍스처가 입혀진 사각형. 좌표 규약은 [`DrawRect`](Self::DrawRect) 와 같다.
    ///
    /// **S1 단계에서는 `DrawRect` 와 마찬가지로 지면(XY 평면)에 눕는다.**
    /// 서 있는 빌보드는 S3 에서 `anchor` 와 함께 들어온다.
    ///
    /// `tint` 는 sRGB 이며 텍스처 색에 **곱해진다** — 흰색이면 원본 그대로다.
    /// 알파는 블렌딩이 아니라 **컷아웃**으로 처리된다
    /// ([`ALPHA_CUTOFF`] 미만은 버려진다). 그래서 깊이 쓰기를 켠 채로도 정렬이 깨지지 않는다.
    DrawSprite {
        center: Vec2,
        size: Vec2,
        rotation: f32,
        z: f32,
        depth_bias: f32,
        uv: UvRect,
        texture: TextureId,
        tint: [f32; 4],
    },
}

/// 겹침 순서를 한 칸 옮기는 [`depth_bias`](RenderCommand::DrawRect) 단위.
///
/// 깊이 버퍼 해상도(NDC 에서 약 `1e-7`)보다 세 자릿수 크고, 전체 깊이 범위에 비하면
/// 무시할 만큼 작다. 몇 십 칸을 쌓아도 문제가 없다.
pub const DEPTH_LAYER: f32 = 1e-4;

/// 스프라이트 알파 컷아웃 기준. 이 값 미만의 알파는 그리지 않는다.
///
/// 반투명 블렌딩이 아니라 컷아웃을 쓰는 이유: 블렌딩은 뒤→앞 정렬을 요구하는데,
/// 깊이 버퍼로 정렬하는 이 렌더러에서는 그 순서를 보장할 수 없다.
/// 진짜 반투명(그림자·이펙트)은 나중에 별도 패스로 분리한다.
pub const ALPHA_CUTOFF: f32 = 0.5;

/// 렌더 계층 오류.
#[derive(Debug)]
pub enum RenderError {
    /// 어댑터·디바이스·서피스를 얻지 못함.
    InitializationFailed(String),
    /// 프레임 획득/제출 실패.
    FrameFailed(String),
    /// 프레임 생명주기 위반 (`begin_frame` 없이 `submit` 등).
    InvalidFrameState(&'static str),
    /// 텍스처를 올리지 못함 (크기 0, 바이트 길이 불일치, 디바이스 한계 초과).
    TextureFailed(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InitializationFailed(m) => write!(f, "렌더러 초기화 실패: {m}"),
            Self::FrameFailed(m) => write!(f, "프레임 처리 실패: {m}"),
            Self::InvalidFrameState(m) => write!(f, "프레임 상태 오류: {m}"),
            Self::TextureFailed(m) => write!(f, "텍스처 업로드 실패: {m}"),
        }
    }
}

impl std::error::Error for RenderError {}

/// [`Renderer::begin_frame`] 의 결과.
///
/// 창이 최소화되었거나 다른 창에 완전히 가려진 경우처럼 **오류가 아니면서
/// 그릴 필요도 없는** 상황이 존재한다. 이를 오류로 다루면 로그가 쏟아지므로
/// 정상 경로로 분리한다.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum FrameStatus {
    /// 스왑체인 이미지를 획득했다. `submit` → `end_frame` 으로 진행한다.
    Acquired,
    /// 이번 프레임은 건너뛴다. `submit` / `end_frame` 을 호출하면 안 된다.
    Skipped,
}

/// 화면 캡처 결과. 8bit RGBA, sRGB 인코딩, 좌상단부터 행 우선.
#[derive(Clone)]
pub struct Capture {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` 바이트.
    pub rgba: Vec<u8>,
}

impl std::fmt::Debug for Capture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Capture")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("bytes", &self.rgba.len())
            .finish()
    }
}

/// 프레임 단위 렌더러.
///
/// 호출 순서는 `begin_frame` → `submit`* → `end_frame` 이다.
/// M3 에서 `begin_frame` 이 `Frame<'_>` 을 반환하는 typestate 로 바꿔
/// 이 순서를 컴파일 타임에 강제하는 것을 검토한다.
pub trait Renderer {
    fn backend(&self) -> RenderBackend;

    fn device_info(&self) -> &RenderDeviceInfo;

    /// 서피스 크기 변경. 스왑체인을 재생성한다.
    ///
    /// 최소화 시 `0` 이 들어올 수 있으며, 구현은 이를 무시해야 한다.
    fn resize(&mut self, width: u32, height: u32);

    /// 이미지를 GPU 에 올리고 핸들을 받는다. **프레임 바깥에서** 호출한다.
    ///
    /// 올린 텍스처는 렌더러가 살아 있는 동안 유지된다. 해제는 아직 없다 —
    /// 존 전환에서 애셋을 갈아끼우게 되는 S5 에서 다시 본다.
    ///
    /// # Errors
    /// 크기가 0 이거나 `rgba` 길이가 `width * height * 4` 와 다르면
    /// [`RenderError::TextureFailed`].
    fn load_texture(&mut self, desc: &TextureDesc<'_>) -> Result<TextureId, RenderError>;

    /// 프레임 시작 — 스왑체인 이미지를 획득한다.
    ///
    /// [`FrameStatus::Skipped`] 를 반환하면 이번 프레임은 그리지 않고 넘어간다.
    ///
    /// # Errors
    /// 서피스가 복구 불가능한 상태이면 오류를 반환한다.
    fn begin_frame(&mut self) -> Result<FrameStatus, RenderError>;

    /// 그리기 명령 제출.
    ///
    /// # Errors
    /// `begin_frame` 이 선행되지 않았으면 오류를 반환한다.
    fn submit(&mut self, command: RenderCommand) -> Result<(), RenderError>;

    /// 프레임 종료 — 커맨드를 제출하고 화면에 표시한다.
    ///
    /// # Errors
    /// `begin_frame` 이 선행되지 않았거나 제출에 실패하면 오류를 반환한다.
    fn end_frame(&mut self) -> Result<(), RenderError>;

    /// 다음으로 표시되는 프레임을 캡처하도록 예약한다.
    ///
    /// 결과는 그 프레임의 `end_frame` 이후 [`take_capture`](Self::take_capture) 로 꺼낸다.
    /// 자동화 검증(스크린샷 비교, 사람 없이 UI 확인)에 쓴다.
    ///
    /// # Errors
    /// 백엔드나 서피스가 화면 읽기를 지원하지 않으면 오류를 반환한다.
    fn request_capture(&mut self) -> Result<(), RenderError>;

    /// 완료된 캡처를 꺼낸다. 아직 없으면 `None`.
    ///
    /// # Errors
    /// 캡처는 수행됐지만 읽기에 실패했으면 오류를 반환한다.
    fn take_capture(&mut self) -> Option<Result<Capture, RenderError>>;
}
