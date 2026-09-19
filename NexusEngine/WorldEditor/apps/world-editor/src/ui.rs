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
use nexus_sim::{BagKind, Tile};

use crate::edit::{InspectorEdit, PointerInput, Tool};
use crate::grid;
use crate::play::{InventoryAction, PlaySession};
use crate::scene::{ItemKind, Pick, Scene, Target, ZONE_LABEL};
use crate::zone_file;

/// 좌우 패널 기본 폭 (논리 포인트).
const SIDE_PANEL_WIDTH: f32 = 220.0;

/// 한글 표시에 쓸 시스템 폰트 후보. 앞에서부터 처음 찾은 것을 쓴다.
///
/// `cfg(target_os)` 분기 없이 경로 목록만 시도한다. 없는 경로는 그냥 건너뛴다.
/// `NEXUS_UI_FONT` 환경변수로 직접 지정할 수도 있다.
const KOREAN_FONT_CANDIDATES: &[&str] = &[
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
    pub(crate) stats: FrameStats,
    /// 플레이 중이면 `Some` — 패널이 편집 대신 게임 상태를 보여 준다.
    pub(crate) play: Option<&'a PlaySession>,
    /// 열어 둔 존 파일. 새 씬이면 `None`.
    pub(crate) zone_path: Option<&'a Path>,
    /// 마지막 저장 이후 바뀌었다.
    pub(crate) dirty: bool,
    pub(crate) notice: Option<&'a Notice>,
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
        }
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

        let output = self.ctx.run_ui(raw, |ui| {
            menu_bar(ui, model, &mut actions);
            shortcuts(ui, model, &mut actions);
            if let Some(mode) = actions.file_dialog.take() {
                *dialog = Some(FileDialog::new(mode, model.zone_path));
            }
            file_dialog(ui, dialog, model.dirty, &mut actions);
            status_bar(ui, model, *cursor_world);
            outline_panel(ui, model, &mut actions);
            inspector_panel(ui, model, &mut actions);
            viewport(ui, model, cursor_world, &mut actions);
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
        for (tool, label) in [(Tool::Select, "선택"), (Tool::PaintTile, "타일 칠하기")] {
            if ui.selectable_label(model.tool == tool, label).clicked() {
                actions.set_tool = Some(tool);
            }
        }
    });

    if model.tool != Tool::PaintTile {
        return;
    }

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
                ui.monospace(format!("{}/{}", u.hp(), u.def().max_hp));
            });
        });
        let ratio = u.hp() as f32 / u.def().max_hp as f32;
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
        me.def().max_hp,
        me.attack(),
        me.defense()
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
                        ui.weak("스탯·진영·AI 필드는 패널 명세를 받은 뒤 추가됩니다.");
                    }
                }
                many => {
                    ui.label(format!("{}개 선택됨", many.len()));
                    ui.weak("뷰포트에서 끌면 함께 이동합니다.\nDelete: 모두 삭제");
                }
            }
        });
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
