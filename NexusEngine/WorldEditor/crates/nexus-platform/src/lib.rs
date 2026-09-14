//! 플랫폼 경계 — 창, 이벤트 루프, 입력, 프레임 타이밍.
//!
//! # 렌더러와의 접점
//!
//! 이 크레이트가 렌더러에 제공하는 것은 [`WindowTarget`] 하나뿐이다.
//! `raw-window-handle` 트레이트를 구현하므로 `ash-window` / `wgpu` 가 여기서
//! 서피스를 만들 수 있다. **플랫폼 분기가 존재하는 유일한 지점이며**, 그 분기는
//! 이 크레이트가 아니라 `ash-window` 내부에 있다.
//!
//! # 루프 구조
//!
//! ```text
//! about_to_wait   →  frame_dt 측정  →  FixedTimestep 누적
//!                       │
//!                       ├─ N회: App::fixed_update(step, &input)   ← 20Hz 고정
//!                       └─ request_redraw()
//!
//! RedrawRequested →  App::render(alpha)                           ← 화면 주사율
//! ```
//!
//! 시뮬레이션은 프레임레이트와 무관하게 항상 같은 `dt` 로 돈다. 렌더는 두 시뮬레이션
//! 상태 사이를 `alpha` 로 보간해 그린다. 이 분리는 나중에 서버(20Hz `ZoneActor` tick)를
//! 붙일 때 그대로 맞물린다.

#![forbid(unsafe_code)]

mod input;

pub use input::{Input, KeyCode, MouseButton};

/// 렌더러가 서피스를 만들 때 쓰는 핸들 트레이트를 재수출한다.
/// 렌더 크레이트는 반드시 이 경로를 통해 써야 버전이 어긋나지 않는다.
pub use raw_window_handle;

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use nexus_core::time::FixedTimestep;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{Window, WindowId};

/// 스모크 테스트용 — 지정한 초가 지나면 창을 자동으로 닫는다.
/// 창을 띄울 수 없는 환경에서 루프 동작만 확인할 때 쓴다.
const ENV_EXIT_AFTER: &str = "NEXUS_EXIT_AFTER_SECS";

// ─────────────────────────────────────────────────────────────────────────────
// 설정 / 오류
// ─────────────────────────────────────────────────────────────────────────────

/// 창 생성 및 루프 설정.
#[derive(Clone, Debug)]
pub struct WindowConfig {
    pub title: String,
    pub width: u32,
    pub height: u32,
    /// 시뮬레이션 주기(Hz). 서버 `ZoneActor` tick 과 맞추려면 20 을 유지한다.
    pub sim_hz: u32,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: String::from("Nexus WorldEditor"),
            width: 1280,
            height: 720,
            sim_hz: FixedTimestep::DEFAULT_HZ,
        }
    }
}

/// 플랫폼 계층에서 발생하는 오류.
#[derive(Debug)]
pub enum PlatformError {
    /// 이벤트 루프를 만들 수 없음 (디스플레이 없는 환경 등).
    EventLoop(String),
    /// 창 생성 실패.
    Window(String),
    /// [`App::init`] 이 실패를 반환함.
    App(String),
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EventLoop(m) => write!(f, "이벤트 루프 생성 실패: {m}"),
            Self::Window(m) => write!(f, "창 생성 실패: {m}"),
            Self::App(m) => write!(f, "애플리케이션 초기화 실패: {m}"),
        }
    }
}

impl std::error::Error for PlatformError {}

// ─────────────────────────────────────────────────────────────────────────────
// 렌더 타깃
// ─────────────────────────────────────────────────────────────────────────────

/// 렌더러가 서피스를 생성하는 데 필요한 것 전부.
///
/// `winit::window::Window` 를 감싸 `raw-window-handle` 트레이트만 노출한다.
/// M2 에서 `ash_window::create_surface(entry, instance, target.display_handle()?,
/// target.window_handle()?, None)` 형태로 쓰인다.
#[derive(Clone, Debug)]
pub struct WindowTarget {
    window: Arc<Window>,
}

impl WindowTarget {
    /// 프레임버퍼 크기 (물리 픽셀). 스왑체인 크기와 같아야 한다.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        let size = self.window.inner_size();
        (size.width, size.height)
    }

    /// 논리 픽셀 대비 물리 픽셀 배율 (HiDPI).
    #[must_use]
    pub fn scale_factor(&self) -> f64 {
        self.window.scale_factor()
    }
}

impl HasWindowHandle for WindowTarget {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        self.window.window_handle()
    }
}

impl HasDisplayHandle for WindowTarget {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        self.window.display_handle()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 애플리케이션 계약
// ─────────────────────────────────────────────────────────────────────────────

/// 플랫폼 루프가 구동하는 애플리케이션.
pub trait App {
    /// 창 생성 직후 1회 호출. 렌더러를 여기서 초기화한다.
    ///
    /// 기본 구현은 아무것도 하지 않는다.
    fn init(&mut self, target: &WindowTarget) -> Result<(), String> {
        let _ = target;
        Ok(())
    }

    /// 고정 간격 시뮬레이션 스텝. `dt` 는 항상 같은 값이다.
    ///
    /// 한 프레임에 0회 이상 호출된다. 프레임레이트에 의존하는 코드를 여기에 두지 말 것.
    fn fixed_update(&mut self, dt: Duration, input: &Input);

    /// 프레임 렌더. `alpha` 는 마지막 시뮬레이션 상태와 다음 상태 사이의 보간 계수 `[0, 1)`.
    fn render(&mut self, alpha: f32);

    /// 창 크기 변경. 스왑체인 재생성 지점 (M2).
    ///
    /// 최소화 시 `0 x 0` 으로 호출될 수 있으므로 구현에서 걸러야 한다.
    fn resize(&mut self, width: u32, height: u32) {
        let _ = (width, height);
    }

    /// 종료 직전 1회. GPU 자원 정리 지점 (M2).
    fn shutdown(&mut self) {}
}

// ─────────────────────────────────────────────────────────────────────────────
// 실행
// ─────────────────────────────────────────────────────────────────────────────

/// 창을 만들고 이벤트 루프를 돌린다. 창이 닫힐 때까지 반환하지 않는다.
///
/// # Errors
///
/// 이벤트 루프나 창을 만들 수 없거나, [`App::init`] 이 실패하면 오류를 반환한다.
pub fn run<A: App>(config: WindowConfig, app: A) -> Result<(), PlatformError> {
    let event_loop = EventLoop::new().map_err(|e| PlatformError::EventLoop(format!("{e}")))?;

    // 게임 루프이므로 이벤트가 없어도 계속 돈다.
    event_loop.set_control_flow(ControlFlow::Poll);

    let exit_after = std::env::var(ENV_EXIT_AFTER)
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .map(Duration::from_secs_f64);

    let mut runner = Runner {
        app,
        timestep: FixedTimestep::new(config.sim_hz),
        config,
        window: None,
        target: None,
        input: Input::default(),
        last_frame: Instant::now(),
        started: Instant::now(),
        exit_after,
        tick_count: 0,
        frame_count: 0,
        title_accum: Duration::ZERO,
        title_frames: 0,
        error: None,
    };

    event_loop
        .run_app(&mut runner)
        .map_err(|e| PlatformError::EventLoop(format!("{e}")))?;

    match runner.error.take() {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

struct Runner<A: App> {
    app: A,
    config: WindowConfig,
    window: Option<Arc<Window>>,
    target: Option<WindowTarget>,
    timestep: FixedTimestep,
    input: Input,

    last_frame: Instant,
    started: Instant,
    exit_after: Option<Duration>,

    tick_count: u64,
    frame_count: u64,
    title_accum: Duration,
    title_frames: u32,

    error: Option<PlatformError>,
}

impl<A: App> Runner<A> {
    /// 오류를 기록하고 루프를 종료시킨다.
    fn fail(&mut self, event_loop: &ActiveEventLoop, err: PlatformError) {
        self.error = Some(err);
        event_loop.exit();
    }

    /// 창 제목에 tick / fps 를 1초마다 갱신한다.
    /// M1 에는 렌더러가 없어 화면이 검으므로, 루프가 도는지 확인하는 유일한 수단이다.
    fn update_title(&mut self, frame_dt: Duration) {
        self.title_accum += frame_dt;
        self.title_frames += 1;

        if self.title_accum < Duration::from_secs(1) {
            return;
        }

        let fps = f64::from(self.title_frames) / self.title_accum.as_secs_f64();
        if let Some(window) = &self.window {
            window.set_title(&format!(
                "{} — tick {} | frame {} | {:.0} fps",
                self.config.title, self.tick_count, self.frame_count, fps
            ));
        }

        self.title_accum = Duration::ZERO;
        self.title_frames = 0;
    }
}

impl<A: App> ApplicationHandler for Runner<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return; // 재개(모바일) 시 중복 생성 방지
        }

        let attrs = Window::default_attributes()
            .with_title(&self.config.title)
            .with_inner_size(winit::dpi::LogicalSize::new(
                self.config.width,
                self.config.height,
            ));

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                self.fail(event_loop, PlatformError::Window(format!("{e}")));
                return;
            }
        };

        let target = WindowTarget {
            window: Arc::clone(&window),
        };

        if let Err(msg) = self.app.init(&target) {
            self.fail(event_loop, PlatformError::App(msg));
            return;
        }

        let (w, h) = target.size();
        self.app.resize(w, h);

        self.window = Some(window);
        self.target = Some(target);
        self.last_frame = Instant::now();
        self.timestep.reset();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }

            WindowEvent::Resized(size) => {
                self.app.resize(size.width, size.height);
            }

            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(target) = &self.target {
                    let (w, h) = target.size();
                    self.app.resize(w, h);
                }
            }

            WindowEvent::Focused(false) => {
                self.input.on_focus_lost();
                // 포커스를 잃은 동안 쌓인 시간을 시뮬레이션에 몰아넣지 않는다.
                self.timestep.reset();
                self.last_frame = Instant::now();
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    self.input
                        .on_key(code, event.state == ElementState::Pressed);
                }
            }

            WindowEvent::MouseInput { state, button, .. } => {
                self.input.on_button(button, state == ElementState::Pressed);
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.input.on_cursor(position.x as f32, position.y as f32);
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let amount = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    // 픽셀 단위 스크롤(터치패드)을 대략 한 줄 = 20px 로 환산
                    MouseScrollDelta::PixelDelta(p) => (p.y / 20.0) as f32,
                };
                self.input.on_wheel(amount);
            }

            WindowEvent::RedrawRequested => {
                self.frame_count += 1;
                self.app.render(self.timestep.alpha());
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(window) = self.window.clone() else {
            return;
        };

        if self
            .exit_after
            .is_some_and(|limit| self.started.elapsed() >= limit)
        {
            event_loop.exit();
            return;
        }

        let now = Instant::now();
        let frame_dt = now - self.last_frame;
        self.last_frame = now;

        // ── 고정 간격 시뮬레이션 ─────────────────────────────────────────
        let steps = self.timestep.accumulate(frame_dt);
        let step = self.timestep.step();
        for _ in 0..steps {
            self.tick_count += 1;
            self.app.fixed_update(step, &self.input);
        }

        // pressed/released/delta 는 프레임 단위이므로 스텝을 다 돌린 뒤 비운다.
        self.input.end_frame();

        self.update_title(frame_dt);
        window.request_redraw();
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.app.shutdown();
    }
}
