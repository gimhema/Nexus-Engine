//! 에디터 UI — egui 통합과 화면 배치.
//!
//! ```text
//! ┌────────────────────── 메뉴 바 ──────────────────────┐
//! │  씬  │                 뷰포트                │ 인스펙터 │
//! ├────────────────────── 상태 바 ──────────────────────┤
//! ```
//!
//! UI 는 씬을 **읽기만** 한다. 사용자가 한 일은 [`UiActions`] 로 돌려주고, 실제 변경은
//! `edit` 모듈이 한다. 그래서 편집 규칙(언두 단위·스냅·선택)이 UI 코드에 흩어지지 않는다.
//!
//! 카메라만 예외로 여기서 직접 움직인다 — 카메라는 편집 대상이 아니라 보는 방식이다.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use nexus_core::{Camera2d, Vec2, units};
use nexus_platform::{WindowEvent, WindowTarget};
use nexus_render_wgpu::{UiFrame, egui};
use nexus_sim::{AiKind, BagKind, Tile, UnitDef};

use crate::edit::{BrushShape, InspectorEdit, MAX_BRUSH_RADIUS, PointerInput, Tool};
use crate::grid;
use crate::palette::Palette;
use crate::play::{InventoryAction, PlaySession};
use crate::scene::{
    ActorId, ArtId, Item, ItemKind, OverrideEdit, Overrides, Pick, Scene, Target, ZONE_LABEL,
};
use crate::screen::Screens;
use crate::screen_editor::ScreenEditor;
use crate::script_editor::ScriptEditor;
use crate::terrain::ArtKind;
use crate::zone_file;

/// 좌우 패널 기본 폭 (논리 포인트).
const SIDE_PANEL_WIDTH: f32 = 220.0;

/// 한글 표시에 쓸 시스템 폰트 후보. 앞에서부터 처음 찾은 것을 쓴다.
///
/// `cfg(target_os)` 분기 없이 경로 목록만 시도한다. 없는 경로는 그냥 건너뛴다.
/// `NEXUS_UI_FONT` 환경변수로 직접 지정할 수도 있다.
pub(crate) const KOREAN_FONT_CANDIDATES: &[&str] = &[
    "C:/Windows/Fonts/malgun.ttf",
    "/usr/share/fonts/truetype/nanum/NanumGothic.ttf",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
    // Fedora (google-noto-sans-cjk-fonts 패키지)
    "/usr/share/fonts/google-noto-sans-cjk-fonts/NotoSansCJK-Regular.ttc",
    // Fedora / Arch 의 나눔 패키지
    "/usr/share/fonts/nanum/NanumGothic.ttf",
    "/System/Library/Fonts/AppleSDGothicNeo.ttc",
];

/// UI 가 읽는 프레임 통계.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FrameStats {
    pub(crate) fps: f32,
    pub(crate) quads: usize,
    pub(crate) ticks: u64,
}

/// 상태 바에 띄우는 알림 (저장·열기·플레이의 결과와 실패 이유).
#[derive(Clone, Debug)]
pub(crate) struct Notice {
    pub(crate) message: String,
    pub(crate) error: bool,
}

/// UI 가 그리는 데 필요한 편집기 상태 (읽기 전용 + 카메라).
pub(crate) struct UiModel<'a> {
    pub(crate) camera: &'a mut Camera2d,
    pub(crate) scene: &'a Scene,
    pub(crate) selection: &'a [Target],
    pub(crate) hover: Option<Pick>,
    pub(crate) undo_label: Option<String>,
    pub(crate) redo_label: Option<String>,
    pub(crate) tool: Tool,
    pub(crate) brush: Tile,
    /// 고를 수 있는 액터 타입 `(번호, 이름, 수치)`. 데이터를 못 읽었으면 빈 목록.
    pub(crate) actor_catalog: Vec<(ActorId, &'a str, UnitDef)>,
    /// 마커 종류별 기본 액터 — [`ItemKind::ALL`] 순서.
    pub(crate) actor_defaults: [ActorId; 3],
    /// `(스크립트 경로, "액터 이름 (#번호)")` — 스크립트 편집기가 "쓰는 액터" 를 보여 준다.
    pub(crate) actor_scripts: Vec<(String, String)>,
    /// 지형 그림 붓 — 0 이면 지우개.
    pub(crate) art_brush: ArtId,
    /// 붓이 칠하는 층.
    pub(crate) art_layer: ArtKind,
    /// 붓 모양·반지름·스포이드 (타일·그림 공용).
    pub(crate) brush_shape: BrushShape,
    pub(crate) brush_radius: u8,
    pub(crate) eyedropper: bool,
    /// 고를 수 있는 지형 그림 `(번호, 이름, 종류)`. 그림 데이터가 없으면 빈 목록이다.
    pub(crate) art_palette: Vec<(ArtId, &'a str, ArtKind)>,
    pub(crate) stats: FrameStats,
    /// 플레이 중이면 `Some` — 패널이 편집 대신 게임 상태를 보여 준다.
    pub(crate) play: Option<&'a PlaySession>,
    /// 에디터 패널을 숨기고 있다 (F9) — HUD 만 남는다 (P5).
    pub(crate) hide_panels: bool,
    /// 레벨 목록 `(번호, 이름)` — 레벨 메뉴 (P7).
    pub(crate) levels: Vec<(String, String)>,
    /// 시작 레벨 번호 (data/project.ron).
    pub(crate) startup_level: String,
    /// 지금 열린 레벨 번호 — 플레이 중이 아니면 `None`.
    pub(crate) level: Option<String>,
    /// 화면(UI) — 위젯 편집기가 화면 목록·테마를 읽는다 (P7).
    pub(crate) screens: &'a Screens,
    /// 위젯 편집기 상태 — 창이 직접 고친다 (편집 규칙이 그쪽에 모여 있다).
    pub(crate) screen_editor: &'a mut ScreenEditor,
    /// 열어 둔 존 파일. 새 씬이면 `None`.
    pub(crate) zone_path: Option<&'a Path>,
    /// 마지막 저장 이후 바뀌었다.
    pub(crate) dirty: bool,
    pub(crate) notice: Option<&'a Notice>,
}

impl UiModel<'_> {
    /// 이 마커 종류가 "기본값" 일 때 실제로 쓰이는 액터 타입.
    fn default_actor(&self, kind: ItemKind) -> ActorId {
        self.actor_defaults[kind.index()]
    }
}

/// 한 프레임 동안 UI 가 편집기에 요청한 것.
#[derive(Debug, Default)]
pub(crate) struct UiActions {
    /// 이 경로의 존을 연다.
    pub(crate) open_zone: Option<PathBuf>,
    /// 이 경로로 저장한다.
    pub(crate) save_zone: Option<PathBuf>,
    /// 열기/저장 창을 띄운다 (UI 내부에서 소비).
    file_dialog: Option<FileDialogMode>,
    /// 플레이 시작 / 정지 (F5).
    pub(crate) toggle_play: bool,
    /// 진행 상황 저장 (플레이 메뉴).
    pub(crate) save_game: bool,
    /// 저장 데이터 삭제 — 다음 플레이는 새로 시작한다.
    pub(crate) delete_save: bool,
    /// HUD 인벤토리 창 열고 닫기 (I).
    pub(crate) toggle_items: bool,
    /// 에디터 패널 숨기기 (F9) — HUD 만으로 플레이되는지 보는 용도.
    pub(crate) toggle_panels: bool,
    /// 위젯 편집기 창 열고 닫기 (P7).
    pub(crate) open_screen_editor: bool,
    /// 이 레벨을 연다 (P7) — 언리얼의 Open Level.
    pub(crate) open_level: Option<String>,
    /// 플레이 중 인벤토리 패널 조작.
    pub(crate) inventory: Option<InventoryAction>,
    pub(crate) reset_view: bool,
    /// 선택 항목(없으면 전체 마커)이 화면에 들어오도록 카메라를 맞춘다.
    pub(crate) frame_selection: bool,
    pub(crate) screenshot: bool,
    pub(crate) undo: bool,
    pub(crate) redo: bool,
    pub(crate) deselect: bool,
    /// 씬 목록에서 클릭 — (대상, Shift 여부).
    pub(crate) list_select: Option<(Target, bool)>,
    pub(crate) inspector: Option<InspectorEdit>,
    /// 이 종류의 마커를 화면 한가운데에 추가.
    pub(crate) add_item: Option<ItemKind>,
    /// 선택된 마커 삭제.
    pub(crate) delete: bool,
    /// 카메라 pitch 변경 (라디안).
    pub(crate) set_pitch: Option<f32>,
    /// 뷰포트 포인터가 할 일 변경.
    pub(crate) set_tool: Option<Tool>,
    /// 타일 붓 변경.
    pub(crate) set_brush: Option<Tile>,
    /// 지형 그림 붓 변경 — `(번호, 층)`. 지우개(`ArtId::NONE`)도 층을 골라야 한다.
    pub(crate) set_art_brush: Option<(ArtId, ArtKind)>,
    /// 팔레트 창 열기. 안쪽 값이 있으면 그 그림을 골라 둔 채로 연다 (UI 내부에서 소비).
    open_palette: Option<Option<String>>,
    /// 스크립트 편집기 열기. 안쪽 값이 있으면 그 파일을 연다 (UI 내부에서 소비).
    open_scripts: Option<Option<String>>,
    /// 팔레트 창에서 고른 그림 조각 — 편집기가 `terrain.ron` 에 추가하고 붓으로 삼는다.
    pub(crate) pick_art: Option<crate::terrain::Pick>,
    /// 붓 모양·반지름 바꾸기.
    pub(crate) set_brush_shape: Option<(BrushShape, u8)>,
    /// 스포이드 켜기/끄기.
    pub(crate) set_eyedropper: Option<bool>,
    /// 뷰포트 포인터 — 뷰포트가 그려진 프레임에만 있다.
    pub(crate) pointer: Option<PointerInput>,
    /// 씬을 그릴 사각형 `[x, y, w, h]` (물리 픽셀).
    pub(crate) viewport_px: Option<[u32; 4]>,
}

/// egui 컨텍스트와 winit 입력 상태.
pub(crate) struct EditorUi {
    ctx: egui::Context,
    state: egui_winit::State,
    /// 커서 아래 월드 좌표 — 상태 바 표시용.
    cursor_world: Option<Vec2>,
    font_note: String,
    /// 열려 있는 존 열기/저장 창.
    file_dialog: Option<FileDialog>,
    /// 지형 그림 팔레트 창 (P4). 닫혀 있어도 상태(고른 그림·확대율)는 남긴다.
    palette: Palette,
    /// 스크립트 편집기 창 (P2). 닫혀 있어도 고치던 내용은 남긴다.
    scripts: ScriptEditor,
}

/// 존 열기 / 다른 이름으로 저장 창.
///
/// OS 파일 대화 상자(`rfd` 등)를 쓰지 않는다 — 리눅스에서 GTK·포털 같은 시스템 패키지를
/// 끌어들여 "빌드에 시스템 패키지가 필요 없다" 는 규칙을 깬다. 대신 `zones/` 폴더 목록과
/// 경로 입력칸만 둔다. 두 OS 에서 똑같이 동작한다.
#[derive(Debug)]
struct FileDialog {
    mode: FileDialogMode,
    /// 입력칸 내용.
    path: String,
    /// 창을 열 때 읽은 `zones/` 목록.
    files: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FileDialogMode {
    Open,
    SaveAs,
}

impl FileDialog {
    fn new(mode: FileDialogMode, current: Option<&Path>) -> Self {
        let path = current.map_or_else(|| zone_file::default_path("untitled"), Path::to_path_buf);
        Self {
            mode,
            path: path.display().to_string(),
            files: zone_file::list(Path::new(zone_file::ZONE_DIR)),
        }
    }
}

impl std::fmt::Debug for EditorUi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditorUi")
            .field("cursor_world", &self.cursor_world)
            .finish_non_exhaustive()
    }
}

impl EditorUi {
    pub(crate) fn new(target: &WindowTarget, max_texture_side: usize) -> Self {
        let ctx = egui::Context::default();
        let font_note = install_korean_font(&ctx);

        let state = egui_winit::State::new(
            ctx.clone(),
            egui::ViewportId::ROOT,
            target,
            Some(target.scale_factor() as f32),
            None,
            Some(max_texture_side),
        );

        Self {
            ctx,
            state,
            cursor_world: None,
            font_note,
            file_dialog: None,
            palette: Palette::default(),
            scripts: ScriptEditor::default(),
        }
    }

    /// 팔레트 창을 연다 — 자동 검증(`NEXUS_SCRIPT`)용. `image` 가 있으면 그 그림을 골라 둔다.
    pub(crate) fn show_palette(&mut self, image: Option<String>) {
        self.palette.open(image);
    }

    /// 스크립트 편집기를 연다 — 메뉴와 자동 검증(`NEXUS_SCRIPT`)이 쓴다.
    pub(crate) fn show_scripts(&mut self, path: Option<&str>) {
        self.scripts.open(path);
    }

    /// 스크립트 편집기 — 자동 검증의 `compile` 단계가 쓴다.
    pub(crate) fn scripts_mut(&mut self) -> &mut ScriptEditor {
        &mut self.scripts
    }

    /// 저장하지 않은 스크립트가 있으면 저장한다 — 플레이는 디스크의 파일로 돌기 때문이다.
    /// 저장할 것이 없으면 `None`.
    pub(crate) fn save_dirty_script(&mut self) -> Option<Result<String, String>> {
        self.scripts.is_dirty().then(|| self.scripts.save())
    }

    /// 열기(`save = false`) / 다른 이름으로 저장 창을 띄운다 — 자동 검증(`NEXUS_SCRIPT`)용.
    pub(crate) fn show_file_dialog(&mut self, save: bool, current: Option<&Path>) {
        let mode = if save {
            FileDialogMode::SaveAs
        } else {
            FileDialogMode::Open
        };
        self.file_dialog = Some(FileDialog::new(mode, current));
    }

    /// 어떤 폰트를 쓰게 됐는지 (시작 로그용).
    pub(crate) fn font_note(&self) -> &str {
        &self.font_note
    }

    /// 창 이벤트를 egui 에 넘긴다. egui 가 가로챘으면 `true`.
    pub(crate) fn on_window_event(&mut self, target: &WindowTarget, event: &WindowEvent) -> bool {
        self.state.on_window_event(target.window(), event).consumed
    }

    /// 한 프레임 UI 를 실행하고, 렌더러에 넘길 출력과 요청 사항을 돌려준다.
    pub(crate) fn run(
        &mut self,
        target: &WindowTarget,
        model: &mut UiModel<'_>,
    ) -> (UiFrame, UiActions) {
        let raw = self.state.take_egui_input(target.window());
        let mut actions = UiActions::default();
        let cursor_world = &mut self.cursor_world;
        let dialog = &mut self.file_dialog;
        let palette = &mut self.palette;
        let scripts = &mut self.scripts;

        let output = self.ctx.run_ui(raw, |ui| {
            menu_bar(ui, model, &mut actions);
            shortcuts(ui, model, &mut actions);
            if let Some(mode) = actions.file_dialog.take() {
                *dialog = Some(FileDialog::new(mode, model.zone_path));
            }
            file_dialog(ui, dialog, model.dirty, &mut actions);
            palette.show(ui, &mut actions);
            // 위젯 편집기 — 창이 직접 model.screen_editor 를 고친다 (액션을 거치지 않는다).
            {
                let screens = model.screens;
                model.screen_editor.show(ui, screens);
            }
            let users = |path: &str| {
                model
                    .actor_scripts
                    .iter()
                    .filter(|(p, _)| p == path)
                    .map(|(_, who)| who.clone())
                    .collect()
            };
            actions.toggle_play |= scripts.show(ui, &users).toggle_play;
            // F9 로 패널을 숨기면 뷰포트만 남는다 — HUD 만으로 플레이되는지 확인하는 모드 (P5).
            if !model.hide_panels {
                status_bar(ui, model, *cursor_world);
                outline_panel(ui, model, &mut actions);
                inspector_panel(ui, model, &mut actions);
            }
            viewport(ui, model, cursor_world, &mut actions);
            // 인스펙터의 "팔레트 열기" 는 창을 그린 뒤에 눌린다 — 다음 프레임부터 뜬다.
            if let Some(image) = actions.open_palette.take() {
                palette.open(image);
            }
            if let Some(path) = actions.open_scripts.take() {
                scripts.open(path.as_deref());
            }
        });

        self.state
            .handle_platform_output(target.window(), output.platform_output);

        let primitives = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        let frame = UiFrame {
            primitives,
            textures_delta: output.textures_delta,
            pixels_per_point: output.pixels_per_point,
        };
        (frame, actions)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 패널
// ─────────────────────────────────────────────────────────────────────────────

fn menu_bar(ui: &mut egui::Ui, model: &UiModel<'_>, actions: &mut UiActions) {
    egui::Panel::top("menu_bar").show(ui, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("파일", |ui| {
                // 플레이 중에는 씬을 바꾸지 않는다 — 열기는 막고, 저장은 편집 상태를 저장하므로 둔다.
                let editing = model.play.is_none();
                if ui
                    .add_enabled(
                        editing,
                        egui::Button::new("존 열기…").shortcut_text("Ctrl+O"),
                    )
                    .clicked()
                {
                    actions.file_dialog = Some(FileDialogMode::Open);
                }
                if ui
                    .add(egui::Button::new("존 저장").shortcut_text("Ctrl+S"))
                    .clicked()
                {
                    request_save(model, actions);
                }
                if ui.button("다른 이름으로 저장…").clicked() {
                    actions.file_dialog = Some(FileDialogMode::SaveAs);
                }
                ui.separator();
                if ui
                    .add(egui::Button::new("스크린샷 저장").shortcut_text("F12"))
                    .clicked()
                {
                    actions.screenshot = true;
                }
            });
            ui.menu_button("편집", |ui| {
                let undo_text = model
                    .undo_label
                    .as_ref()
                    .map_or_else(|| String::from("실행 취소"), |l| format!("실행 취소: {l}"));
                if ui
                    .add_enabled(
                        model.undo_label.is_some(),
                        egui::Button::new(undo_text).shortcut_text("Ctrl+Z"),
                    )
                    .clicked()
                {
                    actions.undo = true;
                }

                let redo_text = model
                    .redo_label
                    .as_ref()
                    .map_or_else(|| String::from("다시 실행"), |l| format!("다시 실행: {l}"));
                if ui
                    .add_enabled(
                        model.redo_label.is_some(),
                        egui::Button::new(redo_text).shortcut_text("Ctrl+Y"),
                    )
                    .clicked()
                {
                    actions.redo = true;
                }

                ui.separator();
                if ui
                    .add_enabled(
                        !model.selection.is_empty(),
                        egui::Button::new("선택 해제").shortcut_text("Esc"),
                    )
                    .clicked()
                {
                    actions.deselect = true;
                }

                ui.separator();
                ui.menu_button("추가", |ui| {
                    for kind in [ItemKind::PlayerSpawn, ItemKind::Npc, ItemKind::Monster] {
                        if ui.button(kind.label()).clicked() {
                            actions.add_item = Some(kind);
                        }
                    }
                });
                let has_items = model.selection.iter().any(|t| matches!(t, Target::Item(_)));
                if ui
                    .add_enabled(has_items, egui::Button::new("삭제").shortcut_text("Delete"))
                    .clicked()
                {
                    actions.delete = true;
                }
            });
            ui.menu_button("스크립트", |ui| {
                if ui.button("스크립트 편집기…").clicked() {
                    actions.open_scripts = Some(None);
                }
                ui.weak("data/scripts/*.rhai — 액터 행동. 고치고 컴파일·저장한 뒤 F5.");
            });
            ui.menu_button("레벨", |ui| {
                ui.weak("레벨을 열면 그 존 파일을 열고 플레이를 시작한다.");
                for (id, name) in &model.levels {
                    let here = model.level.as_deref() == Some(id.as_str());
                    let start = if *id == model.startup_level {
                        "  ★"
                    } else {
                        ""
                    };
                    if ui
                        .selectable_label(here, format!("{name}  ({id}){start}"))
                        .clicked()
                    {
                        actions.open_level = Some(id.clone());
                    }
                }
                if model.levels.is_empty() {
                    ui.weak("레벨 파일이 없습니다 (levels/*.level.ron).");
                }
                ui.separator();
                if ui
                    .add(egui::Button::new("시작 레벨부터").shortcut_text("Shift+F5"))
                    .clicked()
                    && !model.startup_level.is_empty()
                {
                    actions.open_level = Some(model.startup_level.clone());
                }
                ui.weak("★ = 시작 레벨 (data/project.ron).");
            });
            ui.menu_button("UI", |ui| {
                if ui
                    .add(egui::Button::new("위젯 편집기…").shortcut_text("F7"))
                    .clicked()
                {
                    actions.open_screen_editor = true;
                }
                ui.weak("화면(ui/*.ui.ron)의 위젯을 화면에서 끌어 배치한다.");
                ui.weak("플레이 중이 아니어도 편집한다 — 메인 화면·설정 화면도 여기서.");
            });
            ui.menu_button("플레이", |ui| {
                let text = if model.play.is_some() {
                    "정지 — 편집으로 돌아가기"
                } else {
                    "플레이 시작"
                };
                if ui
                    .add(egui::Button::new(text).shortcut_text("F5"))
                    .clicked()
                {
                    actions.toggle_play = true;
                }
                ui.separator();
                // 저장은 진행 상황(레벨·소지품)이다 — 씬(존)은 파일 메뉴에서 저장한다.
                if ui
                    .add_enabled(model.play.is_some(), egui::Button::new("게임 저장"))
                    .clicked()
                {
                    actions.save_game = true;
                }
                if ui.button("저장 데이터 삭제").clicked() {
                    actions.delete_save = true;
                }
                ui.separator();
                if ui
                    .add(egui::Button::new("인벤토리 창").shortcut_text("I"))
                    .clicked()
                {
                    actions.toggle_items = true;
                }
                if ui
                    .add(egui::Button::new("에디터 패널 숨기기").shortcut_text("F9"))
                    .clicked()
                {
                    actions.toggle_panels = true;
                }
                ui.weak("정지하거나 창을 닫을 때도 저장된다 (수동 저장 + 정상 종료).");
                ui.weak("씬은 바뀌지 않는다 — 정지하면 플레이 결과는 버려진다.");
            });
            ui.menu_button("보기", |ui| {
                if ui
                    .add(egui::Button::new("존 전체 보기").shortcut_text("Home"))
                    .clicked()
                {
                    actions.reset_view = true;
                }
                if ui
                    .add(egui::Button::new("선택 항목 보기").shortcut_text("F"))
                    .clicked()
                {
                    actions.frame_selection = true;
                }
                ui.separator();

                // 쿼터뷰는 게임 화면과 같은 시점(WYSIWYG), 탑다운은 좌표를 정밀하게
                // 찍을 때 쓴다. 기울어져 있으면 세로 방향 거리감이 눌려서 배치가 어렵다.
                let topdown = model.camera.pitch >= Camera2d::PITCH_TOPDOWN - 1e-3;
                if ui.radio(!topdown, "쿼터뷰 (게임 시점)").clicked() {
                    actions.set_pitch = Some(Camera2d::PITCH_QUARTER);
                }
                if ui.radio(topdown, "탑다운 (정밀 편집)").clicked() {
                    actions.set_pitch = Some(Camera2d::PITCH_TOPDOWN);
                }
            });
        });
    });
}

/// 저장 — 열어 둔 파일이 있으면 거기에, 없으면 "다른 이름으로 저장" 창.
fn request_save(model: &UiModel<'_>, actions: &mut UiActions) {
    match model.zone_path {
        Some(path) => actions.save_zone = Some(path.to_path_buf()),
        None => actions.file_dialog = Some(FileDialogMode::SaveAs),
    }
}

/// 존 열기 / 다른 이름으로 저장 창.
fn file_dialog(
    ui: &mut egui::Ui,
    dialog: &mut Option<FileDialog>,
    dirty: bool,
    actions: &mut UiActions,
) {
    let Some(d) = dialog.as_mut() else {
        return;
    };
    let (title, confirm) = match d.mode {
        FileDialogMode::Open => ("존 열기", "열기"),
        FileDialogMode::SaveAs => ("다른 이름으로 저장", "저장"),
    };

    let mut close = false;
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ui.ctx(), |ui| {
            ui.weak(format!("{}/ 폴더의 존 파일", zone_file::ZONE_DIR));
            egui::ScrollArea::vertical()
                .max_height(160.0)
                .show(ui, |ui| {
                    if d.files.is_empty() {
                        ui.weak("(없음)");
                    }
                    for file in &d.files {
                        let shown = file.display().to_string();
                        if ui.selectable_label(d.path == shown, &shown).clicked() {
                            d.path = shown;
                        }
                    }
                });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("경로");
                ui.add(egui::TextEdit::singleline(&mut d.path).desired_width(260.0));
            });
            if d.mode == FileDialogMode::SaveAs {
                ui.weak("파일 이름은 소문자로 저장된다 — 리눅스는 대소문자를 가린다.");
            }
            if d.mode == FileDialogMode::Open && dirty {
                ui.colored_label(
                    egui::Color32::from_rgb(240, 190, 90),
                    "저장하지 않은 변경이 있다 — 열면 사라진다.",
                );
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button(confirm).clicked() {
                    match d.mode {
                        FileDialogMode::Open => {
                            actions.open_zone = Some(PathBuf::from(d.path.trim()));
                        }
                        FileDialogMode::SaveAs => {
                            actions.save_zone = Some(zone_file::save_path_from_input(&d.path));
                        }
                    }
                    close = true;
                }
                if ui.button("취소").clicked() {
                    close = true;
                }
            });
        });
    if close {
        *dialog = None;
    }
}

/// 전역 단축키. 텍스트 입력 중에는 받지 않는다 (입력칸 안의 Ctrl+Z 는 글자 되돌리기다).
fn shortcuts(ui: &mut egui::Ui, model: &UiModel<'_>, actions: &mut UiActions) {
    let ctx = ui.ctx().clone();
    if ctx.egui_wants_keyboard_input() {
        return;
    }

    use egui::{Key, KeyboardShortcut, Modifiers};
    let ctrl_shift = Modifiers::COMMAND | Modifiers::SHIFT;

    let (save, open) = ctx.input_mut(|i| {
        (
            i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::S)),
            i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::O)),
        )
    });
    if save {
        request_save(model, actions);
    }
    if open && model.play.is_none() {
        actions.file_dialog = Some(FileDialogMode::Open);
    }

    ctx.input_mut(|i| {
        // Shift 는 "상관없음"으로 판정되므로 Ctrl+Shift+Z 를 Ctrl+Z 보다 먼저 소비해야 한다.
        actions.redo |= i.consume_shortcut(&KeyboardShortcut::new(ctrl_shift, Key::Z));
        actions.redo |= i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Y));
        actions.undo |= i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Z));

        actions.toggle_play |= i.key_pressed(Key::F5);
        actions.toggle_items |= i.key_pressed(Key::I);
        actions.toggle_panels |= i.key_pressed(Key::F9);
        actions.open_screen_editor |= i.key_pressed(Key::F7);
        // Shift+F5 = 시작 레벨부터 (F5 는 지금 씬으로 플레이).
        if i.key_pressed(Key::F5) && i.modifiers.shift && !model.startup_level.is_empty() {
            actions.toggle_play = false;
            actions.open_level = Some(model.startup_level.clone());
        }
        actions.reset_view |= i.key_pressed(Key::Home);
        actions.frame_selection |= i.key_pressed(Key::F);
        actions.screenshot |= i.key_pressed(Key::F12);
        actions.deselect |= i.key_pressed(Key::Escape);
        actions.delete |= i.key_pressed(Key::Delete);
    });
}

/// 커서 아래 타일을 사람이 읽을 문자열로. 저작 중 데이터를 바로 확인하는 수단이다.
fn tile_label(model: &UiModel<'_>, cursor: Option<Vec2>) -> Option<String> {
    let at = model.scene.tiles.world_to_tile(cursor?);
    let tile = model.scene.tiles.get(at)?;
    let mut s = format!("타일 ({}, {}) · L{}", at.x, at.y, tile.level);
    if !tile.walkable {
        s.push_str(" · 막힘");
    }
    if tile.ramp {
        s.push_str(" · 경사로");
    }
    Some(s)
}

fn status_bar(ui: &mut egui::Ui, model: &UiModel<'_>, cursor: Option<Vec2>) {
    egui::Panel::bottom("status_bar").show(ui, |ui| {
        ui.horizontal(|ui| {
            // 파일 이름 + 저장하지 않은 변경 표시(*).
            let name = model.zone_path.and_then(Path::file_name).map_or_else(
                || String::from("(새 존)"),
                |n| n.to_string_lossy().into_owned(),
            );
            let mark = if model.dirty { " *" } else { "" };
            ui.label(format!("{name}{mark}"))
                .on_hover_text(model.zone_path.map_or_else(
                    || String::from("아직 저장하지 않았다 — Ctrl+S"),
                    |p| p.display().to_string(),
                ));
            ui.separator();
            if let Some(notice) = model.notice {
                // 긴 오류는 잘라서 보이고, 전체는 마우스를 올리면 본다.
                let text = egui::RichText::new(&notice.message).small();
                let text = if notice.error {
                    text.color(egui::Color32::from_rgb(240, 110, 100))
                } else {
                    text.weak()
                };
                ui.add(egui::Label::new(text).truncate())
                    .on_hover_text(&notice.message);
                ui.separator();
            }
            match cursor {
                Some(p) => ui.monospace(format!("X {:>9.2}  Y {:>9.2} m", p.x, p.y)),
                None => ui.monospace("X         —  Y         — m"),
            };
            ui.separator();
            ui.monospace(format!(
                "시야 {} · 그리드 {} ",
                format_length(model.camera.view_height),
                format_length(grid::pick_spacing(model.camera.view_height)),
            ));

            if let Some(play) = model.play {
                ui.colored_label(
                    egui::Color32::from_rgb(120, 220, 140),
                    format!("▶ 플레이 중 · sim tick {}", play.ticks()),
                );
                ui.separator();
            }
            if let Some(label) = tile_label(model, cursor) {
                ui.separator();
                ui.monospace(label);
            }
            if let Some(label) = model.hover.and_then(|p| model.scene.label(p.target())) {
                ui.separator();
                ui.label(format!("▸ {label}"));
            }
            if !model.selection.is_empty() {
                ui.separator();
                ui.label(format!("선택 {}", model.selection.len()));
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.monospace(format!(
                    "{:.0} fps · 쿼드 {} · tick {}",
                    model.stats.fps, model.stats.quads, model.stats.ticks
                ));
            });
        });
    });
}

fn outline_panel(ui: &mut egui::Ui, model: &UiModel<'_>, actions: &mut UiActions) {
    egui::Panel::left("outline")
        .resizable(true)
        .default_size(SIDE_PANEL_WIDTH)
        .show(ui, |ui| {
            if let Some(play) = model.play {
                play_units(ui, play);
                return;
            }
            ui.heading("씬");
            ui.horizontal_wrapped(|ui| {
                ui.weak("추가");
                for (kind, text) in [
                    (ItemKind::PlayerSpawn, "플레이어"),
                    (ItemKind::Npc, "NPC"),
                    (ItemKind::Monster, "몬스터"),
                ] {
                    let button = egui::Button::new(format!("+ {text}"))
                        .fill(to_color32(kind.color()).gamma_multiply(0.25));
                    if ui
                        .add(button)
                        .on_hover_text("보고 있는 곳 한가운데에 놓습니다")
                        .clicked()
                    {
                        actions.add_item = Some(kind);
                    }
                }
            });
            ui.separator();

            let mut row = |ui: &mut egui::Ui, target: Target, label: &str, color: [f32; 4]| {
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 2.0, to_color32(color));

                    let selected = model.selection.contains(&target);
                    let response = ui.selectable_label(selected, label);
                    if response.clicked() {
                        let shift = ui.input(|i| i.modifiers.shift);
                        actions.list_select = Some((target, shift));
                    }
                });
            };

            egui::ScrollArea::vertical().show(ui, |ui| {
                row(ui, Target::Zone, ZONE_LABEL, crate::ZONE_BOUNDS_COLOR);
                for item in &model.scene.items {
                    row(ui, Target::Item(item.entity), &item.name, item.kind.color());
                }
            });
        });
}

/// 인스펙터 — 최소 구성.
///
/// 지금은 **위치·경계만** 편집한다. 스탯·진영·AI 같은 나머지 필드는 사용자의
/// 패널 명세를 받은 뒤 채운다 (CLAUDE.md 「UI 명세 전달 형식」).
/// 도구 선택과 타일 붓. 인스펙터 맨 위에 둔다.
fn tool_section(ui: &mut egui::Ui, model: &UiModel<'_>, actions: &mut UiActions) {
    ui.horizontal(|ui| {
        for (tool, label) in [
            (Tool::Select, "선택"),
            (Tool::PaintTile, "타일 칠하기"),
            (Tool::PaintArt, "그림 칠하기"),
        ] {
            if ui.selectable_label(model.tool == tool, label).clicked() {
                actions.set_tool = Some(tool);
            }
        }
    });

    if model.tool == Tool::PaintArt {
        art_section(ui, model, actions);
        return;
    }
    if model.tool != Tool::PaintTile {
        return;
    }

    ui.separator();
    brush_controls(ui, model, actions);
    ui.separator();
    ui.label("붓");

    let mut brush = model.brush;
    let mut changed = false;
    changed |= ui.checkbox(&mut brush.walkable, "걸을 수 있음").changed();
    changed |= ui.checkbox(&mut brush.ramp, "경사로").changed();
    ui.horizontal(|ui| {
        ui.label("높이 레벨");
        let mut level = i32::from(brush.level);
        if ui
            .add(egui::DragValue::new(&mut level).range(0..=9).speed(0.1))
            .changed()
        {
            brush.level = u8::try_from(level.clamp(0, 9)).unwrap_or(0);
            changed = true;
        }
    });
    if changed {
        actions.set_brush = Some(brush);
    }

    ui.label(
        egui::RichText::new(
            "높이는 그리지 않는다 — 이동(경사로로만 오르내림)과 시야 규칙에만 쓰인다.",
        )
        .small()
        .weak(),
    );
}

/// 붓 모양·크기·스포이드 — 타일 칠하기와 그림 칠하기가 같이 쓴다 (P4-4).
fn brush_controls(ui: &mut egui::Ui, model: &UiModel<'_>, actions: &mut UiActions) {
    ui.horizontal(|ui| {
        for (shape, label, tip) in [
            (BrushShape::Stroke, "붓", "끄는 동안 지나간 칸을 칠한다"),
            (
                BrushShape::Rect,
                "사각형",
                "누른 칸과 뗀 칸을 모서리로 하는 사각형을 한 번에",
            ),
        ] {
            if ui
                .selectable_label(model.brush_shape == shape, label)
                .on_hover_text(tip)
                .clicked()
            {
                actions.set_brush_shape = Some((shape, model.brush_radius));
            }
        }
        let eyedropper = ui
            .selectable_label(model.eyedropper, "스포이드")
            .on_hover_text("다음에 누른 칸의 값을 붓으로 집는다 (한 번 쓰면 꺼진다)");
        if eyedropper.clicked() {
            actions.set_eyedropper = Some(!model.eyedropper);
        }
    });
    if model.brush_shape == BrushShape::Stroke {
        ui.horizontal(|ui| {
            ui.label("크기");
            let mut radius = model.brush_radius;
            let side = |r: u8| u32::from(r) * 2 + 1;
            egui::ComboBox::from_id_salt("brush_radius")
                .selected_text(format!("{0}×{0}", side(radius)))
                .show_ui(ui, |ui| {
                    for r in 0..=MAX_BRUSH_RADIUS {
                        ui.selectable_value(&mut radius, r, format!("{0}×{0}", side(r)));
                    }
                });
            if radius != model.brush_radius {
                actions.set_brush_shape = Some((BrushShape::Stroke, radius));
            }
        });
    }
}

/// 지형 그림 붓 — `data/terrain.ron` 의 목록에서 고른다.
fn art_section(ui: &mut egui::Ui, model: &UiModel<'_>, actions: &mut UiActions) {
    ui.separator();

    // 그림을 눈으로 보고 고르는 창 — 고른 조각은 terrain.ron 에 자동으로 추가된다 (P4).
    if ui
        .button("팔레트 열기…")
        .on_hover_text("그림 파일에서 칸을 골라 붓으로 쓴다")
        .clicked()
    {
        actions.open_palette = Some(None);
    }
    brush_controls(ui, model, actions);
    ui.separator();

    if model.art_palette.is_empty() {
        ui.label(
            egui::RichText::new("지형 그림이 없습니다 — data/terrain.ron 을 확인하세요.")
                .small()
                .weak(),
        );
        return;
    }

    // 지우개도 층을 고른다 — 건물만 지우고 지면은 남기는 것이 보통이다.
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("지우기").small().weak());
        for (kind, label) in [(ArtKind::Ground, "지면"), (ArtKind::Prop, "오브젝트")] {
            let active = model.art_brush.is_none() && model.art_layer == kind;
            if ui.selectable_label(active, label).clicked() {
                actions.set_art_brush = Some((ArtId::NONE, kind));
            }
        }
    });

    for (kind, title) in [(ArtKind::Ground, "지면"), (ArtKind::Prop, "오브젝트")] {
        let items: Vec<_> = model
            .art_palette
            .iter()
            .filter(|(_, _, k)| *k == kind)
            .collect();
        if items.is_empty() {
            continue;
        }
        ui.label(egui::RichText::new(title).small().weak());
        for &&(id, name, art_kind) in &items {
            if ui
                .selectable_label(model.art_brush == id, name)
                .on_hover_text(format!("번호 {}", id.raw()))
                .clicked()
            {
                actions.set_art_brush = Some((id, art_kind));
            }
        }
    }

    ui.label(
        egui::RichText::new(
            "그림은 걷기 규칙과 별개다 — 건물을 놓아도 막히려면 타일을 칠해야 한다.",
        )
        .small()
        .weak(),
    );
}

/// 플레이 중 왼쪽 패널 — 유닛과 HP.
fn play_units(ui: &mut egui::Ui, play: &PlaySession) {
    ui.heading("유닛");
    ui.separator();
    let world = play.world();
    for (unit, u) in world.units() {
        ui.horizontal(|ui| {
            let name = play.name(unit);
            if u.is_alive() {
                ui.label(name);
            } else {
                ui.weak(format!("{name} (쓰러짐)"));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.monospace(format!("{}/{}", u.hp(), u.max_hp()));
            });
        });
        let ratio = u.hp() as f32 / u.max_hp() as f32;
        ui.add(egui::ProgressBar::new(ratio).desired_height(4.0));
    }
    let items = world.ground_items().count();
    if items > 0 {
        ui.add_space(8.0);
        ui.weak(format!("땅에 떨어진 아이템 {items}개"));
    }
}

/// 플레이 중 오른쪽 패널 — 플레이어 상태·인벤토리·기록.
fn play_inspector(ui: &mut egui::Ui, play: &PlaySession, actions: &mut UiActions) {
    ui.heading("플레이어");
    ui.separator();
    let Some(me) = play.world().unit(play.player()) else {
        return;
    };
    ui.monospace(format!(
        "HP {}/{}  공격 {}  방어 {}",
        me.hp(),
        me.max_hp(),
        me.attack(),
        me.defense()
    ));
    let p = me.progress();
    ui.monospace(format!(
        "레벨 {}  경험치 {}/{}",
        p.level(),
        p.exp(),
        p.exp_to_next()
    ));

    ui.add_space(6.0);
    ui.strong("장착");
    for (slot, place, item) in play.equipped_lines() {
        ui.horizontal(|ui| {
            ui.label(format!("{place}:"));
            match item {
                Some(name) => {
                    if ui.small_button(name).on_hover_text("클릭: 해제").clicked() {
                        actions.inventory = Some(InventoryAction::Unequip(slot));
                    }
                }
                None => {
                    ui.weak("—");
                }
            }
        });
    }

    ui.add_space(6.0);
    ui.strong("소모품");
    let consumables = play.bag_lines(BagKind::Consumable);
    if consumables.is_empty() {
        ui.weak("비어 있음");
    }
    for line in consumables {
        let text = format!("{} ×{}", line.name, line.count);
        if ui.button(text).on_hover_text("클릭: 사용").clicked() {
            actions.inventory = Some(InventoryAction::Use(line.slot));
        }
    }

    ui.add_space(6.0);
    ui.strong("장비 가방");
    let gear = play.bag_lines(BagKind::Equipment);
    if gear.is_empty() {
        ui.weak("비어 있음");
    }
    for line in gear {
        if ui.button(line.name).on_hover_text("클릭: 장착").clicked() {
            actions.inventory = Some(InventoryAction::Equip(line.slot));
        }
    }

    ui.add_space(10.0);
    ui.strong("기록");
    for line in play.log() {
        ui.label(egui::RichText::new(line).small());
    }
}

fn inspector_panel(ui: &mut egui::Ui, model: &UiModel<'_>, actions: &mut UiActions) {
    egui::Panel::right("inspector")
        .resizable(true)
        .default_size(SIDE_PANEL_WIDTH)
        .show(ui, |ui| {
            if let Some(play) = model.play {
                play_inspector(ui, play, actions);
                return;
            }
            ui.heading("인스펙터");
            ui.separator();
            tool_section(ui, model, actions);
            ui.separator();

            // 드래그 속도: 화면 1px 당 월드 이동량 — 줌 레벨과 무관하게 손맛이 같다.
            let speed = model.camera.view_height / model.camera.viewport.1.max(1) as f32;

            match model.selection {
                [] => {
                    ui.weak("선택된 오브젝트 없음");
                    ui.add_space(8.0);
                    ui.weak(
                        "왼쪽 클릭: 선택\nShift+클릭: 추가/해제\n빈 곳 드래그: 박스 선택\n\
                         Ctrl+드래그: 그리드 스냅\nDelete: 삭제\n\
                         F: 선택 항목 보기 · Home: 존 전체",
                    );
                }
                [Target::Zone] => zone_inspector(ui, model, speed, actions),
                [Target::Item(entity)] => {
                    if let Some(item) = model.scene.item(*entity) {
                        ui.strong(&item.name);
                        ui.weak(item.kind.label());
                        ui.add_space(6.0);

                        let mut pos = item.pos;
                        let (changed, finished) =
                            vec2_grid(ui, "item_pos", |row| row.edit("위치", &mut pos, speed));
                        if changed || finished {
                            actions.inspector = Some(InspectorEdit::ItemPos {
                                entity: *entity,
                                pos,
                                finished,
                            });
                        }

                        // 방향: 저장은 라디안, 편집·표시는 도 (nexus_core::units 규약).
                        // 0~360 을 넘겨 끌어도 되고, 저장할 때 정규화된다.
                        let mut degrees = units::normalize_heading(item.orientation).to_degrees();
                        ui.horizontal(|ui| {
                            ui.label("방향");
                            let r = ui.add(
                                egui::DragValue::new(&mut degrees)
                                    .speed(1.0)
                                    .fixed_decimals(1)
                                    .suffix("°"),
                            );
                            let finished = r.drag_stopped() || r.lost_focus();
                            if r.changed() || finished {
                                actions.inspector = Some(InspectorEdit::ItemHeading {
                                    entity: *entity,
                                    heading: degrees.to_radians(),
                                    finished,
                                });
                            }
                            ui.weak(format!("{:.3} rad", item.orientation));
                        });
                        ui.weak("뷰포트: 화살표 끝 ◆ 를 끌어 회전 (Ctrl 15°)");

                        ui.add_space(10.0);
                        actor_section(ui, model, &[item], actions);
                    }
                }
                many => {
                    ui.label(format!("{}개 선택됨", many.len()));
                    ui.weak("뷰포트에서 끌면 함께 이동합니다.\nDelete: 모두 삭제");

                    // 여러 마커의 타입·덮어쓰기를 한 번에 바꾼다. 값이 다르면 "—" 로 보여 준다
                    // (CLAUDE.md 「UI 명세 전달 형식」의 다중 선택 규칙).
                    let actors: Vec<_> = many
                        .iter()
                        .filter_map(|t| match t {
                            Target::Item(e) => model.scene.item(*e),
                            Target::Zone => None,
                        })
                        .collect();
                    if !actors.is_empty() {
                        ui.add_space(10.0);
                        actor_section(ui, model, &actors, actions);
                    }
                }
            }
        });
}

/// 덮어쓴 줄의 글자색 — 수치가 `rules.ron` 이 아니라 존 파일에 있다는 표시.
const OVERRIDE_COLOR: egui::Color32 = egui::Color32::from_rgb(255, 190, 90);

/// 액터 타입 드롭다운 (P1) + 수치 표와 마커별 덮어쓰기 (P1-4).
///
/// 표의 각 줄은 **타입 값**을 보여 준다. 오른쪽 칸을 켜면 이 마커에만 다른 값을 줄 수 있고,
/// 그 줄은 강조색이 된다 — 수치가 `rules.ron` 과 존 파일 두 곳에 있다는 것이 눈에 보이게.
/// 여러 개를 골랐으면 전부에 적용하고, 서로 다른 값은 "—" 로 보여 준다.
fn actor_section(ui: &mut egui::Ui, model: &UiModel<'_>, items: &[&Item], actions: &mut UiActions) {
    let Some(first) = items.first() else {
        return;
    };
    let kind = first.kind;
    let same_actor = items.iter().all(|i| i.actor == first.actor);
    let actor = if same_actor {
        first.actor
    } else {
        ActorId::DEFAULT
    };

    ui.strong("액터 타입");
    if model.actor_catalog.is_empty() {
        ui.weak("게임 데이터를 읽지 못했습니다 — data/rules.ron 을 확인하세요.");
        return;
    }

    let label = |id: ActorId| -> String {
        if id.is_default() {
            let base = model
                .actor_catalog
                .iter()
                .find(|(a, _, _)| *a == model.default_actor(kind))
                .map_or("?", |(_, name, _)| *name);
            format!("기본값 ({base})")
        } else {
            model
                .actor_catalog
                .iter()
                .find(|(a, _, _)| *a == id)
                .map_or_else(
                    || format!("없는 타입 #{}", id.raw()),
                    |(_, n, _)| (*n).to_string(),
                )
        }
    };

    egui::ComboBox::from_id_salt("actor_type")
        .selected_text(label(actor))
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(actor.is_default(), label(ActorId::DEFAULT))
                .clicked()
            {
                actions.inspector = Some(InspectorEdit::ItemActor {
                    actor: ActorId::DEFAULT,
                });
            }
            for &(id, name, _) in &model.actor_catalog {
                if ui
                    .selectable_label(actor == id, format!("{name} (#{})", id.raw()))
                    .clicked()
                {
                    actions.inspector = Some(InspectorEdit::ItemActor { actor: id });
                }
            }
        });
    if !same_actor {
        ui.weak("— (선택한 마커의 타입이 서로 다릅니다)");
    }

    // 마커마다 실제로 쓰일 타입의 수치 — 기본값이면 종류 기본 타입의 것.
    let base_of = |i: &Item| {
        let effective = if i.actor.is_default() {
            model.default_actor(i.kind)
        } else {
            i.actor
        };
        model
            .actor_catalog
            .iter()
            .find(|(a, _, _)| *a == effective)
            .map(|(_, _, def)| *def)
    };
    let bases: Vec<_> = items.iter().map(|i| base_of(i)).collect();
    let Some(Some(base)) = bases.first().copied() else {
        return;
    };
    let same_base = bases.iter().all(|b| *b == Some(base));
    // 타입 값이 마커마다 다르면 "—" 로 보여 준다.
    let shown = |v: String| if same_base { v } else { String::from("—") };

    ui.add_space(4.0);
    egui::Grid::new("actor_stats")
        .num_columns(3)
        .show(ui, |ui| {
            let mut row = OverrideRow { items, actions };
            row.number(
                ui,
                "HP",
                shown(base.max_hp.to_string()),
                base.max_hp,
                |o| o.max_hp,
                OverrideEdit::MaxHp,
                1,
            );
            row.number(
                ui,
                "공격",
                shown(base.attack.to_string()),
                base.attack,
                |o| o.attack,
                OverrideEdit::Attack,
                0,
            );
            row.number(
                ui,
                "방어",
                shown(base.defense.to_string()),
                base.defense,
                |o| o.defense,
                OverrideEdit::Defense,
                0,
            );

            // 이동 속도는 덮어쓰지 않는다 — 걷는 애니메이션·경로와 묶인 타입의 성질이다.
            ui.weak("이동");
            ui.label(shown(format!("{:.1} m/s", base.move_speed)));
            ui.end_row();

            row.number(
                ui,
                "진영",
                shown(base.faction.0.to_string()),
                base.faction.0,
                |o| o.faction,
                OverrideEdit::Faction,
                0,
            );
            row.ai(ui, shown(ai_label(base.ai).to_string()), base.ai);
            row.range(
                ui,
                "어그로",
                shown(format!("{:.1} m", base.aggro_range)),
                base.aggro_range,
                |o| o.aggro_range,
                OverrideEdit::AggroRange,
            );
            row.range(
                ui,
                "귀환 거리",
                shown(format!("{:.1} m", base.leash_range)),
                base.leash_range,
                |o| o.leash_range,
                OverrideEdit::LeashRange,
            );
            row.immortal(ui, shown(yes_no(base.immortal).to_string()), base.immortal);
        });

    let overridden = items.iter().filter(|i| !i.overrides.is_empty()).count();
    if overridden > 0 {
        ui.horizontal(|ui| {
            ui.colored_label(
                OVERRIDE_COLOR,
                format!(
                    "덮어쓴 항목 {}개",
                    items.iter().map(|i| i.overrides.count()).sum::<usize>()
                ),
            );
            if ui.small_button("타입 값으로 되돌리기").clicked() {
                actions.inspector = Some(InspectorEdit::ItemOverride {
                    edit: OverrideEdit::Reset,
                    finished: true,
                });
            }
        });
    }
    ui.weak("타입 값은 data/rules.ron 에서 고칩니다 (F5 로 적용).\n오른쪽 칸을 켜면 이 마커에만 다른 값을 줍니다.");
}

fn ai_label(ai: AiKind) -> &'static str {
    match ai {
        AiKind::Passive => "수동",
        AiKind::Defensive => "방어",
        AiKind::Aggressive => "공격",
    }
}

fn yes_no(b: bool) -> &'static str {
    if b { "예" } else { "아니오" }
}

/// 덮어쓰기 표의 한 줄 — `항목 | 값 | 덮어쓰기 칸`.
///
/// 고른 마커들의 덮어쓰기 상태는 셋 중 하나다: 모두 없음(타입 값 표시) · 모두 같은 값(편집 가능) ·
/// 섞임("—", 칸은 반쯤 체크). 칸을 켜면 **이미 덮어쓴 값이 있으면 그 값, 없으면 타입 값**에서 시작한다.
struct OverrideRow<'a, 'b> {
    items: &'a [&'a Item],
    actions: &'b mut UiActions,
}

impl OverrideRow<'_, '_> {
    /// 공통 뼈대. `editor` 는 값 칸의 위젯이고 `(changed, finished)` 를 돌려준다.
    #[allow(clippy::too_many_arguments)]
    fn show<T: Copy + PartialEq>(
        &mut self,
        ui: &mut egui::Ui,
        label: &str,
        base_text: String,
        base: T,
        get: impl Fn(&Overrides) -> Option<T>,
        make: impl Fn(Option<T>) -> OverrideEdit,
        editor: impl FnOnce(&mut egui::Ui, &mut T) -> (bool, bool),
    ) {
        let values: Vec<Option<T>> = self.items.iter().map(|i| get(&i.overrides)).collect();
        let first = values.first().copied().flatten();
        let all_on = values.iter().all(Option::is_some);
        let any_on = values.iter().any(Option::is_some);
        let uniform = all_on && values.iter().all(|v| *v == first);

        if any_on {
            ui.colored_label(OVERRIDE_COLOR, label);
        } else {
            ui.weak(label);
        }

        match first {
            Some(mut v) if uniform => {
                let (changed, finished) = editor(ui, &mut v);
                if changed || finished {
                    self.actions.inspector = Some(InspectorEdit::ItemOverride {
                        edit: make(Some(v)),
                        finished,
                    });
                }
            }
            _ if any_on => {
                ui.colored_label(OVERRIDE_COLOR, "—");
            }
            _ => {
                ui.label(base_text);
            }
        }

        let mut on = all_on;
        let r = ui
            .add(egui::Checkbox::new(&mut on, "").indeterminate(any_on && !all_on))
            .on_hover_text("이 마커에만 다른 값을 줍니다");
        if r.changed() {
            let edit = if on {
                make(Some(values.iter().find_map(|v| *v).unwrap_or(base)))
            } else {
                make(None)
            };
            self.actions.inspector = Some(InspectorEdit::ItemOverride {
                edit,
                finished: true,
            });
        }
        ui.end_row();
    }

    #[allow(clippy::too_many_arguments)]
    fn number(
        &mut self,
        ui: &mut egui::Ui,
        label: &str,
        base_text: String,
        base: u32,
        get: impl Fn(&Overrides) -> Option<u32>,
        make: impl Fn(Option<u32>) -> OverrideEdit,
        min: u32,
    ) {
        self.show(ui, label, base_text, base, get, make, |ui, v| {
            let r = ui.add(egui::DragValue::new(v).speed(1.0).range(min..=1_000_000));
            (r.changed(), r.drag_stopped() || r.lost_focus())
        });
    }

    fn range(
        &mut self,
        ui: &mut egui::Ui,
        label: &str,
        base_text: String,
        base: f32,
        get: impl Fn(&Overrides) -> Option<f32>,
        make: impl Fn(Option<f32>) -> OverrideEdit,
    ) {
        self.show(ui, label, base_text, base, get, make, |ui, v| {
            let r = ui.add(
                egui::DragValue::new(v)
                    .speed(0.1)
                    .range(0.0..=1000.0)
                    .fixed_decimals(1)
                    .suffix(" m"),
            );
            (r.changed(), r.drag_stopped() || r.lost_focus())
        });
    }

    fn ai(&mut self, ui: &mut egui::Ui, base_text: String, base: AiKind) {
        self.show(
            ui,
            "AI",
            base_text,
            base,
            |o| o.ai,
            OverrideEdit::Ai,
            |ui, v| {
                let mut picked = false;
                egui::ComboBox::from_id_salt("override_ai")
                    .selected_text(ai_label(*v))
                    .show_ui(ui, |ui| {
                        for kind in [AiKind::Passive, AiKind::Defensive, AiKind::Aggressive] {
                            if ui.selectable_label(*v == kind, ai_label(kind)).clicked() {
                                *v = kind;
                                picked = true;
                            }
                        }
                    });
                // 드롭다운은 고르는 순간 끝난다 — 한 번에 한 스텝.
                (picked, picked)
            },
        );
    }

    fn immortal(&mut self, ui: &mut egui::Ui, base_text: String, base: bool) {
        self.show(
            ui,
            "불사",
            base_text,
            base,
            |o| o.immortal,
            OverrideEdit::Immortal,
            |ui, v| {
                let r = ui.checkbox(v, yes_no(*v));
                (r.changed(), r.changed())
            },
        );
    }
}

fn zone_inspector(ui: &mut egui::Ui, model: &UiModel<'_>, speed: f32, actions: &mut UiActions) {
    let zone = model.scene.zone;
    ui.strong(ZONE_LABEL);
    ui.weak("서버 ZoneConfig.boundsMin / boundsMax (XY)");
    ui.add_space(6.0);

    let mut bounds = zone;
    let (changed, finished) = vec2_grid(ui, "zone_bounds", |row| {
        let (c1, f1) = row.edit("최소", &mut bounds.min, speed);
        let (c2, f2) = row.edit("최대", &mut bounds.max, speed);
        (c1 || c2, f1 || f2)
    });

    // 인스펙터에서도 뒤집힌 경계는 허용하지 않는다.
    let size = bounds.size();
    let valid = size.x >= crate::scene::MIN_ZONE_SIZE && size.y >= crate::scene::MIN_ZONE_SIZE;
    if (changed || finished) && valid {
        actions.inspector = Some(InspectorEdit::Zone { bounds, finished });
    }

    ui.add_space(6.0);
    let s = zone.size();
    ui.monospace(format!(
        "크기 {} × {}",
        format_length(s.x),
        format_length(s.y)
    ));
}

/// `라벨 | X | Y` 3열 표 안의 한 행 편집기.
struct Vec2Row<'u> {
    ui: &'u mut egui::Ui,
}

impl Vec2Row<'_> {
    /// X/Y 드래그 값 한 행. `(값이 바뀌었나, 편집이 끝났나)` 를 돌려준다.
    fn edit(&mut self, label: &str, v: &mut Vec2, speed: f32) -> (bool, bool) {
        let mut changed = false;
        let mut finished = false;
        self.ui.label(label);
        for value in [&mut v.x, &mut v.y] {
            let r = self
                .ui
                .add(egui::DragValue::new(value).speed(speed).fixed_decimals(2));
            changed |= r.changed();
            // 드래그를 놓았거나, 직접 입력 후 포커스를 잃었을 때 = 편집 한 번 끝
            finished |= r.drag_stopped() || r.lost_focus();
        }
        self.ui.end_row();
        (changed, finished)
    }
}

/// 좌표 입력 표. 단위(m)는 머리글에 한 번만 적어 폭을 아낀다 — 좁은 사이드바에서
/// 입력칸마다 단위를 붙이면 패널 밖으로 넘친다 (스크린샷으로 확인한 문제).
fn vec2_grid<R>(ui: &mut egui::Ui, id: &str, body: impl FnOnce(&mut Vec2Row<'_>) -> R) -> R {
    egui::Grid::new(id)
        .num_columns(3)
        .spacing([6.0, 4.0])
        .show(ui, |ui| {
            ui.label("");
            ui.weak("X (m)");
            ui.weak("Y (m)");
            ui.end_row();
            body(&mut Vec2Row { ui })
        })
        .inner
}

/// 남은 영역 전체를 씬 뷰포트로 쓴다.
///
/// - 왼쪽: 선택 / 끌어서 이동 (편집 규칙은 `edit` 모듈이 판단)
/// - 가운데·오른쪽 드래그: 팬
/// - 휠: 커서 기준 줌
fn viewport(
    ui: &mut egui::Ui,
    model: &mut UiModel<'_>,
    cursor_world: &mut Option<Vec2>,
    actions: &mut UiActions,
) {
    egui::CentralPanel::no_frame().show(ui, |ui| {
        let rect = ui.max_rect();
        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        let ppp = ui.ctx().pixels_per_point();
        let camera = &mut *model.camera;

        // 논리 포인트 → 물리 픽셀. 씬 렌더러와 카메라는 물리 픽셀 기준이다.
        let px = [
            (rect.min.x * ppp).round().max(0.0) as u32,
            (rect.min.y * ppp).round().max(0.0) as u32,
            (rect.width() * ppp).round().max(0.0) as u32,
            (rect.height() * ppp).round().max(0.0) as u32,
        ];
        actions.viewport_px = Some(px);
        camera.viewport = (px[2], px[3]);

        let to_world = |camera: &Camera2d, pos: egui::Pos2| {
            camera.screen_to_world(Vec2::new(
                (pos.x - rect.min.x) * ppp,
                (pos.y - rect.min.y) * ppp,
            ))
        };

        // ── 카메라 ──────────────────────────────────────────────────────
        if response.dragged_by(egui::PointerButton::Middle)
            || response.dragged_by(egui::PointerButton::Secondary)
        {
            let d = response.drag_delta() * ppp;
            camera.pan_by_pixels(Vec2::new(d.x, d.y));
        }

        if let Some(pos) = response.hover_pos() {
            let notches = ui.input(|i| {
                i.events
                    .iter()
                    .filter_map(|e| match e {
                        egui::Event::MouseWheel { unit, delta, .. } => Some(match unit {
                            egui::MouseWheelUnit::Line => delta.y,
                            egui::MouseWheelUnit::Page => delta.y * 10.0,
                            // 터치패드 픽셀 스크롤: 대략 50pt = 한 노치
                            egui::MouseWheelUnit::Point => delta.y / 50.0,
                        }),
                        _ => None,
                    })
                    .sum::<f32>()
            });
            if notches.abs() > f32::EPSILON {
                let local = Vec2::new((pos.x - rect.min.x) * ppp, (pos.y - rect.min.y) * ppp);
                camera.zoom_at(notches, local);
            }
        }

        *cursor_world = response.hover_pos().map(|p| to_world(camera, p));

        // ── 편집 포인터 ─────────────────────────────────────────────────
        let (pressed, released, latest, modifiers) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_released(),
                i.pointer.latest_pos(),
                i.modifiers,
            )
        });

        let world_per_px = camera.view_height / camera.viewport.1.max(1) as f32;
        actions.pointer = Some(PointerInput {
            // 드래그 중에는 뷰포트 밖으로 나가도 계속 따라가야 하므로 latest_pos 를 쓴다
            world: latest.map(|p| to_world(camera, p)),
            // HUD 편집기는 화면 좌표로 피킹한다 (P6) — 뷰포트 왼쪽 위가 원점.
            screen: latest.map(|p| Vec2::new((p.x - rect.min.x) * ppp, (p.y - rect.min.y) * ppp)),
            pressed: pressed && response.hovered(),
            over_viewport: response.hovered(),
            released,
            additive: modifiers.shift,
            snap: modifiers.command,
            px: world_per_px,
            grid: grid::pick_spacing(camera.view_height),
        });
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// 도우미
// ─────────────────────────────────────────────────────────────────────────────

/// 시스템 한글 폰트를 egui 폴백 폰트로 등록한다. 결과를 사람이 읽을 문장으로 반환한다.
fn install_korean_font(ctx: &egui::Context) -> String {
    let override_path = std::env::var_os("NEXUS_UI_FONT").map(PathBuf::from);
    let candidates = override_path
        .iter()
        .cloned()
        .chain(KOREAN_FONT_CANDIDATES.iter().map(PathBuf::from));

    for path in candidates {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };

        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "korean".to_owned(),
            Arc::new(egui::FontData::from_owned(bytes)),
        );
        // 기본 라틴 폰트 뒤에 폴백으로 둔다 — 영문·숫자 모양은 egui 기본을 유지.
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .push("korean".to_owned());
        }
        ctx.set_fonts(fonts);
        return format!("한글 폰트 — {}", path.display());
    }

    String::from("한글 폰트를 찾지 못함 — 한글이 네모로 표시됩니다 (NEXUS_UI_FONT 로 지정 가능)")
}

fn to_color32(c: [f32; 4]) -> egui::Color32 {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    egui::Color32::from_rgba_unmultiplied(byte(c[0]), byte(c[1]), byte(c[2]), byte(c[3]))
}

/// 미터 값을 읽기 쉬운 단위로. 정수면 소수점을 떼고, 아니면 한 자리까지.
fn format_length(meters: f32) -> String {
    let trim = |v: f32, unit: &str| {
        if (v - v.round()).abs() < 0.05 {
            format!("{v:.0} {unit}")
        } else {
            format!("{v:.1} {unit}")
        }
    };
    let abs = meters.abs();
    if abs >= units::KILOMETER {
        trim(meters / units::KILOMETER, "km")
    } else if abs >= 1.0 {
        trim(meters, "m")
    } else {
        trim(meters / units::CENTIMETER, "cm")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_formatting_picks_units() {
        // 입력은 미터
        assert_eq!(format_length(0.5), "50 cm");
        assert_eq!(format_length(2.0), "2 m");
        assert_eq!(format_length(12.5), "12.5 m");
        assert_eq!(format_length(2000.0), "2 km");
        assert_eq!(format_length(2600.0), "2.6 km");
    }

    #[test]
    fn color_conversion_clamps() {
        assert_eq!(
            to_color32([2.0, -1.0, 0.5, 1.0]),
            egui::Color32::from_rgba_unmultiplied(255, 0, 128, 255)
        );
    }
}
