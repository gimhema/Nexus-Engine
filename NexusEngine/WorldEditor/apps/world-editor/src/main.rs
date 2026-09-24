//! Nexus WorldEditor — 에디터 애플리케이션.
//!
//! 현재 단계: S7-1 플레이 모드 (F5) — 편집 중인 씬을 그 자리에서 시뮬레이션으로 돌린다.
//! 편집 기능 (M5):
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
//! 존 파일(`zones/*.zone.ron`)로 저장·로드한다 (S7-2). 서버 `ZoneConfig` 내보내기는 단계 2.

mod actor_editor;
mod content;
mod edit;
mod game_data;
mod grid;
mod level;
mod level_editor;
mod palette;
mod play;
mod ron_patch;
mod save_file;
mod scene;
mod screen;
mod screen_editor;
mod screenshot;
mod script;
mod script_editor;
mod sheet_viewer;
mod sprites;
mod terrain;
mod tiles;
mod ui;
mod zone_file;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use nexus_core::{Camera2d, Entity, Vec2, units};
use nexus_platform::{App, Input, WindowConfig, WindowEvent, WindowTarget};
use nexus_render::{
    DEPTH_LAYER, DrawLayer, FrameStatus, RenderCommand, RenderError, Renderer, TextureId, UvRect,
};
use nexus_render_wgpu::{TextureCarry, UiFrame, WgpuRenderer};

use edit::{Editing, PointerInput, Tool};
use game_data::GameData;
use level::{Levels, Shell};
use play::{PlayOptions, PlaySession};
use scene::{Handle, Pick, Scene, Target};
use screen::{Screens, Values};
use screen_editor::ScreenEditor;
use script::{Anchor, Button, Script, Step};
use sprites::SpriteLibrary;
use terrain::Terrain;
use ui::{EditorUi, FrameStats, Notice, UiActions, UiModel};

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
/// 지형 그림 — 지면 층 맨 아래. 규칙 색(`BIAS_TILE`)이 이 위에 얹힌다.
/// 플레이 중 띄우는 HUD 화면 번호 — 레벨마다 고를 수 있게 되기 전의 기본값이다.
const HUD_SCREEN: &str = "hud";

const BIAS_ART: f32 = 0.25 * DEPTH_LAYER;
/// 타일 — 지면 층에서 그리드 바로 위.
const BIAS_TILE: f32 = 0.5 * DEPTH_LAYER;
const BIAS_ZONE: f32 = 1.0 * DEPTH_LAYER;
const BIAS_MARKER: f32 = 2.0 * DEPTH_LAYER;
/// 마커 스프라이트. 발밑 지면 표시 바로 위에 선다 — 빌보드는 발밑과 깊이가 같아
/// 편향 없이는 지면 표시와 z-파이팅이 난다.
const BIAS_SPRITE: f32 = 3.0 * DEPTH_LAYER;
/// 정적 오브젝트(건물). 캐릭터와 같은 층이라 **깊이(발밑 Y)로 앞뒤가 정해진다** —
/// 편향은 지면 표시와 겹치지 않을 만큼만 준다.
const BIAS_PROP: f32 = 3.0 * DEPTH_LAYER;
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

/// 시작할 때 열 존 파일 경로.
const ENV_ZONE: &str = "NEXUS_ZONE";

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
         | Ctrl+Z/Y = 실행 취소/다시 | Esc = 선택 해제 | Home | F5 = 플레이 | F12"
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
    sprites: Option<SpriteLibrary>,
    /// 지형 그림(타일 아트·건물). 렌더러 초기화 후에 올라간다. 실패하면 비어 있다.
    terrain: Terrain,
    /// 화면(UI) — 테마와 화면 파일들 (P5·P7). 렌더러 초기화 후에 올라간다.
    screens: Screens,
    /// HUD 인벤토리 창을 보여 준다 (I).
    show_items: bool,
    /// 에디터 패널을 숨긴다 (F9) — HUD 만으로 플레이되는지 확인하는 용도.
    hide_panels: bool,
    /// 위젯 편집기 (P7) — 화면의 위젯 트리를 고친다.
    screen_editor: ScreenEditor,
    /// 레벨 목록과 프로젝트 설정 (P7) — 언리얼의 레벨 관리에 대응한다.
    levels: Levels,
    /// 게임 셸 — 열린 레벨과 화면 스택. `None` 이면 에디터 편집 중이다.
    shell: Option<Shell>,
    /// 화면 버튼 상태 — 마우스가 올라간 것 / 누르고 있는 것.
    ui_hover: Option<String>,
    ui_press: Option<String>,
    /// 에디터가 보는 게임 데이터 — **액터 타입 목록**(인스펙터)과 마커 그림에 쓴다.
    ///
    /// 플레이는 시작할 때 자기 것을 새로 읽는다. 이쪽도 그때 함께 갱신해 에디터와 플레이가
    /// 같은 데이터를 본다. 못 읽으면 `None` — 마커는 종류 기본값으로 그려진다.
    data: Option<GameData>,
    /// 고정 줌 배율 (px/m). `Some` 이면 매 프레임 이 배율로 잠그고 픽셀 격자에 스냅한다.
    ///
    /// 게임 모드의 동작을 에디터에서 확인하기 위한 것이다 — S7 에서 플레이 모드의 기본이 된다.
    fixed_zoom: Option<f32>,
    /// 진행 중인 플레이 (F5). 있으면 편집 대신 시뮬레이션을 보여 주고 조작한다.
    play: Option<PlaySession>,
    /// 열어 둔 존 파일. 새 씬이면 `None` — 저장할 때 경로를 묻는다.
    zone_path: Option<PathBuf>,
    /// 마지막으로 저장·연 시점의 편집 상태 번호 (`History::state_id`). 다르면 저장 안 됨.
    saved_state: u64,
    /// 상태 바에 띄울 마지막 알림 (저장·열기·플레이 실패 이유 등).
    notice: Option<Notice>,

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
            terrain: Terrain::default(),
            screens: Screens::default(),
            show_items: false,
            hide_panels: false,
            screen_editor: ScreenEditor::default(),
            levels: Levels::default(),
            shell: None,
            ui_hover: None,
            ui_press: None,
            data: None,
            fixed_zoom: None,
            play: None,
            zone_path: None,
            saved_state: 0,
            notice: None,
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
        let dirty = history.state_id() != self.saved_state;
        let mut model = UiModel {
            camera: &mut self.camera,
            scene: &self.scene,
            selection: self.editing.selection(),
            hover: self.hover,
            undo_label: history.undo_label(),
            redo_label: history.redo_label(),
            tool: self.editing.tool(),
            brush: self.editing.brush(),
            actor_catalog: self
                .data
                .as_ref()
                .map(GameData::actor_catalog)
                .unwrap_or_default(),
            actor_defaults: scene::ItemKind::ALL.map(|kind| {
                self.data
                    .as_ref()
                    .map_or(scene::ActorId::DEFAULT, |d| d.default_actor(kind))
            }),
            actor_scripts: self.data.as_ref().map_or_else(Vec::new, |d| {
                d.actor_catalog()
                    .into_iter()
                    .filter_map(|(id, name, _)| {
                        let path = d.actor_script(id)?;
                        Some((path.to_owned(), format!("{name} (#{})", id.raw())))
                    })
                    .collect()
            }),
            art_brush: self.editing.art_brush(),
            art_layer: self.editing.art_layer(),
            brush_shape: self.editing.brush_shape(),
            brush_radius: self.editing.brush_radius(),
            eyedropper: self.editing.eyedropper(),
            art_palette: self.terrain.list(),
            play: self.play.as_ref(),
            hide_panels: self.hide_panels,
            levels: self.levels.list(),
            startup_level: self.levels.startup().to_owned(),
            level: self.shell.as_ref().map(|s| s.level.clone()),
            screens: &self.screens,
            screen_editor: &mut self.screen_editor,
            zone_path: self.zone_path.as_deref(),
            dirty,
            notice: self.notice.as_ref(),
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

    /// 사용자에게 알릴 일 — 콘솔에 찍고 상태 바에도 남긴다. 창만 보는 사람도 실패 이유를 알 수 있게.
    fn notify(&mut self, message: String, error: bool) {
        if error {
            eprintln!("{message}");
        } else {
            println!("{message}");
        }
        self.notice = Some(Notice { message, error });
    }

    /// 존 파일로 저장한다. 성공하면 그 경로가 "열어 둔 파일" 이 되고 저장 안 됨 표시가 풀린다.
    ///
    /// 플레이 중에도 저장할 수 있다 — 저장하는 것은 편집 중인 씬이지 플레이 결과가 아니다.
    fn save_zone(&mut self, path: PathBuf) {
        match zone_file::save(&self.scene, &path) {
            Ok(()) => {
                self.notify(format!("존 저장 — {}", path.display()), false);
                self.saved_state = self.editing.history().state_id();
                self.zone_path = Some(path);
            }
            Err(e) => self.notify(format!("존 저장 실패: {e}"), true),
        }
    }

    /// 존 파일을 연다. 실패하면 지금 씬을 그대로 둔다.
    ///
    /// 여는 것은 편집이 아니라 **언두 기록을 비운다** — 다른 파일의 편집을 되돌리면 안 된다.
    /// 도구·붓은 유지한다.
    fn open_zone(&mut self, path: PathBuf) {
        let scene = match zone_file::load(&path) {
            Ok(scene) => scene,
            Err(e) => {
                self.notify(format!("존 열기 실패: {e}"), true);
                return;
            }
        };
        if self.play.is_some() {
            self.toggle_play();
        }
        self.notify(
            format!(
                "존 열기 — {} (마커 {}개)",
                path.display(),
                scene.items.len()
            ),
            false,
        );
        let (tool, brush) = (self.editing.tool(), self.editing.brush());
        self.editing = Editing::default();
        self.editing.set_tool(tool);
        self.editing.set_brush(brush);
        self.saved_state = self.editing.history().state_id();
        self.scene = scene;
        self.zone_path = Some(path);
        self.hover = None;
        // 선택이 없으면 모든 마커가 보이도록 맞춘다.
        self.pending_view = Some(ViewRequest::Selection);
    }

    /// 마커가 실제로 쓸 액터 타입과 그 타입이 **명시한** 색(없으면 `NO_TINT`).
    /// 데이터를 못 읽었으면 종류 기본 타입으로.
    fn actor_look_of(&self, item: &scene::Item) -> (scene::ActorId, [f32; 4]) {
        let Some(data) = &self.data else {
            return (scene::ActorId::DEFAULT, sprites::NO_TINT);
        };
        let actor = data.resolve_actor(item.actor, item.kind);
        let tint = data
            .actor_look(actor)
            .map_or(sprites::NO_TINT, game_data::ActorLook::tint);
        (actor, tint)
    }

    /// 열어 둔 존 파일 이름 (`zones/village.zone.ron`). 새 씬이면 빈 문자열.
    fn zone_name(&self) -> String {
        self.zone_path
            .as_ref()
            .map(|p| p.display().to_string().replace('\\', "/"))
            .unwrap_or_default()
    }

    /// 고른 플레이어 스폰 마커 — 여기서 시작한다 (P3-1). 하나만 골랐을 때만.
    fn selected_player_spawn(&self) -> Option<Entity> {
        match self.editing.selection() {
            [Target::Item(e)] => self
                .scene
                .item(*e)
                .filter(|i| i.kind == scene::ItemKind::PlayerSpawn)
                .map(|i| i.entity),
            _ => None,
        }
    }

    /// 진행 상황을 저장한다 (P3-4 — 수동 저장과 정상 종료).
    fn save_game(&mut self, session: &PlaySession) {
        let data = session.save_data(self.zone_name());
        match save_file::save(&save_file::default_path(), &data) {
            Ok(()) => self.notify(
                format!("저장 — 레벨 {} ({})", data.level, save_file::DEFAULT_SAVE),
                false,
            ),
            Err(e) => self.notify(format!("저장하지 못했습니다 — {e}"), true),
        }
    }

    /// 플레이 시작 / 정지. 시작하면 씬을 복사해 시뮬레이션을 만들고, 정지하면 버린다 —
    /// 씬과 언두 기록은 건드리지 않는다.
    fn toggle_play(&mut self) {
        if self.play.is_some() || self.shell.is_some() {
            self.stop_play();
        } else {
            self.start_play();
        }
    }

    /// 플레이를 멈추고 에디터로 돌아온다. 진행 상황은 저장한다 (P3-4 정상 종료 경로).
    fn stop_play(&mut self) {
        self.shell = None;
        self.ui_hover = None;
        self.ui_press = None;
        if let Some(session) = self.play.take() {
            self.camera = *session.editor_camera();
            println!("플레이 정지 — {} tick 진행", session.ticks());
            self.save_game(&session);
        }
    }

    /// 지금 편집 중인 씬으로 플레이를 시작한다 (F5). 화면은 이 존이 속한 레벨이 정한다.
    fn start_play(&mut self) {
        // 스크립트 편집기에 저장하지 않은 변경이 있으면 먼저 저장한다 — 플레이는 디스크의 파일로 돈다.
        // 고친 것이 적용되지 않은 채 옛 스크립트로 플레이하면 무엇이 문제인지 알기 어렵다.
        if let Some(saved) = self.ui.as_mut().and_then(EditorUi::save_dirty_script) {
            match saved {
                Ok(path) => self.notify(format!("플레이 전에 {path} 저장"), false),
                Err(e) => {
                    self.notify(
                        format!("스크립트를 저장하지 못해 플레이하지 않음 — {e}"),
                        true,
                    );
                    return;
                }
            }
        }
        // 데이터는 시작할 때마다 새로 읽는다 — `data/*.ron` 을 고치고 F5 만 다시 누르면 된다.
        let data = match GameData::load() {
            Ok(data) => data,
            Err(e) => {
                self.notify(format!("플레이할 수 없음: {e}"), true);
                return;
            }
        };
        // 에디터가 보는 목록도 같이 갱신한다 — 인스펙터와 플레이가 같은 데이터를 봐야 한다.
        // (시트 그림은 시작할 때 GPU 에 한 번 올리므로 교체하려면 다시 시작해야 한다.)
        match GameData::load() {
            Ok(fresh) => self.data = Some(fresh),
            Err(e) => self.notify(format!("액터 목록을 갱신하지 못했습니다 — {e}"), true),
        }
        // 저장 데이터가 있으면 이어서 한다. 다른 존이면 위치만 버리고 캐릭터는 이어진다.
        let save = match save_file::load(&save_file::default_path()) {
            Ok(save) => save,
            Err(e) => {
                self.notify(format!("저장 파일을 읽지 못해 새로 시작합니다 — {e}"), true);
                None
            }
        };
        let save = save.map(|mut save| {
            let here = self.zone_name();
            if save.zone != here {
                save.pos = None;
            }
            save
        });
        if let Some(save) = &save {
            self.notify(
                format!("이어 하기 — 레벨 {} (저장: {})", save.level, save.zone),
                false,
            );
        }
        let options = PlayOptions {
            spawn: self.selected_player_spawn(),
            save,
        };
        match PlaySession::start(&self.scene, self.camera, data, options) {
            Ok(session) => {
                println!("플레이 시작 — 유닛 {}명", session.world().unit_count());
                self.hover = None;
                // 플레이 중에는 카메라가 플레이어를 따라가므로 예약된 시점 맞추기는 의미가 없다.
                self.pending_view = None;
                self.camera.pitch = Camera2d::PITCH_QUARTER;
                self.play = Some(session);
                self.shell = Some(self.shell_for_zone());
            }
            Err(e) => self.notify(format!("플레이할 수 없음: {e}"), true),
        }
    }

    /// 지금 열린 존 파일이 속한 레벨로 셸을 만든다. 레벨을 못 찾으면 기본 HUD 만 띄운다 —
    /// 저장하지 않은 새 씬으로도 플레이할 수 있어야 한다.
    fn shell_for_zone(&self) -> Shell {
        match self.levels.find_by_zone(self.zone_path.as_deref()) {
            Some(id) => {
                let level = self.levels.level(id).expect("방금 찾은 레벨");
                Shell::new(id, level.base_screen())
            }
            None => Shell::new("", Some(HUD_SCREEN)),
        }
    }

    /// 레벨을 연다 — 언리얼의 `Open Level`. 존이 있으면 그 존 파일을 열고 플레이를 시작하고,
    /// 없으면 (메인 화면처럼) 시뮬레이션 없이 화면만 띄운다.
    ///
    /// **존이 있는 레벨은 에디터의 씬을 바꾼다** (언리얼의 PIE 가 맵을 여는 것과 같다).
    /// 그래서 저장하지 않은 편집이 있으면 열지 않고 알린다.
    fn open_level(&mut self, id: &str) {
        let Some(level) = self.levels.level(id).cloned() else {
            self.notify(format!("'{id}' 레벨이 없습니다"), true);
            return;
        };
        match level.zone.as_deref() {
            Some(zone) => {
                if self.is_dirty() {
                    self.notify(
                        format!(
                            "저장하지 않은 편집이 있어 '{}' 을 열지 않았습니다 — 먼저 저장하세요",
                            level.name
                        ),
                        true,
                    );
                    return;
                }
                let same = self
                    .levels
                    .find_by_zone(self.zone_path.as_deref())
                    .is_some_and(|here| here == id);
                if !same {
                    self.open_zone(PathBuf::from(zone));
                }
                self.stop_play();
                self.start_play();
            }
            None => {
                self.stop_play();
                self.shell = Some(Shell::new(id, level.base_screen()));
                self.notify(format!("레벨 '{}' — {}", id, level.name), false);
            }
        }
    }

    /// 콘텐츠 브라우저·편집기가 에디터에 맡긴 일 (P8).
    fn apply_content_actions(&mut self, actions: &UiActions) {
        match &actions.content {
            Some(content::ContentAction::EditScreen(id)) => {
                if !self.screen_editor.is_open() {
                    self.screen_editor.toggle(&self.screens);
                }
                self.screen_editor.switch(&self.screens, id);
                if let Some((error, message)) = self.screen_editor.take_status() {
                    self.notify(message, error);
                }
            }
            Some(content::ContentAction::NewScreen) => {
                if !self.screen_editor.is_open() {
                    self.screen_editor.toggle(&self.screens);
                }
                self.notify(
                    String::from("위젯 편집기의 '+ 새 화면' 에서 번호를 적어 만드세요"),
                    false,
                );
            }
            _ => {}
        }
        if actions.reload_data {
            match GameData::load() {
                Ok(data) => self.data = Some(data),
                Err(e) => self.notify(format!("게임 데이터를 다시 읽지 못했습니다 — {e}"), true),
            }
        }
        if actions.reload_levels {
            let (levels, warnings) = Levels::load();
            self.levels = levels;
            for w in warnings {
                self.notify(w, true);
            }
        }
        if let Some(text) = &actions.notice {
            self.notify(text.clone(), false);
        }
    }

    /// 마지막 저장 이후 씬이 바뀌었는가 — 상태 바의 `*` 와 같은 판정.
    fn is_dirty(&self) -> bool {
        self.editing.history().state_id() != self.saved_state
    }

    /// 화면 버튼의 동작 — 데이터에 적힌 [`screen::Action`] 을 실행한다.
    fn run_action(&mut self, action: screen::Action) {
        use screen::Action;
        match action {
            Action::OpenScreen(id) => {
                if self.screens.screen(&id).is_none() {
                    self.notify(format!("'{id}' 화면이 없습니다"), true);
                } else if let Some(shell) = self.shell.as_mut() {
                    shell.push(&id);
                }
            }
            Action::Close => {
                if let Some(shell) = self.shell.as_mut()
                    && shell.pop()
                {
                    shell.paused = shell.screens.len() > 1;
                }
            }
            Action::Resume => {
                if let Some(shell) = self.shell.as_mut() {
                    while shell.pop() {}
                    shell.paused = false;
                }
            }
            Action::OpenLevel(id) => self.open_level(&id),
            Action::OpenStartLevel => {
                let id = self.levels.startup().to_owned();
                self.open_level(&id);
            }
            // 에디터에서는 창을 닫지 않고 플레이를 멈춘다 — 편집하던 씬으로 돌아온다.
            Action::Quit => {
                self.stop_play();
                self.notify(
                    String::from("게임을 종료했습니다 — 에디터로 돌아왔습니다"),
                    false,
                );
            }
            Action::None => {}
        }
    }

    /// 화면 버튼에 포인터를 넘긴다. **처리했으면 `true`** — 그러면 클릭이 게임 명령으로 가지 않는다.
    ///
    /// 버튼은 **맨 위 화면에서만** 찾는다. 겹친 창(일시정지·설정)이 떠 있으면 아래 화면의
    /// 버튼도, 게임 클릭도 받지 않는다 — 창이 모달로 동작한다.
    fn ui_pointer(&mut self, actions: &UiActions, pointer: Option<&PointerInput>) -> bool {
        self.ui_hover = None;
        let Some(shell) = self.shell.clone() else {
            return false;
        };
        let Some(p) = pointer else { return false };
        let Some(at) = p.screen else { return false };
        let Some(top) = shell.top() else { return false };

        let values = Values::of(self.play.as_ref(), self.show_items);
        let size = self.screen_size(actions.viewport_px);
        let hit = self.screens.button_at(top, &values, size, at);
        if p.over_viewport {
            self.ui_hover = hit.as_ref().map(|(id, _)| id.clone());
        }

        let modal = !shell.is_base();
        if p.pressed && p.over_viewport {
            self.ui_press = hit.as_ref().map(|(id, _)| id.clone());
            return hit.is_some() || modal;
        }
        if p.released {
            let pressed = self.ui_press.take();
            match (pressed, hit) {
                // 누른 버튼에서 뗐을 때만 동작한다 (누른 뒤 밖으로 끌면 취소).
                (Some(down), Some((up, action))) if down == up => {
                    self.run_action(action);
                    return true;
                }
                _ => return modal,
            }
        }
        modal
    }

    /// 플레이 중의 UI 요청. 편집 요청(언두·추가·삭제·인스펙터)은 받지 않는다 — 플레이는 씬을 바꾸지 않는다.
    fn apply_play_actions(&mut self, actions: &UiActions, pointer: Option<PointerInput>) {
        // 위젯 편집기가 열려 있으면 뷰포트 클릭은 위젯을 고르고 끄는 데 쓴다 (P7) —
        // 편집 중에 클릭이 게임 명령(이동·공격)으로 새면 위젯을 잡을 수 없다.
        let grabbed = self.screen_pointer(actions, pointer.as_ref())
            || self.ui_pointer(actions, pointer.as_ref());
        // Esc — 일시정지 화면을 열고 닫는다 (레벨이 정한 화면). 없으면 아무 일도 없다.
        if actions.deselect {
            self.toggle_pause();
        }
        let Some(session) = self.play.as_mut() else {
            return;
        };
        if let Some(action) = actions.inventory {
            session.inventory(action);
        }
        if !grabbed
            && let Some(p) = pointer
            && p.pressed
            && p.over_viewport
            && let Some(at) = p.world
        {
            session.click(at, p.px);
        }
        self.hover = None;
    }

    /// 일시정지 — 레벨이 정한 화면을 열고 닫는다. 겹친 창이 있으면 그것부터 닫는다.
    fn toggle_pause(&mut self) {
        let pause = self
            .shell
            .as_ref()
            .and_then(|s| self.levels.level(&s.level))
            .and_then(|l| l.pause.clone());
        let Some(shell) = self.shell.as_mut() else {
            return;
        };
        if shell.pop() {
            shell.paused = shell.screens.len() > 1;
            return;
        }
        if let Some(pause) = pause {
            shell.push(&pause);
            shell.paused = true;
        }
    }

    /// 화면(UI)이 기준으로 삼는 크기 — **씬을 그리는 사각형**(패널 사이 영역)이다.
    /// 그리기와 피킹이 같은 값을 써야 한다 — 창 전체 크기를 쓰면 클릭이 어긋난다.
    fn screen_size(&self, viewport: Option<[u32; 4]>) -> (f32, f32) {
        viewport.map_or(
            (self.camera.viewport.0 as f32, self.camera.viewport.1 as f32),
            |[_, _, w, h]| (w as f32, h as f32),
        )
    }

    /// 위젯 편집기에 뷰포트 포인터를 넘긴다. **처리했으면 `true`** — 그러면 클릭이
    /// 게임 명령이나 씬 편집으로 가지 않는다. 플레이 중이 아닐 때도 편집한다 (메인 화면 등).
    fn screen_pointer(&mut self, actions: &UiActions, pointer: Option<&PointerInput>) -> bool {
        if !self.screen_editor.is_open() {
            return false;
        }
        let Some(p) = pointer else { return false };
        let values = Values::of(self.play.as_ref(), self.show_items);
        let size = self.screen_size(actions.viewport_px);
        let grabbed = self
            .screen_editor
            .handle_pointer(&self.screens, &values, size, p);
        if let Some((error, message)) = self.screen_editor.take_status() {
            self.notify(message, error);
        }
        grabbed
    }

    /// 플레이 중 카메라 — 게임과 같은 고정 줌으로 플레이어를 따라간다.
    /// 두 tick 사이 보간된 위치를 쓰므로 20Hz 시뮬레이션에서도 화면은 매 프레임 부드럽다.
    fn follow_player(&mut self, alpha: f32) {
        let Some(at) = self.play.as_ref().and_then(|s| s.player_render_pos(alpha)) else {
            return;
        };
        self.camera.center = at;
        self.camera
            .set_pixels_per_meter(self.fixed_zoom.unwrap_or(Camera2d::PIXELS_PER_METER));
        self.camera.snap_to_pixel_grid();
    }

    /// UI 가 요청한 편집을 씬에 반영한다. 모든 씬 변경은 `Editing` 을 거친다.
    /// 팔레트 창에서 고른 그림 조각을 붓으로 삼는다 (P4).
    ///
    /// `data/terrain.ron` 에 같은 조각이 있으면 그 번호를, 없으면 **항목을 추가**하고 그 번호를 쓴다.
    /// 추가했으면 지형 그림을 다시 읽는다 (새 그림 파일만 GPU 에 올린다). 고르는 것은 편집이 아니라
    /// 언두에 남지 않는다 — 칠하는 것이 편집이다.
    fn apply_pick(&mut self, pick: &terrain::Pick) {
        let added = match terrain::add_to_disk(pick) {
            Ok(added) => added,
            Err(e) => {
                self.notify(format!("그림을 추가하지 못했습니다 — {e}"), true);
                return;
            }
        };
        if added.created
            && let Some(renderer) = self.renderer.as_mut()
            && let Err(e) = self.terrain.reload(renderer)
        {
            self.notify(format!("추가한 그림을 읽지 못했습니다 — {e}"), true);
            return;
        }
        self.editing.set_tool(Tool::PaintArt);
        self.editing.set_art_brush(added.id, pick.kind);
        let name = self
            .terrain
            .get(added.id)
            .map_or_else(String::new, |a| a.name.clone());
        let message = if added.created {
            format!(
                "그림 #{} \"{name}\" 을(를) terrain.ron 에 추가했습니다",
                added.id.raw()
            )
        } else {
            format!(
                "이미 있는 그림 #{} \"{name}\" 을(를) 붓으로",
                added.id.raw()
            )
        };
        self.notify(message, false);
    }

    fn apply_actions(&mut self, actions: &UiActions) {
        if let Some(path) = &actions.save_zone {
            self.save_zone(path.clone());
        }
        if let Some(path) = &actions.open_zone {
            self.open_zone(path.clone());
        }
        self.apply_content_actions(actions);
        if actions.toggle_play {
            self.toggle_play();
        }
        if actions.save_game
            && let Some(session) = self.play.take()
        {
            self.save_game(&session);
            self.play = Some(session);
        }
        if let Some(id) = actions.open_level.clone() {
            self.open_level(&id);
        }
        if actions.open_screen_editor {
            self.screen_editor.toggle(&self.screens);
        }
        if actions.toggle_items {
            self.show_items = !self.show_items;
        }
        if actions.toggle_panels {
            self.hide_panels = !self.hide_panels;
            self.notify(
                if self.hide_panels {
                    String::from("에디터 패널을 숨겼습니다 — F9 로 되돌립니다")
                } else {
                    String::from("에디터 패널을 다시 켰습니다")
                },
                false,
            );
        }
        if actions.delete_save {
            match save_file::delete(&save_file::default_path()) {
                Ok(()) => self.notify(
                    String::from("저장 데이터를 지웠습니다 — 다음 플레이는 새로 시작합니다"),
                    false,
                ),
                Err(e) => self.notify(format!("저장 데이터를 지우지 못했습니다 — {e}"), true),
            }
        }
        self.manual_shot_requested |= actions.screenshot;
        // 셸이 떠 있으면(플레이 중이거나 UI 레벨) 뷰포트는 게임 화면이다 — 씬 편집을 받지 않는다.
        if self.play.is_some() || self.shell.is_some() {
            let pointer = match actions.pointer {
                Some(p) if self.script.is_some() => Some(self.scripted_pointer(p, actions)),
                other => other,
            };
            self.apply_play_actions(actions, pointer);
            return;
        }

        if actions.reset_view {
            self.pending_view = Some(ViewRequest::Zone);
        }
        if actions.frame_selection {
            self.pending_view = Some(ViewRequest::Selection);
        }

        if actions.undo {
            self.editing.undo(&mut self.scene);
        }
        if actions.redo {
            self.editing.redo(&mut self.scene);
        }
        if let Some(pitch) = actions.set_pitch {
            self.camera.pitch = pitch;
        }
        if let Some(tool) = actions.set_tool {
            self.editing.set_tool(tool);
        }
        if let Some(brush) = actions.set_brush {
            self.editing.set_brush(brush);
        }
        if let Some((id, kind)) = actions.set_art_brush {
            self.editing.set_art_brush(id, kind);
        }
        if let Some(pick) = &actions.pick_art {
            self.apply_pick(pick);
        }
        if let Some((shape, radius)) = actions.set_brush_shape {
            self.editing.set_brush_shape(shape, radius);
        }
        if let Some(on) = actions.set_eyedropper {
            self.editing.set_eyedropper(on);
        }
        if actions.deselect && !self.editing.is_dragging() && !self.editing.is_painting() {
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
        if actions.delete && !self.editing.is_dragging() && !self.editing.is_painting() {
            self.editing.delete_selected(&mut self.scene);
        }

        let pointer = match actions.pointer {
            Some(p) if self.script.is_some() => Some(self.scripted_pointer(p, actions)),
            other => other,
        };
        // 위젯 편집기가 먼저 포인터를 본다 — 플레이 중이 아니어도 화면을 편집하기 때문이다.
        let pointer = if self.screen_pointer(actions, pointer.as_ref()) {
            None
        } else {
            pointer
        };
        // 도구에 따라 포인터가 하는 일이 다르다. 칠하는 중에는 선택이 끼어들지 않는다.
        self.hover = match (self.editing.tool(), pointer) {
            (Tool::PaintTile, Some(p)) => {
                self.editing.paint_pointer(&mut self.scene, &p);
                None
            }
            (Tool::PaintArt, Some(p)) => {
                self.editing.paint_art_pointer(&mut self.scene, &p);
                None
            }
            (Tool::Select, Some(p)) => self.editing.handle_pointer(&mut self.scene, &p),
            (_, None) => None,
        };

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
            // 화면 좌표도 같이 채운다 — HUD(화면 공간) 피킹이 스크립트에서도 돌게 (P6).
            screen: self.script_cursor.map(|w| self.camera.world_to_screen(w)),
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
            Step::SetTool(tool) => {
                self.editing.set_tool(tool);
            }
            Step::SetArtBrush(id, explicit) => {
                // 번호가 어느 층인지는 지형 데이터가 안다. 지우개(0)는 스크립트가 직접 고른다.
                let kind = explicit
                    .or_else(|| self.terrain.get(id).map(|art| art.kind))
                    .unwrap_or(terrain::ArtKind::Ground);
                self.editing.set_art_brush(id, kind);
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
            Step::TogglePlay => {
                self.toggle_play();
            }
            Step::Save(path) => {
                self.save_zone(path);
            }
            Step::Open(path) => {
                self.open_zone(path);
            }
            Step::Dialog(save) => {
                if let Some(ui) = self.ui.as_mut() {
                    ui.show_file_dialog(save, self.zone_path.as_deref());
                }
            }
            Step::Palette(image) => {
                if let Some(ui) = self.ui.as_mut() {
                    ui.show_palette(image);
                }
            }
            // 팔레트 창에서 고른 것과 같은 경로를 탄다 — 창의 마우스 조작만 빠진다.
            Step::Pick(pick) => self.apply_pick(&pick),
            Step::Scripts(path) => {
                if let Some(ui) = self.ui.as_mut() {
                    ui.show_scripts(path.as_deref());
                }
            }
            Step::Compile => {
                if let Some(ui) = self.ui.as_mut() {
                    ui.scripts_mut().compile();
                }
            }
            Step::SaveGame => {
                if let Some(session) = self.play.take() {
                    self.save_game(&session);
                    self.play = Some(session);
                }
            }
            Step::DeleteSave => {
                if let Err(e) = save_file::delete(&save_file::default_path()) {
                    self.notify(format!("저장 데이터를 지우지 못했습니다 — {e}"), true);
                }
            }
            Step::ToggleItems => self.show_items = !self.show_items,
            Step::TogglePanels => self.hide_panels = !self.hide_panels,
            Step::UiEdit => self.screen_editor.toggle(&self.screens),
            Step::UiScreen(id) => self.screen_editor.switch(&self.screens, &id),
            Step::UiPick(id) => {
                self.screen_editor.select(&id);
            }
            Step::UiDrag(dx, dy) => self.screen_editor.nudge((dx, dy)),
            // 화면 좌표로 누르기·떼기 — 실제 마우스와 같은 경로로 버튼을 눌러 본다.
            Step::ScreenPointer { button, at } => {
                p.screen = Some(at);
                p.world = None;
                p.over_viewport = true;
                p.pressed = button == Button::Press;
                p.released = button == Button::Release;
            }
            Step::OpenLevel(id) => self.open_level(&id),
            Step::Escape => self.toggle_pause(),
            Step::ContentToggle => {
                if let Some(ui) = self.ui.as_mut() {
                    ui.content_mut().toggle();
                }
            }
            Step::ContentPick(key) => {
                let found = self
                    .ui
                    .as_mut()
                    .is_some_and(|ui| ui.content_mut().select_key(&key));
                if !found {
                    self.notify(format!("콘텐츠 브라우저에 '{key}' 가 없습니다"), true);
                }
            }
            // 두 번 누르기와 같은 경로 — UI 창은 UI 가 열고, 나머지는 에디터가 맡는다.
            Step::ContentOpen => {
                let dirty = self.is_dirty();
                let acts = self.ui.as_mut().and_then(|ui| {
                    let action = ui.content_mut().open_selected()?;
                    Some(ui.open_content(action, dirty))
                });
                if let Some(acts) = acts {
                    if let Some(path) = &acts.open_zone {
                        self.open_zone(path.clone());
                    }
                    self.apply_content_actions(&acts);
                }
            }
            Step::StartLevel => {
                let id = self.levels.startup().to_owned();
                self.open_level(&id);
            }
            Step::UiSave => {
                self.screen_editor.save();
            }
            Step::Brush(shape, radius) => self.editing.set_brush_shape(shape, radius),
            Step::Eyedropper => self.editing.set_eyedropper(true),
            Step::Wait => {}
        }
        p
    }

    /// 이번 프레임에 그릴 씬 명령을 쌓는다. `alpha` 는 두 시뮬레이션 tick 사이의 보간 계수.
    fn build_commands(&mut self, viewport: Option<[u32; 4]>, alpha: f32) {
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

        // 화면 픽셀 → 월드 길이. 표시를 화면상 일정한 크기로 그리는 데 쓴다.
        let px = self.camera.view_height / self.camera.viewport.1.max(1) as f32;

        if self.play.is_some() {
            self.build_play_commands(alpha, px, viewport);
            return;
        }
        // 존이 없는 레벨(메인 화면 등) — 게임 화면만 그린다. 에디터 표시를 섞지 않는다.
        if self.shell.is_some() {
            self.build_screen_commands(viewport);
            return;
        }

        // ── 지면 층 ──────────────────────────────────────────────────────────
        // 여기 있는 것은 오브젝트를 절대 가리지 않는다.
        self.commands
            .push(RenderCommand::SetLayer(DrawLayer::Ground));
        grid::build(&self.camera, &mut self.commands);
        // 지형 그림이 먼저, 규칙 색이 그 위에 — 저작 중에는 걷기 막힘이 그림에 가려선 안 된다.
        terrain::build_ground(
            &self.scene.art,
            &self.scene.tiles,
            &self.terrain,
            &self.camera,
            BIAS_ART,
            &mut self.commands,
        );
        tiles::build(
            &self.scene.tiles,
            &self.camera,
            BIAS_TILE,
            None,
            &mut self.commands,
        );
        self.draw_zone_bounds();

        for item in &self.scene.items {
            // 발판 — 스폰 지점과 클릭 범위를 보여 준다. 스프라이트는 이 위에 선다.
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
            draw_arrow(item, px, &mut self.commands);
        }

        // ── 오브젝트 층 ──────────────────────────────────────────────────────
        // 같은 텍스처를 연달아 제출하므로 드로우 콜 하나로 묶인다.
        self.commands
            .push(RenderCommand::SetLayer(DrawLayer::Object));
        terrain::build_props(
            &self.scene.props,
            &self.scene.tiles,
            &self.terrain,
            &self.camera,
            BIAS_PROP,
            &mut self.commands,
        );
        if let Some(sprites) = &self.sprites {
            for item in &self.scene.items {
                // 그림은 마커 종류가 아니라 **액터 타입**이 정한다 (P1).
                let (actor, tint) = self.actor_look_of(item);
                sprites.build(item, actor, tint, px, BIAS_SPRITE, &mut self.commands);
            }
        }

        // ── 표시 층 ──────────────────────────────────────────────────────────
        // 스프라이트에 가리면 안 되는 것들.
        self.commands
            .push(RenderCommand::SetLayer(DrawLayer::Overlay));
        self.draw_zone_handles(px);
        for item in &self.scene.items {
            let target = Target::Item(item.entity);
            let (color, bias, pad, thick) = if self.editing.is_selected(target) {
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
                bias,
                color,
                &mut self.commands,
            );
        }

        self.draw_rotate_handle(px);
        self.draw_box_selection();
        self.draw_brush_preview();

        // 위젯 편집기가 열려 있으면 편집 중인 화면을 씬 위에 얹어 보여 준다 (P7) —
        // 메인 화면·설정 화면은 플레이 중이 아닐 때 편집하기 때문이다.
        self.build_screen_commands(viewport);
    }

    /// 사각형 붓으로 끄는 중인 범위 — 뗄 때 이 칸들이 한 번에 칠해진다.
    fn draw_brush_preview(&mut self) {
        let Some((a, b)) = self.editing.brush_preview() else {
            return;
        };
        let tiles = &self.scene.tiles;
        let (min_a, max_a) = tiles.tile_bounds(a);
        let (min_b, max_b) = tiles.tile_bounds(b);
        grid::build_outline(
            &self.camera,
            min_a.min(min_b),
            max_a.max(max_b),
            THIN_LINE_PX,
            BIAS_BOX + DEPTH_LAYER,
            SELECT_COLOR,
            &mut self.commands,
        );
    }

    /// 플레이 화면 — 편집 표시(그리드·마커·핸들) 없이 게임에 보일 것만.
    /// 타일 레벨 색과 존 경계는 남긴다 — 아트가 생기기 전에는 지형을 알아볼 수단이 이것뿐이다.
    fn build_play_commands(&mut self, alpha: f32, px: f32, viewport: Option<[u32; 4]>) {
        let Some(play) = self.play.as_ref() else {
            return;
        };
        self.commands
            .push(RenderCommand::SetLayer(DrawLayer::Ground));
        terrain::build_ground(
            &self.scene.art,
            &self.scene.tiles,
            &self.terrain,
            &self.camera,
            BIAS_ART,
            &mut self.commands,
        );
        // 그림이 있는 칸은 규칙 색을 덮지 않는다 — 플레이 중에는 게임 화면이어야 한다.
        tiles::build(
            &self.scene.tiles,
            &self.camera,
            BIAS_TILE,
            Some(&self.scene.art),
            &mut self.commands,
        );
        let zone = self.scene.zone;
        grid::build_outline(
            &self.camera,
            zone.min,
            zone.max,
            2.0,
            BIAS_ZONE,
            ZONE_BOUNDS_COLOR,
            &mut self.commands,
        );
        play.build_ground(&self.camera, alpha, px, &mut self.commands);

        self.commands
            .push(RenderCommand::SetLayer(DrawLayer::Object));
        terrain::build_props(
            &self.scene.props,
            &self.scene.tiles,
            &self.terrain,
            &self.camera,
            BIAS_PROP,
            &mut self.commands,
        );
        if let Some(sprites) = &self.sprites {
            play.build_objects(alpha, px, sprites, &mut self.commands);
        }

        self.commands
            .push(RenderCommand::SetLayer(DrawLayer::Overlay));
        // 시트가 없으면(로드 실패) 기본 칸 높이 48px 를 기준으로 막대를 띄운다.
        play.build_overlay(alpha, px, self.sprites.as_ref(), &mut self.commands);

        // 화면(UI)은 맨 마지막 — 화면 좌표라 카메라를 따라가지 않는다 (P5).
        self.build_screen_commands(viewport);
    }

    /// 화면(UI)을 그린다 — 플레이 중이면 HUD, 위젯 편집기가 열려 있으면 편집 중인 화면.
    ///
    /// 기준 크기는 **씬을 그리는 사각형**이다 (에디터에서는 패널 사이 영역).
    fn build_screen_commands(&mut self, viewport: Option<[u32; 4]>) {
        // 편집 중인 위젯 배치를 먼저 반영한다 — 고치는 즉시 화면에 보이게 (P7).
        self.screen_editor.apply(&mut self.screens);
        if !self.screens.ready() {
            return;
        }
        let editing = self.screen_editor.is_open();
        let mut ids: Vec<String> = Vec::new();
        match self.shell.as_ref() {
            // 레벨이 정한 화면 스택 — 바탕(HUD·메뉴) 위에 겹친 창들.
            Some(shell) => ids.extend(shell.screens.iter().cloned()),
            // 셸 없이 플레이하는 경우는 없지만, 있더라도 HUD 는 보여 준다.
            None if self.play.is_some() => ids.push(String::from(HUD_SCREEN)),
            None => {}
        }
        // 편집 중인 화면은 플레이 중이 아니어도 보여 준다 (메인 화면·설정 화면).
        if let Some(id) = self.screen_editor.preview_screen()
            && !ids.iter().any(|x| x == id)
        {
            ids.push(id.to_owned());
        }
        if ids.is_empty() {
            return;
        }
        let values = Values::of(self.play.as_ref(), self.show_items || editing);
        let size = self.screen_size(viewport);
        let state = screen::DrawState {
            hover: self.ui_hover.as_deref(),
            pressed: self.ui_press.as_deref(),
            selected: self.screen_editor.selected(),
            editing,
        };
        self.screens
            .build_commands(&ids, &values, state, size, &mut self.commands);
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
            uv: UvRect::FULL,
            texture: TextureId::WHITE,
        });
        self.commands.push(RenderCommand::DrawRect {
            center: at,
            size: Vec2::splat((scene::HANDLE_SIZE_PX - 3.0) * px),
            rotation: diamond,
            z: 0.0,
            depth_bias: BIAS_HANDLE + DEPTH_LAYER,
            color: if hot { SELECT_COLOR } else { HANDLE_FILL },
            uv: UvRect::FULL,
            texture: TextureId::WHITE,
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
            uv: UvRect::FULL,
            texture: TextureId::WHITE,
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

    /// 존 경계선 — 지면 층.
    fn draw_zone_bounds(&mut self) {
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
    }

    /// 존 크기 조절 핸들 — 표시 층. 스프라이트에 가리면 잡을 수 없다.
    fn draw_zone_handles(&mut self, px: f32) {
        let zone = self.scene.zone;
        if !self.editing.is_selected(Target::Zone) {
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
                uv: UvRect::FULL,
                texture: TextureId::WHITE,
            });
            self.commands.push(RenderCommand::DrawRect {
                rotation: 0.0,
                center: at,
                size: Vec2::splat(8.0 * px),
                z: 0.0,
                depth_bias: BIAS_HANDLE + DEPTH_LAYER,
                color: fill,
                uv: UvRect::FULL,
                texture: TextureId::WHITE,
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
        // 선택(NEXUS_SELECT)은 이름으로 찾으므로 존을 먼저 읽는다.
        if let Some(path) = std::env::var_os(ENV_ZONE) {
            self.open_zone(PathBuf::from(path));
        }
        self.apply_env_camera();
        self.apply_env_selection();
        self.script = Script::from_env();

        // 게임 데이터 — 액터 타입 목록(인스펙터)과 타입별 시트 경로가 여기서 나온다.
        // 그림은 여기서 GPU 에 한 번 올린다 — 시트를 갈아끼우면 다시 시작해야 반영된다
        // (텍스처 해제 API 가 없어 매번 올리면 쌓인다).
        let mut warnings = Vec::new();
        let sheets = match GameData::load() {
            Ok(data) => {
                let sheets = data.sprite_sheets();
                self.data = Some(data);
                sheets
            }
            Err(e) => {
                warnings.push(format!("게임 데이터를 읽지 못해 기본값을 씁니다 — {e}"));
                Vec::new()
            }
        };
        // 실패해도 에디터는 돌아간다 — 마커가 지면 사각형만 남을 뿐이다.
        match SpriteLibrary::load(&mut renderer, &sheets, &mut warnings) {
            Ok(s) => self.sprites = Some(s),
            Err(e) => warnings.push(format!("스프라이트 시트 로드 실패: {e}")),
        }
        // 지형 그림(타일 아트·건물). 실패하면 그림 없이 돈다 — 규칙 색만 보인다.
        match Terrain::load(&mut renderer) {
            Ok(t) => self.terrain = t,
            Err(e) => warnings.push(format!("지형 그림을 읽지 못했습니다 — {e}")),
        }
        // UI 테마·화면 파일. 실패하면 화면 없이 돈다 (에디터 패널은 그대로 보인다).
        let (screens, ui_warnings) = Screens::load(&mut renderer);
        self.screens = screens;
        warnings.extend(ui_warnings);
        // 레벨 목록·프로젝트 설정 (P7). 없어도 F5 로 지금 씬을 플레이할 수 있다.
        let (levels, level_warnings) = Levels::load();
        self.levels = levels;
        warnings.extend(level_warnings);
        for warning in warnings {
            self.notify(warning, true);
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
        // 플레이 중이면 시뮬레이션이 여기서 돈다 — 게임 규칙은 20Hz 고정 timestep 에서만 진행한다.
        // 에디터 조작(카메라·선택·편집)은 뷰·저작 작업이므로 UI 프레임에서 처리한다.
        // 일시정지 화면이 떠 있으면 시뮬레이션을 멈춘다 (화면·애니메이션은 계속 돈다).
        let paused = self.shell.as_ref().is_some_and(|s| s.paused);
        if let Some(play) = self.play.as_mut().filter(|_| !paused) {
            play.tick(dt, self.sprites.as_ref());
            // 스크립트 오류는 콘솔만이 아니라 상태 바에도 — 창만 보는 사람도 알 수 있게.
            for alert in play.take_alerts() {
                self.notify(alert, true);
            }
        }
    }

    fn render(&mut self, alpha: f32) {
        self.frame_index += 1;
        self.update_fps();

        let (mut ui_frame, viewport) = match self.run_ui() {
            Some((frame, actions)) => {
                self.apply_actions(&actions);
                (Some(frame), actions.viewport_px)
            }
            None => (None, None),
        };
        // 뷰포트가 확정된 뒤여야 배율이 맞는다.
        if viewport.is_some() {
            self.follow_player(alpha);
        }

        // 화면 문구에 새 글자(편집기에서 타이핑한 한글 등)가 생겼으면 글자 아틀라스를
        // 다시 굽는다 — 프레임 바깥에서 텍스처를 올려야 하므로 그리기 전에 한다.
        let font_warning = match self.renderer.as_mut() {
            Some(renderer) => self.screens.ensure_text_glyphs(renderer),
            None => None,
        };
        if let Some(e) = font_warning {
            self.notify(e, true);
        }

        self.build_commands(viewport, alpha);
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
        // 창을 닫는 것도 정상 종료다 — 플레이 중이면 진행 상황을 저장한다 (P3-4).
        if let Some(session) = self.play.take() {
            self.save_game(&session);
        }
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
pub(crate) fn push_segment(
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
        uv: UvRect::FULL,
        texture: TextureId::WHITE,
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
