//! Nexus WorldEditor — 에디터 애플리케이션.
//!
//! 현재 단계: M5-1 뷰포트 편집.
//!   - 왼쪽 클릭 선택 / Shift+클릭 추가·해제 / 끌어서 이동 / Ctrl+드래그 그리드 스냅
//!   - 존 경계: 변·모서리 핸들을 끌어 크기 조절
//!   - 언두/리두 (Ctrl+Z / Ctrl+Y · Ctrl+Shift+Z), Esc 선택 해제
//!   - 인스펙터: 위치·경계 값 편집 (나머지 필드는 패널 명세 대기)
//!   - 가운데·오른쪽 드래그 팬, 휠 줌, Home 시점 초기화, F12 스크린샷
//!
//! 화면에 보이는 배치는 NexusEngine `Server.cpp` 의 기본 존 설정을 옮겨온 것이다.
//! M6 에서 `ZoneConfig` 로 내보낸다.

mod edit;
mod grid;
mod scene;
mod screenshot;
mod ui;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use nexus_core::{Camera2d, Vec2};
use nexus_platform::{App, Input, WindowConfig, WindowEvent, WindowTarget};
use nexus_render::{FrameStatus, RenderCommand, RenderError, Renderer};
use nexus_render_wgpu::{TextureCarry, UiFrame, WgpuRenderer};

use edit::Editing;
use scene::{Handle, Pick, Scene, Target};
use ui::{EditorUi, FrameStats, UiActions, UiModel};

// 색은 모두 sRGB. 렌더러가 선형으로 변환한다.
const CLEAR_COLOR: [f32; 4] = [0.05, 0.06, 0.08, 1.0];
pub(crate) const ZONE_BOUNDS_COLOR: [f32; 4] = [0.35, 0.60, 0.95, 0.9];
const SELECT_COLOR: [f32; 4] = [1.0, 0.82, 0.25, 1.0];
const HOVER_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 0.55];
const HANDLE_FILL: [f32; 4] = [0.95, 0.97, 1.0, 1.0];
const HANDLE_BORDER: [f32; 4] = [0.05, 0.06, 0.08, 1.0];

// 겹침 순서 (월드 Z, 클수록 위). 그리드는 음수.
const Z_ZONE: f32 = 0.5;
const Z_MARKER: f32 = 1.0;
const Z_HOVER: f32 = 1.5;
const Z_SELECT: f32 = 2.0;
const Z_HANDLE: f32 = 3.0;

/// 시작 시 미리 선택할 대상 (쉼표 구분 이름). 스크린샷으로 선택 표시를 검증할 때 쓴다.
const ENV_SELECT: &str = "NEXUS_SELECT";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = WindowConfig::default();

    println!(
        "WorldEditor 시작 — {}x{}, 시뮬레이션 {}Hz",
        config.width, config.height, config.sim_hz
    );
    println!(
        "조작: 왼쪽 = 선택/이동 (Shift 추가, Ctrl 스냅) | 가운데·오른쪽 드래그 = 팬 | 휠 = 줌 \
         | Ctrl+Z/Y = 실행 취소/다시 | Esc = 선택 해제 | Home | F12"
    );

    nexus_platform::run(config, Editor::default())?;

    println!("WorldEditor 종료");
    Ok(())
}

#[derive(Debug)]
struct Editor {
    renderer: Option<WgpuRenderer>,
    ui: Option<EditorUi>,
    target: Option<WindowTarget>,
    /// 건너뛴 프레임에서 적용하지 못한 UI 텍스처 변경분.
    texture_carry: TextureCarry,

    scene: Scene,
    editing: Editing,
    camera: Camera2d,
    /// 포인터 아래의 대상 — 직전 프레임 기준.
    hover: Option<Pick>,
    /// 프레임마다 재사용하는 명령 버퍼 — 매 프레임 할당을 피한다.
    commands: Vec<RenderCommand>,

    // ── 통계 ─────────────────────────────────────────────────────────────
    ticks: u64,
    last_frame: Option<Instant>,
    fps: f32,

    // ── 스크린샷 ─────────────────────────────────────────────────────────
    frame_index: u64,
    auto_shot: Option<screenshot::AutoPlan>,
    manual_shot_requested: bool,
    /// 캡처를 요청해 두고 결과를 기다리는 중인 저장 경로. `true` 면 저장 후 종료.
    pending_shot: Option<(PathBuf, bool)>,
    exit_requested: bool,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            renderer: None,
            ui: None,
            target: None,
            texture_carry: TextureCarry::default(),
            scene: Scene::server_default(),
            editing: Editing::default(),
            camera: Camera2d::default(),
            hover: None,
            commands: Vec::new(),
            ticks: 0,
            last_frame: None,
            fps: 0.0,
            frame_index: 0,
            auto_shot: None,
            manual_shot_requested: false,
            pending_shot: None,
            exit_requested: false,
        }
    }
}

impl Editor {
    fn reset_view(&mut self) {
        self.camera.center = Vec2::ZERO;
        self.camera.view_height = 2600.0;
    }

    /// `NEXUS_SELECT` 로 지정된 대상을 미리 선택한다.
    fn apply_env_selection(&mut self) {
        let Ok(list) = std::env::var(ENV_SELECT) else {
            return;
        };
        for name in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            match self.scene.find_by_label(name) {
                Some(target) => self.editing.select(target, true),
                None => eprintln!("{ENV_SELECT}: '{name}' 를 찾지 못함"),
            }
        }
    }

    fn update_fps(&mut self) {
        let now = Instant::now();
        if let Some(prev) = self.last_frame.replace(now) {
            let dt = (now - prev).as_secs_f32();
            if dt > 0.0 {
                // 지수 평균 — 숫자가 매 프레임 튀지 않게
                let instant = 1.0 / dt;
                self.fps = if self.fps == 0.0 {
                    instant
                } else {
                    self.fps * 0.9 + instant * 0.1
                };
            }
        }
    }

    /// UI 를 한 프레임 실행한다. 결과 액션은 [`Self::apply_actions`] 로 반영한다.
    fn run_ui(&mut self) -> Option<(UiFrame, UiActions)> {
        let (Some(ui), Some(target)) = (self.ui.as_mut(), self.target.as_ref()) else {
            return None;
        };

        let history = self.editing.history();
        let mut model = UiModel {
            camera: &mut self.camera,
            scene: &self.scene,
            selection: self.editing.selection(),
            hover: self.hover,
            undo_label: history.undo_label(),
            redo_label: history.redo_label(),
            stats: FrameStats {
                fps: self.fps,
                quads: self
                    .renderer
                    .as_ref()
                    .map_or(0, WgpuRenderer::last_quad_count),
                ticks: self.ticks,
            },
        };

        let (mut frame, actions) = ui.run(target, &mut model);
        self.texture_carry.apply_to(&mut frame);
        Some((frame, actions))
    }

    /// UI 가 요청한 편집을 씬에 반영한다. 모든 씬 변경은 `Editing` 을 거친다.
    fn apply_actions(&mut self, actions: &UiActions) {
        if actions.reset_view {
            self.reset_view();
        }
        self.manual_shot_requested |= actions.screenshot;

        if actions.undo {
            self.editing.undo(&mut self.scene);
        }
        if actions.redo {
            self.editing.redo(&mut self.scene);
        }
        if actions.deselect && !self.editing.is_dragging() {
            self.editing.clear_selection();
        }
        if let Some((target, additive)) = actions.list_select {
            self.editing.select(target, additive);
        }
        if let Some(edit) = actions.inspector {
            self.editing.apply_inspector(&mut self.scene, edit);
        }
        self.hover = actions
            .pointer
            .and_then(|p| self.editing.handle_pointer(&mut self.scene, &p));
    }

    /// 이번 프레임에 그릴 씬 명령을 쌓는다.
    fn build_commands(&mut self, viewport: Option<[u32; 4]>) {
        self.commands.clear();
        self.commands
            .push(RenderCommand::Clear { color: CLEAR_COLOR });
        if let Some([x, y, width, height]) = viewport {
            self.commands.push(RenderCommand::SetViewport {
                x,
                y,
                width,
                height,
            });
        }
        self.commands.push(RenderCommand::SetCamera {
            view_proj: self.camera.view_proj(),
        });

        grid::build(&self.camera, &mut self.commands);

        // 화면 픽셀 → 월드 길이. 선택 테두리·핸들을 화면상 일정한 크기로 그리는 데 쓴다.
        let px = self.camera.view_height / self.camera.viewport.1.max(1) as f32;

        self.draw_zone(px);

        for item in &self.scene.items {
            self.commands.push(RenderCommand::DrawRect {
                center: item.pos,
                size: Vec2::splat(item.size),
                z: Z_MARKER,
                color: item.kind.color(),
            });

            let target = Target::Item(item.entity);
            let (color, z, pad, thick) = if self.editing.is_selected(target) {
                (SELECT_COLOR, Z_SELECT, 4.0, 2.0)
            } else if self.hover == Some(Pick::Item(item.entity)) {
                (HOVER_COLOR, Z_HOVER, 3.0, 1.5)
            } else {
                continue;
            };
            let half = Vec2::splat(item.size * 0.5 + pad * px);
            grid::build_outline(
                item.pos - half,
                item.pos + half,
                thick * px,
                z,
                color,
                &mut self.commands,
            );
        }
    }

    fn draw_zone(&mut self, px: f32) {
        let zone = self.scene.zone;
        let selected = self.editing.is_selected(Target::Zone);
        let hovered = matches!(self.hover, Some(Pick::ZoneHandle(_)));

        let (color, thick) = if selected {
            (SELECT_COLOR, 2.5)
        } else if hovered {
            ([0.55, 0.75, 1.0, 1.0], 2.5)
        } else {
            (ZONE_BOUNDS_COLOR, 2.0)
        };
        grid::build_outline(
            zone.min,
            zone.max,
            thick * px,
            Z_ZONE,
            color,
            &mut self.commands,
        );

        if !selected {
            return;
        }
        // 핸들 8개 — 어두운 테두리 + 밝은 속. 호버 중인 핸들은 강조색.
        for handle in Handle::CORNERS.into_iter().chain(Handle::EDGES) {
            let at = zone.handle_pos(handle);
            let fill = if self.hover == Some(Pick::ZoneHandle(handle)) {
                SELECT_COLOR
            } else {
                HANDLE_FILL
            };
            self.commands.push(RenderCommand::DrawRect {
                center: at,
                size: Vec2::splat(11.0 * px),
                z: Z_HANDLE,
                color: HANDLE_BORDER,
            });
            self.commands.push(RenderCommand::DrawRect {
                center: at,
                size: Vec2::splat(8.0 * px),
                z: Z_HANDLE + 0.1,
                color: fill,
            });
        }
    }

    /// 이번 프레임에 캡처를 걸어야 하면 렌더러에 요청한다.
    fn schedule_capture(&mut self) {
        if self.pending_shot.is_some() {
            return;
        }

        let request = match &self.auto_shot {
            Some(plan) if self.frame_index >= plan.at_frame => Some((plan.path.clone(), true)),
            _ if self.manual_shot_requested => Some((screenshot::manual_path(), false)),
            _ => None,
        };
        let Some((path, exit_after)) = request else {
            return;
        };
        self.manual_shot_requested = false;
        if exit_after {
            self.auto_shot = None;
        }

        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        match renderer.request_capture() {
            Ok(()) => self.pending_shot = Some((path, exit_after)),
            Err(e) => {
                eprintln!("스크린샷 요청 실패: {e}");
                self.exit_requested |= exit_after;
            }
        }
    }

    /// 캡처 결과가 나왔으면 PNG 로 저장한다.
    fn collect_capture(&mut self) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        let Some(result) = renderer.take_capture() else {
            return;
        };
        let Some((path, exit_after)) = self.pending_shot.take() else {
            return;
        };

        match result.map_err(|e| e.to_string()).and_then(|capture| {
            screenshot::save_png(&path, &capture)
                .map(|()| capture)
                .map_err(|e| e.to_string())
        }) {
            Ok(capture) => println!(
                "스크린샷 저장 — {} ({}x{}, frame {})",
                path.display(),
                capture.width,
                capture.height,
                self.frame_index
            ),
            Err(e) => eprintln!("스크린샷 실패 ({}): {e}", path.display()),
        }

        self.exit_requested |= exit_after;
    }
}

impl App for Editor {
    fn init(&mut self, target: &WindowTarget) -> Result<(), String> {
        let (width, height) = target.size();

        // WindowTarget 은 Clone + Send + Sync + 'static 이므로 wgpu 서피스 타깃으로
        // 그대로 넘길 수 있다. 플랫폼 분기는 wgpu 내부에 있다.
        let renderer =
            WgpuRenderer::new(target.clone(), width, height).map_err(|e| e.to_string())?;

        let info = renderer.device_info();
        println!(
            "렌더러 초기화 — {} | {:?} | {} | {}x{} @ {:.2}x",
            info.name,
            info.backend,
            if info.is_integrated {
                "통합 GPU"
            } else {
                "외장 GPU"
            },
            width,
            height,
            target.scale_factor(),
        );

        let ui = EditorUi::new(target, renderer.max_texture_side());
        println!("{}", ui.font_note());

        self.camera.viewport = (width, height);
        self.reset_view();
        self.apply_env_selection();
        self.renderer = Some(renderer);
        self.ui = Some(ui);
        self.target = Some(target.clone());

        println!(
            "기본 존 로드 — 경계 {:?}~{:?}cm, 마커 {}개",
            self.scene.zone.min.to_array(),
            self.scene.zone.max.to_array(),
            self.scene.items.len()
        );

        self.auto_shot = screenshot::plan_from_env();
        if let Some(plan) = &self.auto_shot {
            println!(
                "자동 스크린샷 — frame {} → {}",
                plan.at_frame,
                plan.path.display()
            );
        }
        Ok(())
    }

    fn window_event(&mut self, target: &WindowTarget, event: &WindowEvent) -> bool {
        self.ui
            .as_mut()
            .is_some_and(|ui| ui.on_window_event(target, event))
    }

    fn fixed_update(&mut self, _dt: Duration, _input: &Input) {
        self.ticks += 1;
        // M7 에서 Intent → Authority → World 경로가 여기에 들어온다.
        // 에디터 조작(카메라·선택·편집)은 뷰·저작 작업이므로 UI 프레임에서 처리한다.
    }

    fn render(&mut self, _alpha: f32) {
        self.frame_index += 1;
        self.update_fps();

        let (mut ui_frame, viewport) = match self.run_ui() {
            Some((frame, actions)) => {
                self.apply_actions(&actions);
                (Some(frame), actions.viewport_px)
            }
            None => (None, None),
        };

        self.build_commands(viewport);
        self.schedule_capture();

        if let Some(renderer) = self.renderer.as_mut()
            && let Err(e) = draw(renderer, &self.commands, &mut ui_frame)
        {
            eprintln!("프레임 스킵: {e}");
        }

        // 그리지 못한 UI 프레임(최소화 등)의 텍스처 변경분은 다음 프레임으로 넘긴다.
        if let Some(frame) = ui_frame {
            self.texture_carry.stash(frame);
        }

        self.collect_capture();
    }

    fn resize(&mut self, width: u32, height: u32) {
        // 카메라 뷰포트는 UI 가 매 프레임 실제 씬 영역으로 다시 설정한다.
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.resize(width, height);
        }
    }

    fn should_exit(&self) -> bool {
        self.exit_requested
    }

    fn shutdown(&mut self) {
        println!(
            "종료 — 총 {} tick, 마커 {}, 선택 {}, 마지막 프레임 쿼드 {}",
            self.ticks,
            self.scene.items.len(),
            self.editing.selection().len(),
            self.renderer
                .as_ref()
                .map_or(0, WgpuRenderer::last_quad_count),
        );
        // egui 텍스처가 렌더러보다 먼저 사라지도록 UI 부터 정리한다.
        self.ui = None;
        // wgpu 는 Drop 에서 GPU 유휴 대기 후 자원을 해제한다.
        self.renderer = None;
    }
}

/// 한 프레임을 그린다. 프레임 생명주기를 한 곳에 모아 중간 반환 시에도
/// `end_frame` 누락이 눈에 띄게 한다.
///
/// `ui` 는 실제로 렌더러에 넘겼을 때만 비워진다. 프레임을 건너뛰면 그대로 남아
/// 호출자가 텍스처 변경분을 보관할 수 있다.
fn draw(
    renderer: &mut WgpuRenderer,
    commands: &[RenderCommand],
    ui: &mut Option<UiFrame>,
) -> Result<(), RenderError> {
    // 최소화·가려짐 상태는 오류가 아니라 정상적인 건너뛰기다.
    if renderer.begin_frame()? == FrameStatus::Skipped {
        return Ok(());
    }
    for &cmd in commands {
        renderer.submit(cmd)?;
    }
    if let Some(frame) = ui.take() {
        renderer.submit_ui(frame)?;
    }
    renderer.end_frame()
}
