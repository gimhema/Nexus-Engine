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

/// 한 프레임에 제출하는 그리기 명령.
///
/// `DrawRect` 는 인스턴스 버퍼에 누적되어 드로우 콜 한 번으로 처리된다.
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
    /// `z` 는 높이(m)이자 깊이 정렬 기준이다 — 값이 큰 쪽이 위에 그려진다.
    DrawRect {
        center: Vec2,
        size: Vec2,
        rotation: f32,
        z: f32,
        color: [f32; 4],
    },
}

/// 렌더 계층 오류.
#[derive(Debug)]
pub enum RenderError {
    /// 어댑터·디바이스·서피스를 얻지 못함.
    InitializationFailed(String),
    /// 프레임 획득/제출 실패.
    FrameFailed(String),
    /// 프레임 생명주기 위반 (`begin_frame` 없이 `submit` 등).
    InvalidFrameState(&'static str),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InitializationFailed(m) => write!(f, "렌더러 초기화 실패: {m}"),
            Self::FrameFailed(m) => write!(f, "프레임 처리 실패: {m}"),
            Self::InvalidFrameState(m) => write!(f, "프레임 상태 오류: {m}"),
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
