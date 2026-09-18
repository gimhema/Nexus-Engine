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

use std::path::PathBuf;
use std::sync::Arc;

use nexus_core::{Camera2d, Vec2};
use nexus_platform::{WindowEvent, WindowTarget};
use nexus_render_wgpu::{UiFrame, egui};

use crate::edit::{InspectorEdit, PointerInput};
use crate::grid;
use crate::scene::{Pick, Scene, Target, ZONE_LABEL};

/// 좌우 패널 기본 폭 (논리 포인트).
const SIDE_PANEL_WIDTH: f32 = 220.0;

/// 피킹 허용 오차 (물리 픽셀). 작은 마커도 잡을 수 있게.
const PICK_TOLERANCE_PX: f32 = 6.0;

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
    "/System/Library/Fonts/AppleSDGothicNeo.ttc",
];

/// UI 가 읽는 프레임 통계.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FrameStats {
    pub(crate) fps: f32,
    pub(crate) quads: usize,
    pub(crate) ticks: u64,
}

/// UI 가 그리는 데 필요한 편집기 상태 (읽기 전용 + 카메라).
pub(crate) struct UiModel<'a> {
    pub(crate) camera: &'a mut Camera2d,
    pub(crate) scene: &'a Scene,
    pub(crate) selection: &'a [Target],
    pub(crate) hover: Option<Pick>,
    pub(crate) undo_label: Option<String>,
    pub(crate) redo_label: Option<String>,
    pub(crate) stats: FrameStats,
}

/// 한 프레임 동안 UI 가 편집기에 요청한 것.
#[derive(Debug, Default)]
pub(crate) struct UiActions {
    pub(crate) reset_view: bool,
    pub(crate) screenshot: bool,
    pub(crate) undo: bool,
    pub(crate) redo: bool,
    pub(crate) deselect: bool,
    /// 씬 목록에서 클릭 — (대상, Shift 여부).
    pub(crate) list_select: Option<(Target, bool)>,
    pub(crate) inspector: Option<InspectorEdit>,
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
        }
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

        let output = self.ctx.run_ui(raw, |ui| {
            menu_bar(ui, model, &mut actions);
            shortcuts(ui, &mut actions);
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
                ui.add_enabled(false, egui::Button::new("존 열기…"));
                ui.add_enabled(false, egui::Button::new("존 저장"));
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
            });
            ui.menu_button("보기", |ui| {
                if ui
                    .add(egui::Button::new("시점 초기화").shortcut_text("Home"))
                    .clicked()
                {
                    actions.reset_view = true;
                }
            });
        });
    });
}

/// 전역 단축키. 텍스트 입력 중에는 받지 않는다 (입력칸 안의 Ctrl+Z 는 글자 되돌리기다).
fn shortcuts(ui: &mut egui::Ui, actions: &mut UiActions) {
    let ctx = ui.ctx().clone();
    if ctx.egui_wants_keyboard_input() {
        return;
    }

    use egui::{Key, KeyboardShortcut, Modifiers};
    let ctrl_shift = Modifiers::COMMAND | Modifiers::SHIFT;

    ctx.input_mut(|i| {
        // Shift 는 "상관없음"으로 판정되므로 Ctrl+Shift+Z 를 Ctrl+Z 보다 먼저 소비해야 한다.
        actions.redo |= i.consume_shortcut(&KeyboardShortcut::new(ctrl_shift, Key::Z));
        actions.redo |= i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Y));
        actions.undo |= i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Z));

        actions.reset_view |= i.key_pressed(Key::Home);
        actions.screenshot |= i.key_pressed(Key::F12);
        actions.deselect |= i.key_pressed(Key::Escape);
    });
}

fn status_bar(ui: &mut egui::Ui, model: &UiModel<'_>, cursor: Option<Vec2>) {
    egui::Panel::bottom("status_bar").show(ui, |ui| {
        ui.horizontal(|ui| {
            match cursor {
                Some(p) => ui.monospace(format!("X {:>9.1}  Y {:>9.1} cm", p.x, p.y)),
                None => ui.monospace("X         —  Y         — cm"),
            };
            ui.separator();
            ui.monospace(format!(
                "시야 {:.1} m · 그리드 {} ",
                model.camera.view_height / 100.0,
                format_length(grid::pick_spacing(model.camera.view_height)),
            ));

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
            ui.heading("씬");
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
fn inspector_panel(ui: &mut egui::Ui, model: &UiModel<'_>, actions: &mut UiActions) {
    egui::Panel::right("inspector")
        .resizable(true)
        .default_size(SIDE_PANEL_WIDTH)
        .show(ui, |ui| {
            ui.heading("인스펙터");
            ui.separator();

            // 드래그 속도: 화면 1px 당 월드 이동량 — 줌 레벨과 무관하게 손맛이 같다.
            let speed = model.camera.view_height / model.camera.viewport.1.max(1) as f32;

            match model.selection {
                [] => {
                    ui.weak("선택된 오브젝트 없음");
                    ui.add_space(8.0);
                    ui.weak("왼쪽 클릭: 선택\nShift+클릭: 추가/해제\nCtrl+드래그: 그리드 스냅");
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

                        ui.add_space(10.0);
                        ui.weak("스탯·진영·AI 필드는 패널 명세를 받은 뒤 추가됩니다.");
                    }
                }
                many => {
                    ui.label(format!("{}개 선택됨", many.len()));
                    ui.weak("뷰포트에서 끌면 함께 이동합니다.");
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
    ui.monospace(format!("크기 {:.1} × {:.1} m", s.x / 100.0, s.y / 100.0));
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
                .add(egui::DragValue::new(value).speed(speed).fixed_decimals(1));
            changed |= r.changed();
            // 드래그를 놓았거나, 직접 입력 후 포커스를 잃었을 때 = 편집 한 번 끝
            finished |= r.drag_stopped() || r.lost_focus();
        }
        self.ui.end_row();
        (changed, finished)
    }
}

/// 좌표 입력 표. 단위(cm)는 머리글에 한 번만 적어 폭을 아낀다 — 좁은 사이드바에서
/// 입력칸마다 단위를 붙이면 패널 밖으로 넘친다 (스크린샷으로 확인한 문제).
fn vec2_grid<R>(ui: &mut egui::Ui, id: &str, body: impl FnOnce(&mut Vec2Row<'_>) -> R) -> R {
    egui::Grid::new(id)
        .num_columns(3)
        .spacing([6.0, 4.0])
        .show(ui, |ui| {
            ui.label("");
            ui.weak("X (cm)");
            ui.weak("Y (cm)");
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
            tolerance: PICK_TOLERANCE_PX * world_per_px,
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

/// cm 값을 읽기 쉬운 단위로.
fn format_length(cm: f32) -> String {
    if cm >= 100_000.0 {
        format!("{:.0} km", cm / 100_000.0)
    } else if cm >= 100.0 {
        format!("{:.0} m", cm / 100.0)
    } else {
        format!("{cm:.0} cm")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_formatting_picks_units() {
        assert_eq!(format_length(50.0), "50 cm");
        assert_eq!(format_length(200.0), "2 m");
        assert_eq!(format_length(500_000.0), "5 km");
    }

    #[test]
    fn color_conversion_clamps() {
        assert_eq!(
            to_color32([2.0, -1.0, 0.5, 1.0]),
            egui::Color32::from_rgba_unmultiplied(255, 0, 128, 255)
        );
    }
}
