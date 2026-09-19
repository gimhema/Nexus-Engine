//! Nexus WorldEditor — 에디터 애플리케이션.
//!
//! 현재 단계: M5-2 뷰포트 편집.
//!   - 왼쪽 클릭 선택 / Shift+클릭 추가·해제 / 끌어서 이동 / Ctrl+드래그 그리드 스냅
//!   - 빈 곳 드래그 박스 선택, 방향 화살표 + 회전 핸들(Ctrl 15°), 마커 추가 / Delete 삭제
//!   - 존 경계: 변·모서리 핸들을 끌어 크기 조절
//!   - 언두/리두 (Ctrl+Z / Ctrl+Y · Ctrl+Shift+Z), Esc 선택 해제
//!   - 인스펙터: 위치·방향·경계 값 편집 (나머지 필드는 패널 명세 대기)
//!   - 가운데·오른쪽 드래그 팬, 휠 줌, Home 존 전체, F 선택 항목 보기, F12 스크린샷
//!
//! 단위는 미터(m), 방향은 라디안 — `nexus_core::units`.
//!
//! 화면에 보이는 배치는 NexusEngine `Server.cpp` 의 기본 존 설정을 옮겨온 것이다.
//! M6 에서 `ZoneConfig` 로 내보낸다.

mod edit;
mod grid;
mod scene;
mod screenshot;
mod script;
mod sprites;
mod ui;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use nexus_core::{Camera2d, Vec2, units};
use nexus_platform::{App, Input, WindowConfig, WindowEvent, WindowTarget};
use nexus_render::{DEPTH_LAYER, FrameStatus, RenderCommand, RenderError, Renderer};
use nexus_render_wgpu::{TextureCarry, UiFrame, WgpuRenderer};

use edit::{Editing, PointerInput};
use scene::{Handle, Pick, Scene, Target};
use script::{Anchor, Button, Script, Step};
use sprites::MarkerSprites;
use ui::{EditorUi, FrameStats, UiActions, UiModel};

// 색은 모두 sRGB. 렌더러가 선형으로 변환한다.
const CLEAR_COLOR: [f32; 4] = [0.05, 0.06, 0.08, 1.0];
pub(crate) const ZONE_BOUNDS_COLOR: [f32; 4] = [0.35, 0.60, 0.95, 0.9];
const SELECT_COLOR: [f32; 4] = [1.0, 0.82, 0.25, 1.0];
const HOVER_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 0.55];
const HANDLE_FILL: [f32; 4] = [0.95, 0.97, 1.0, 1.0];
const HANDLE_BORDER: [f32; 4] = [0.05, 0.06, 0.08, 1.0];
/// 마커 안쪽 화살표 — 마커 색 위에서 보이는 어두운 색.
const ARROW_INNER: [f32; 4] = [0.08, 0.09, 0.12, 0.9];
/// 박스 선택 채움. 블렌딩이 선형 공간이라 어두운 배경에서는 알파가 훨씬 진하게 보인다
/// (0.04 → sRGB 약 0.23, 스크린샷으로 확인). 그래서 아주 낮게 둔다.
const BOX_FILL: [f32; 4] = [1.0, 0.82, 0.25, 0.012];

// 겹침 순서 — **월드 Z 가 아니라 깊이 편향**이다. 클수록 앞에 그려진다.
//
// 이것들은 전부 지면에 깔리는 표시이므로 월드 Z 는 0 이다. 월드 Z 로 순서를 주면
// 쿼터뷰에서 화면 세로 위치가 밀려 선택 테두리가 마커에서 떨어져 나간다.
const BIAS_ZONE: f32 = 1.0 * DEPTH_LAYER;
const BIAS_MARKER: f32 = 2.0 * DEPTH_LAYER;
/// 마커 스프라이트. 발밑 지면 표시 바로 위에 선다 — 빌보드는 발밑과 깊이가 같아
/// 편향 없이는 지면 표시와 z-파이팅이 난다.
const BIAS_SPRITE: f32 = 3.0 * DEPTH_LAYER;
const BIAS_ARROW: f32 = 4.0 * DEPTH_LAYER;
const BIAS_HOVER: f32 = 5.0 * DEPTH_LAYER;
const BIAS_SELECT: f32 = 6.0 * DEPTH_LAYER;
const BIAS_HANDLE: f32 = 7.0 * DEPTH_LAYER;
const BIAS_BOX: f32 = 9.0 * DEPTH_LAYER;

/// 가는 선의 최소 굵기 (픽셀). 1px 쿼드는 픽셀 경계에 걸리면 어떤 픽셀 중심도 덮지 못해
/// 통째로 사라진다 (그리드 선 위에 놓인 박스 테두리에서 확인).
const THIN_LINE_PX: f32 = 1.5;

/// 시작 시 미리 선택할 대상 (쉼표 구분 이름). 스크린샷으로 선택 표시를 검증할 때 쓴다.
const ENV_SELECT: &str = "NEXUS_SELECT";

/// 카메라 pitch 를 도(°) 단위로 지정한다. 90 = 탑다운, 45 = 쿼터뷰(기본).
const ENV_PITCH: &str = "NEXUS_PITCH";

/// 고정 줌 배율 (화면 px / 월드 m). 설정하면 자유 줌 대신 이 배율로 고정된다 — 게임 모드 확인용.
const ENV_PIXELS_PER_METER: &str = "NEXUS_PIXELS_PER_METER";

/// 카메라 시점 맞추기 요청.
///
/// 즉시 실행하지 않고 **UI 가 이번 프레임의 뷰포트를 확정한 뒤** 실행한다. 시작 직후에는
/// 뷰포트가 창 전체 크기였다가 패널이 자리 잡으며 좁아지므로, 그 전에 계산하면
/// 가로세로비가 어긋나 대상이 화면 밖으로 잘린다 (스크린샷으로 확인한 버그).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ViewRequest {
    /// 존 전체 (Home)
    Zone,
    /// 선택 항목, 없으면 모든 마커 (F)
    Selection,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = WindowConfig::default();

    println!(
        "WorldEditor 시작 — {}x{}, 시뮬레이션 {}Hz",
        config.width, config.height, config.sim_hz
    );
    println!(
        "조작: 왼쪽 = 선택/이동/박스 (Shift 추가, Ctrl 스냅) | ◆ = 회전 | Delete = 삭제 \
         | 가운데·오른쪽 드래그 = 팬 | 휠 = 줌 \
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
    /// 다음 뷰포트 확정 후 실행할 시점 맞추기.
    pending_view: Option<ViewRequest>,
    /// 프레임마다 재사용하는 명령 버퍼 — 매 프레임 할당을 피한다.
    commands: Vec<RenderCommand>,
    /// 마커 스프라이트 시트. 렌더러 초기화 후에 올라간다.
    sprites: Option<MarkerSprites>,
    /// 고정 줌 배율 (px/m). `Some` 이면 매 프레임 이 배율로 잠그고 픽셀 격자에 스냅한다.
    ///
    /// 게임 모드의 동작을 에디터에서 확인하기 위한 것이다 — S7 에서 플레이 모드의 기본이 된다.
    fixed_zoom: Option<f32>,

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

    // ── 자동 검증 ────────────────────────────────────────────────────────
    /// `NEXUS_SCRIPT` — 있으면 실제 포인터 대신 쓴다.
    script: Option<Script>,
    /// 스크립트 포인터의 현재 위치 (월드).
    script_cursor: Option<Vec2>,
    /// 스크립트가 누르고 있는 수식 키 (Shift, Ctrl) — 다음 포인터 단계까지 유지된다.
    script_mods: (bool, bool),
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
            pending_view: None,
            commands: Vec::new(),
            sprites: None,
            fixed_zoom: None,
            ticks: 0,
            last_frame: None,
            fps: 0.0,
            frame_index: 0,
            auto_shot: None,
            manual_shot_requested: false,
            pending_shot: None,
            exit_requested: false,
            script: None,
            script_cursor: None,
            script_mods: (false, false),
        }
    }
}

impl Editor {
    /// 존 전체가 보이도록 (Home).
    fn reset_view(&mut self) {
        let zone = self.scene.zone;
        self.frame_rect(zone.min, zone.max);
    }

    /// 선택 항목이 보이도록 (F). 선택이 없으면 모든 마커를 담는다.
    ///
    /// 미터 단위에서는 존(수 km)과 스폰 간격(수 m)의 차이가 커서, 존 전체 화면에서는
    /// 스폰들이 한 점에 모여 보인다. 편집하려면 이 기능으로 다가가야 한다.
    fn frame_selection(&mut self) {
        let targets: Vec<Target> = if self.editing.selection().is_empty() {
            self.scene
                .items
                .iter()
                .map(|i| Target::Item(i.entity))
                .collect()
        } else {
            self.editing.selection().to_vec()
        };

        let mut bounds: Option<(Vec2, Vec2)> = None;
        let mut include = |min: Vec2, max: Vec2| {
            bounds = Some(match bounds {
                Some((a, b)) => (a.min(min), b.max(max)),
                None => (min, max),
            });
        };
        for target in targets {
            match target {
                Target::Zone => include(self.scene.zone.min, self.scene.zone.max),
                Target::Item(e) => {
                    if let Some(item) = self.scene.item(e) {
                        let half = Vec2::splat(item.size * 0.5);
                        include(item.pos - half, item.pos + half);
                    }
                }
            }
        }
        if let Some((min, max)) = bounds {
            self.frame_rect(min, max);
        }
    }

    /// 사각형이 뷰포트에 여유 있게 들어오도록 카메라를 맞춘다.
    fn frame_rect(&mut self, min: Vec2, max: Vec2) {
        /// 너무 작은 대상(마커 하나)을 잡았을 때도 주변이 보이도록 하는 최소 시야 (m).
        const MIN_FRAME_HEIGHT: f32 = 20.0;
        /// 가장자리 여유.
        const MARGIN: f32 = 1.2;

        let size = max - min;
        let height = size.y.max(size.x / self.camera.aspect()) * MARGIN;
        self.camera.center = (min + max) * 0.5;
        self.camera.view_height = height
            .max(MIN_FRAME_HEIGHT)
            .clamp(Camera2d::MIN_VIEW_HEIGHT, Camera2d::MAX_VIEW_HEIGHT);
    }

    /// `NEXUS_SELECT` 로 지정된 대상을 미리 선택하고 그쪽으로 시점을 맞춘다.
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
        if !self.editing.selection().is_empty() {
            self.pending_view = Some(ViewRequest::Selection);
        }
    }

    /// `NEXUS_PITCH` / `NEXUS_PIXELS_PER_METER` 를 반영한다.
    fn apply_env_camera(&mut self) {
        if let Ok(v) = std::env::var(ENV_PITCH) {
            match v.parse::<f32>() {
                Ok(deg) => self.camera.pitch = deg.to_radians(),
                Err(_) => eprintln!("{ENV_PITCH}: '{v}' 는 숫자가 아님"),
            }
        }
        if let Ok(v) = std::env::var(ENV_PIXELS_PER_METER) {
            match v.parse::<f32>() {
                Ok(ppm) => self.fixed_zoom = Some(ppm),
                Err(_) => eprintln!("{ENV_PIXELS_PER_METER}: '{v}' 는 숫자가 아님"),
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
            self.pending_view = Some(ViewRequest::Zone);
        }
        if actions.frame_selection {
            self.pending_view = Some(ViewRequest::Selection);
        }
        self.manual_shot_requested |= actions.screenshot;

        if actions.undo {
            self.editing.undo(&mut self.scene);
        }
        if actions.redo {
            self.editing.redo(&mut self.scene);
        }
        if let Some(pitch) = actions.set_pitch {
            self.camera.pitch = pitch;
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
        if let Some(kind) = actions.add_item {
            // 지금 보고 있는 곳 한가운데에 놓는다
            self.editing
                .add_item(&mut self.scene, kind, self.camera.center);
        }
        if actions.delete && !self.editing.is_dragging() {
            self.editing.delete_selected(&mut self.scene);
        }

        let pointer = match actions.pointer {
            Some(p) if self.script.is_some() => Some(self.scripted_pointer(p, actions)),
            other => other,
        };
        self.hover = pointer.and_then(|p| self.editing.handle_pointer(&mut self.scene, &p));

        // 고정 줌이면 자유 줌·시점 맞추기 결과를 덮어쓴다. 뷰포트가 확정된 뒤여야
        // 배율 계산이 맞는다. 게임 모드가 생기는 S7 에서는 이쪽이 기본이 된다.
        if let Some(ppm) = self.fixed_zoom
            && actions.viewport_px.is_some()
        {
            self.camera.set_pixels_per_meter(ppm);
            self.camera.snap_to_pixel_grid();
        }

        // UI 가 이번 프레임의 뷰포트 크기를 `camera.viewport` 에 이미 반영했다 — 이제 맞춘다.
        // 뷰포트가 없는 프레임(UI 미실행)에는 다음 프레임으로 미룬다.
        if actions.viewport_px.is_some()
            && let Some(request) = self.pending_view.take()
        {
            match request {
                ViewRequest::Zone => self.reset_view(),
                ViewRequest::Selection => self.frame_selection(),
            }
        }
    }

    /// `NEXUS_SCRIPT` 가 있으면 실제 포인터 대신 스크립트 단계를 포인터 입력으로 만든다.
    ///
    /// 수식 키는 실제 키보드처럼 다음 포인터 단계까지 눌린 채로 둔다 — 한 프레임만 적용하면
    /// 이어지는 프레임의 드래그 갱신이 스냅을 풀어 버린다 (스크린샷으로 확인).
    ///
    /// 스크립트가 끝난 뒤에도 실제 마우스는 계속 무시한다 — 창이 뜬 자리의 마우스가
    /// 끄는 중인 박스를 움직여 스크린샷이 흔들리지 않도록.
    fn scripted_pointer(&mut self, real: PointerInput, actions: &UiActions) -> PointerInput {
        let mut p = PointerInput {
            world: self.script_cursor,
            over_viewport: self.script_cursor.is_some(),
            pressed: false,
            released: false,
            additive: self.script_mods.0,
            snap: self.script_mods.1,
            ..real
        };
        // 시점 맞추기가 끝난 뒤에 시작한다 — 핸들 위치가 배율에 따라 달라지므로.
        if actions.viewport_px.is_none() || self.pending_view.is_some() {
            return p;
        }
        let Some(step) = self.script.as_mut().and_then(Script::next) else {
            return p;
        };
        println!("{}: {step:?}", script::ENV_SCRIPT);

        match step {
            Step::Pointer {
                button,
                at,
                shift,
                ctrl,
            } => {
                let world = match at {
                    Anchor::World(w) => Some(w),
                    Anchor::RotateHandle => self
                        .editing
                        .rotatable()
                        .and_then(|e| self.scene.item(e))
                        .map(|item| scene::rotate_handle_pos(item, real.px)),
                };
                self.script_cursor = world.or(self.script_cursor);
                p.world = self.script_cursor;
                p.over_viewport = true;
                p.pressed = button == Button::Press;
                p.released = button == Button::Release;
                self.script_mods = (shift, ctrl);
                p.additive = shift;
                p.snap = ctrl;
            }
            Step::Add(kind, pos) => {
                self.editing.add_item(&mut self.scene, kind, pos);
            }
            Step::Delete => {
                self.editing.delete_selected(&mut self.scene);
            }
            Step::Undo => {
                self.editing.undo(&mut self.scene);
            }
            Step::Redo => {
                self.editing.redo(&mut self.scene);
            }
            Step::Wait => {}
        }
        p
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
        let (right, up, _) = self.camera.basis();
        self.commands.push(RenderCommand::SetCamera {
            view_proj: self.camera.view_proj(),
            right,
            up,
        });

        grid::build(&self.camera, &mut self.commands);

        // 화면 픽셀 → 월드 길이. 선택 테두리·핸들을 화면상 일정한 크기로 그리는 데 쓴다.
        let px = self.camera.view_height / self.camera.viewport.1.max(1) as f32;

        self.draw_zone(px);

        for item in &self.scene.items {
            // 발판(지면 테두리) + 그 위에 서는 스프라이트.
            //
            // 발판을 채우지 않는 이유는 보기 문제만이 아니다 — 지면 쿼드는 Y 방향으로
            // 퍼져 있어 **먼 쪽 절반이 빌보드보다 앞선다**(빌보드 깊이는 발밑 한 점으로
            // 정해지므로). 채우면 스프라이트 다리가 통째로 가려진다.
            // 테두리면 먼 변이 발끝을 살짝 가리는데, 그건 오히려 올바른 앞뒤 관계다.
            let half = Vec2::splat(scene::marker_half_extent(item, px));
            grid::build_outline(
                &self.camera,
                item.pos - half,
                item.pos + half,
                THIN_LINE_PX,
                BIAS_MARKER,
                item.kind.color(),
                &mut self.commands,
            );
            if let Some(sprites) = &self.sprites {
                sprites.build(item, px, BIAS_SPRITE, &mut self.commands);
            }
            draw_arrow(item, px, &mut self.commands);

            let target = Target::Item(item.entity);
            let (color, z, pad, thick) = if self.editing.is_selected(target) {
                (SELECT_COLOR, BIAS_SELECT, 4.0, 2.0)
            } else if self.hover == Some(Pick::Item(item.entity)) {
                (HOVER_COLOR, BIAS_HOVER, 3.0, 1.5)
            } else {
                continue;
            };
            let half = Vec2::splat(scene::marker_half_extent(item, px) + pad * px);
            grid::build_outline(
                &self.camera,
                item.pos - half,
                item.pos + half,
                thick,
                z,
                color,
                &mut self.commands,
            );
        }

        self.draw_rotate_handle(px);
        self.draw_box_selection();
    }

    /// 단독 선택된 마커의 회전 핸들 — 화살표 끝에서 이어지는 가는 선 + 원 대신 마름모.
    fn draw_rotate_handle(&mut self, px: f32) {
        let Some(item) = self.editing.rotatable().and_then(|e| self.scene.item(e)) else {
            return;
        };
        let tip = scene::arrow_tip(item, px);
        let at = scene::rotate_handle_pos(item, px);
        push_segment(
            tip,
            at,
            THIN_LINE_PX * px,
            BIAS_HANDLE,
            SELECT_COLOR,
            &mut self.commands,
        );

        let hot = self.hover == Some(Pick::RotateHandle(item.entity));
        let diamond = std::f32::consts::FRAC_PI_4;
        self.commands.push(RenderCommand::DrawRect {
            center: at,
            size: Vec2::splat(scene::HANDLE_SIZE_PX * px),
            rotation: diamond,
            z: 0.0,
            depth_bias: BIAS_HANDLE,
            color: HANDLE_BORDER,
        });
        self.commands.push(RenderCommand::DrawRect {
            center: at,
            size: Vec2::splat((scene::HANDLE_SIZE_PX - 3.0) * px),
            rotation: diamond,
            z: 0.0,
            depth_bias: BIAS_HANDLE + DEPTH_LAYER,
            color: if hot { SELECT_COLOR } else { HANDLE_FILL },
        });
    }

    /// 진행 중인 박스 선택 — 반투명 채움 + 테두리. 모든 것 위에 그린다.
    fn draw_box_selection(&mut self) {
        let Some((min, max)) = self.editing.box_rect() else {
            return;
        };
        self.commands.push(RenderCommand::DrawRect {
            center: (min + max) * 0.5,
            size: max - min,
            rotation: 0.0,
            z: 0.0,
            depth_bias: BIAS_BOX,
            color: BOX_FILL,
        });
        grid::build_outline(
            &self.camera,
            min,
            max,
            THIN_LINE_PX,
            BIAS_BOX + DEPTH_LAYER,
            SELECT_COLOR,
            &mut self.commands,
        );
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
            &self.camera,
            zone.min,
            zone.max,
            thick,
            BIAS_ZONE,
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
                rotation: 0.0,
                center: at,
                size: Vec2::splat(scene::HANDLE_SIZE_PX * px),
                z: 0.0,
                depth_bias: BIAS_HANDLE,
                color: HANDLE_BORDER,
            });
            self.commands.push(RenderCommand::DrawRect {
                rotation: 0.0,
                center: at,
                size: Vec2::splat(8.0 * px),
                z: 0.0,
                depth_bias: BIAS_HANDLE + DEPTH_LAYER,
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
        let mut renderer =
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
        self.pending_view = Some(ViewRequest::Zone);
        self.apply_env_camera();
        self.apply_env_selection();
        self.script = Script::from_env();

        // 마커 스프라이트. 실패해도 에디터는 돌아간다 — 마커가 지면 사각형만 남을 뿐이다.
        match MarkerSprites::load(&mut renderer) {
            Ok(s) => self.sprites = Some(s),
            Err(e) => eprintln!("마커 스프라이트 로드 실패: {e}"),
        }

        self.renderer = Some(renderer);
        self.ui = Some(ui);
        self.target = Some(target.clone());

        println!(
            "기본 존 로드 — 경계 {:?}~{:?} m, 마커 {}개",
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

    fn fixed_update(&mut self, dt: Duration, _input: &Input) {
        self.ticks += 1;
        // 애니메이션은 여기서만 진행한다 — 렌더 프레임에서 진행하면
        // 프레임레이트에 따라 속도가 달라진다.
        if let Some(sprites) = self.sprites.as_mut() {
            sprites.advance(dt);
        }
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

/// 마커의 방향 화살표.
///
/// 마커 안쪽 구간은 어두운 색(마커 색 위에서 보이도록), 바깥 구간과 촉은 마커 색
/// (어두운 배경 위에서 보이도록)으로 그린다. 굵기·촉 크기는 화면 픽셀 기준이다.
fn draw_arrow(item: &scene::Item, px: f32, out: &mut Vec<RenderCommand>) {
    const THICK_PX: f32 = 2.0;
    const HEAD_PX: f32 = 6.0;
    /// 촉 날개가 화살표 반대쪽으로 벌어진 각도.
    const HEAD_SPREAD: f32 = 2.5;

    let dir = units::heading_to_dir(item.orientation);
    let edge = item.pos + dir * scene::marker_half_extent(item, px);
    let tip = scene::arrow_tip(item, px);
    let color = item.kind.color();

    push_segment(item.pos, edge, THICK_PX * px, BIAS_ARROW, ARROW_INNER, out);
    push_segment(edge, tip, THICK_PX * px, BIAS_ARROW, color, out);
    for side in [HEAD_SPREAD, -HEAD_SPREAD] {
        let wing = tip + units::heading_to_dir(item.orientation + side) * HEAD_PX * px;
        push_segment(tip, wing, THICK_PX * px, BIAS_ARROW, color, out);
    }
}

/// `a` 에서 `b` 까지 굵기 `thick` 의 선분 (회전된 사각형 하나).
fn push_segment(
    a: Vec2,
    b: Vec2,
    thick: f32,
    depth_bias: f32,
    color: [f32; 4],
    out: &mut Vec<RenderCommand>,
) {
    let d = b - a;
    let len = d.length();
    if len <= f32::EPSILON {
        return;
    }
    out.push(RenderCommand::DrawRect {
        center: (a + b) * 0.5,
        // 양 끝을 굵기의 절반씩 늘려 꺾이는 곳(촉)에 틈이 생기지 않게 한다
        size: Vec2::new(len + thick, thick),
        rotation: units::dir_to_heading(d),
        z: 0.0,
        depth_bias,
        color,
    });
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
