//! 에디터 UI — egui 통합과 화면 배치.
//!
//! ```text
//! ┌────────────────────── 메뉴 바 ──────────────────────┐
//! │  씬  │                 뷰포트                │ 인스펙터 │
//! ├────────────────────── 상태 바 ──────────────────────┤
//! ```
//!
//! 이 단계에서는 **골격만** 둔다. 각 패널의 실제 내용은 패널별 명세를 받아 하나씩 채운다.
//!
//! 뷰포트 조작(팬·줌)도 여기서 처리한다. egui 가 포인터가 패널 위에 있는지를 이미 알고
//! 있으므로, 패널을 드래그하다가 카메라가 따라 움직이는 문제가 구조적으로 생기지 않는다.
//! 또 카메라는 시뮬레이션 상태가 아니라 뷰이므로 20Hz 고정 스텝이 아니라 매 프레임 갱신한다.

use std::path::PathBuf;
use std::sync::Arc;

use nexus_core::{Camera2d, Vec2};
use nexus_platform::{WindowEvent, WindowTarget};
use nexus_render_wgpu::{UiFrame, egui};

use crate::grid;

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
    "/System/Library/Fonts/AppleSDGothicNeo.ttc",
];

/// 씬 목록에 표시할 항목.
#[derive(Debug, Clone)]
pub(crate) struct OutlineItem {
    pub(crate) label: String,
    /// sRGB RGBA
    pub(crate) color: [f32; 4],
}

/// UI 가 읽는 프레임 통계.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FrameStats {
    pub(crate) fps: f32,
    pub(crate) quads: usize,
    pub(crate) ticks: u64,
}

/// 한 프레임 동안 UI 가 편집기에 요청한 것.
#[derive(Debug, Default)]
pub(crate) struct UiActions {
    pub(crate) reset_view: bool,
    pub(crate) screenshot: bool,
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
        camera: &mut Camera2d,
        outline: &[OutlineItem],
        stats: FrameStats,
    ) -> (UiFrame, UiActions) {
        let raw = self.state.take_egui_input(target.window());
        let mut actions = UiActions::default();
        let cursor_world = &mut self.cursor_world;

        let output = self.ctx.run_ui(raw, |ui| {
            menu_bar(ui, &mut actions);
            status_bar(ui, camera, *cursor_world, stats);
            outline_panel(ui, outline);
            inspector_panel(ui);
            viewport(ui, camera, cursor_world, &mut actions);
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

fn menu_bar(ui: &mut egui::Ui, actions: &mut UiActions) {
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
                ui.add_enabled(
                    false,
                    egui::Button::new("실행 취소").shortcut_text("Ctrl+Z"),
                );
                ui.add_enabled(
                    false,
                    egui::Button::new("다시 실행").shortcut_text("Ctrl+Y"),
                );
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

    // 단축키는 텍스트 입력 중이 아닐 때만 받는다.
    let ctx = ui.ctx().clone();
    if !ctx.egui_wants_keyboard_input() {
        ctx.input(|i| {
            actions.reset_view |= i.key_pressed(egui::Key::Home);
            actions.screenshot |= i.key_pressed(egui::Key::F12);
        });
    }
}

fn status_bar(ui: &mut egui::Ui, camera: &Camera2d, cursor: Option<Vec2>, stats: FrameStats) {
    egui::Panel::bottom("status_bar").show(ui, |ui| {
        ui.horizontal(|ui| {
            match cursor {
                Some(p) => ui.monospace(format!("X {:>9.1}  Y {:>9.1} cm", p.x, p.y)),
                None => ui.monospace("X         —  Y         — cm"),
            };
            ui.separator();
            ui.monospace(format!(
                "시야 {:.1} m · 그리드 {} ",
                camera.view_height / 100.0,
                format_length(grid::pick_spacing(camera.view_height)),
            ));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.monospace(format!(
                    "{:.0} fps · 쿼드 {} · tick {}",
                    stats.fps, stats.quads, stats.ticks
                ));
            });
        });
    });
}

fn outline_panel(ui: &mut egui::Ui, items: &[OutlineItem]) {
    egui::Panel::left("outline")
        .resizable(true)
        .default_size(SIDE_PANEL_WIDTH)
        .show(ui, |ui| {
            ui.heading("씬");
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                for item in items {
                    ui.horizontal(|ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 2.0, to_color32(item.color));
                        ui.label(&item.label);
                    });
                }
            });
        });
}

fn inspector_panel(ui: &mut egui::Ui) {
    egui::Panel::right("inspector")
        .resizable(true)
        .default_size(SIDE_PANEL_WIDTH)
        .show(ui, |ui| {
            ui.heading("인스펙터");
            ui.separator();
            ui.weak("선택된 오브젝트 없음");
        });
}

/// 남은 영역 전체를 씬 뷰포트로 쓴다. 여기서 카메라 팬·줌을 처리한다.
///
/// - 가운데 또는 오른쪽 버튼 드래그: 팬 (왼쪽 버튼은 M5 선택용으로 비워 둔다)
/// - 휠: 커서 기준 줌
fn viewport(
    ui: &mut egui::Ui,
    camera: &mut Camera2d,
    cursor_world: &mut Option<Vec2>,
    actions: &mut UiActions,
) {
    egui::CentralPanel::no_frame().show(ui, |ui| {
        let rect = ui.max_rect();
        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        let ppp = ui.ctx().pixels_per_point();

        // 논리 포인트 → 물리 픽셀. 씬 렌더러와 카메라는 물리 픽셀 기준이다.
        let px = [
            (rect.min.x * ppp).round().max(0.0) as u32,
            (rect.min.y * ppp).round().max(0.0) as u32,
            (rect.width() * ppp).round().max(0.0) as u32,
            (rect.height() * ppp).round().max(0.0) as u32,
        ];
        actions.viewport_px = Some(px);
        camera.viewport = (px[2], px[3]);

        let to_local_px =
            |pos: egui::Pos2| Vec2::new((pos.x - rect.min.x) * ppp, (pos.y - rect.min.y) * ppp);

        if response.dragged_by(egui::PointerButton::Middle)
            || response.dragged_by(egui::PointerButton::Secondary)
        {
            let d = response.drag_delta() * ppp;
            camera.pan_by_pixels(Vec2::new(d.x, d.y));
        }

        *cursor_world = response
            .hover_pos()
            .map(|pos| camera.screen_to_world(to_local_px(pos)));

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
                camera.zoom_at(notches, to_local_px(pos));
            }
        }
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
