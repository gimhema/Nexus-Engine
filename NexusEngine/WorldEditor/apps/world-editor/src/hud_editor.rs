//! HUD 편집기 — 에디터 안에서 HUD 배치를 고친다 (단계 2 P6).
//!
//! ```text
//! ┌ HUD 편집기 ───────────────┐      플레이 화면
//! │ hp_text   (글자)          │      ┌──────────────────┐
//! │ hp_bar    (막대) ◀ 선택    │      │  HP 200          │
//! │ exp_bar   (막대)          │      │ ▣▣▣▣▣▣░░░ ◀ 테두리 │
//! │ items     (아이템 창)      │      │                  │
//! │ 앵커 [왼쪽 아래 ▼]        │      └──────────────────┘
//! │ 위치 x 12  y -26          │       ← 화면에서 끌어 옮긴다
//! │ [저장] [되돌리기] [원래대로]│
//! └───────────────────────────┘
//! ```
//!
//! - **편집 대상은 `data/hud.ron` 의 위젯 목록**이다 (P6). 이 창이 고치고 저장한다.
//! - 고친 값은 **그 자리에서 화면에 반영된다** — HUD 는 매 프레임 데이터를 보고 다시 그린다.
//! - 위젯은 플레이 화면에서 **끌어서** 옮긴다. 끌기는 편집과 같은 `PointerInput` 으로 들어오고,
//!   편집기가 열려 있는 동안에는 클릭이 게임 명령(이동·공격)으로 가지 않는다.
//! - 되돌리기는 **이 창 안에서만** 쌓인다 — 존 편집 언두와 섞지 않는다 (다른 일이므로).
//! - 저장은 임시 파일 → 이름 바꾸기, 저장 전에 **다시 읽어 검증**한다 (터레인 자동 추가와 같은 방식).

use std::path::Path;

use nexus_core::Vec2;
use nexus_render_wgpu::egui;

use crate::edit::PointerInput;
use crate::hud::{Anchor, BarSource, Hud, HudFile, Rect, WidgetFile, WidgetKind};
use crate::play::PlaySession;

/// 되돌리기 기록 상한 — 존 편집(256)보다 짧게. HUD 는 위젯 몇 개뿐이다.
const HISTORY_LIMIT: usize = 64;

/// 새 위젯의 기본 크기·문구.
const NEW_BAR: WidgetKind = WidgetKind::Bar {
    source: BarSource::Hp,
    size: (110, 9),
    color: (0.85, 0.25, 0.25),
};

/// 편집 중인 상태. 창이 닫혀 있어도 고치던 내용은 남는다.
#[derive(Debug, Default)]
pub(crate) struct HudEditor {
    open: bool,
    /// 고른 위젯의 이름.
    selected: Option<String>,
    /// 편집 중인 위젯 목록 — 창을 열 때 HUD 에서 복사한다.
    widgets: Vec<WidgetFile>,
    scale: u32,
    /// 마지막으로 저장(또는 읽은) 상태 — "저장 안 됨" 판정과 "원래대로".
    saved: Vec<WidgetFile>,
    saved_scale: u32,
    history: Vec<(Vec<WidgetFile>, u32)>,
    drag: Option<Drag>,
    status: Option<(bool, String)>,
    new_name: String,
}

/// 끌고 있는 위젯.
#[derive(Clone, Debug)]
struct Drag {
    id: String,
    /// 누른 지점 (화면 픽셀).
    from: Vec2,
    /// 누를 때의 `pos` — 델타를 여기에 더한다 (끌기 중 누적 오차가 쌓이지 않게).
    start: (i32, i32),
}

impl HudEditor {
    /// 창을 연다. HUD 정의에서 위젯을 복사해 온다.
    pub(crate) fn open(&mut self, hud: &Hud) {
        self.open = true;
        if let Some(file) = hud.file() {
            self.widgets = file.widgets.clone();
            self.scale = file.scale;
            self.saved = file.widgets.clone();
            self.saved_scale = file.scale;
            if self.selected.is_none() {
                self.selected = self.widgets.first().map(|w| w.id.clone());
            }
        } else {
            self.status = Some((
                true,
                String::from("HUD 정의를 읽지 못했습니다 — data/hud.ron 을 확인하세요"),
            ));
        }
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    /// 고른 위젯 이름 — HUD 가 테두리를 그리는 데 쓴다.
    pub(crate) fn selected(&self) -> Option<&str> {
        self.open.then_some(self.selected.as_deref()).flatten()
    }

    /// 저장하지 않은 변경이 있다.
    pub(crate) fn is_dirty(&self) -> bool {
        self.open && (self.widgets != self.saved || self.scale != self.saved_scale)
    }

    /// 이름으로 고른다 — 자동 검증(`NEXUS_SCRIPT`)용.
    pub(crate) fn select(&mut self, id: &str) -> bool {
        if self.widgets.iter().any(|w| w.id == id) {
            self.selected = Some(id.to_owned());
            true
        } else {
            self.status = Some((true, format!("'{id}' 위젯이 없습니다")));
            false
        }
    }

    /// 고른 위젯을 `(dx, dy)` HUD 픽셀만큼 옮긴다 — 끌기와 같은 경로(기록 포함).
    pub(crate) fn nudge(&mut self, delta: (i32, i32)) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        self.remember();
        if let Some(w) = self.widgets.iter_mut().find(|w| w.id == id) {
            w.pos = (w.pos.0 + delta.0, w.pos.1 + delta.1);
        }
    }

    /// 플레이 화면의 포인터. **처리했으면 `true`** — 그러면 클릭이 게임 명령으로 가지 않는다.
    pub(crate) fn handle_pointer(
        &mut self,
        hud: &Hud,
        play: &PlaySession,
        viewport: (f32, f32),
        p: &PointerInput,
    ) -> bool {
        if !self.open {
            return false;
        }
        let Some(at) = p.screen else {
            return false;
        };

        if p.pressed && p.over_viewport {
            // 위에 그려진 것이 먼저 잡힌다 — 목록 뒤쪽이 위다.
            let hit = self
                .widgets
                .iter()
                .rev()
                .find(|w| hud.widget_rect(w, play, viewport).contains(at));
            match hit {
                Some(w) => {
                    self.selected = Some(w.id.clone());
                    self.drag = Some(Drag {
                        id: w.id.clone(),
                        from: at,
                        start: w.pos,
                    });
                    self.remember();
                    return true;
                }
                // 빈 곳을 누르면 선택만 해제한다 — 게임 명령으로 새지 않게 여기서 멈춘다.
                None => {
                    self.selected = None;
                    return true;
                }
            }
        }

        let Some(drag) = self.drag.clone() else {
            return false;
        };
        if let Some(w) = self.widgets.iter().find(|w| w.id == drag.id).cloned() {
            let delta = hud.drag_delta(&w, at - drag.from, p.snap);
            if let Some(w) = self.widgets.iter_mut().find(|w| w.id == drag.id) {
                w.pos = (drag.start.0 + delta.0, drag.start.1 + delta.1);
            }
        }
        if p.released {
            // 화면 밖으로 밀어 놓으면 보이지 않아 다시 잡을 수 없다 — 알려만 준다(되돌리기로 복구).
            if let Some(w) = self.widgets.iter().find(|w| w.id == drag.id)
                && !inside(hud.widget_rect(w, play, viewport), viewport)
            {
                self.status = Some((true, format!("'{}' 이 화면 밖으로 나갔습니다", w.id)));
            }
            self.drag = None;
        }
        true
    }

    /// 지금 편집 중인 값을 HUD 에 반영한다 — 매 프레임 부른다 (화면에 바로 보이게).
    pub(crate) fn apply(&self, hud: &mut Hud) {
        if self.open {
            hud.set_widgets(self.widgets.clone(), self.scale);
        }
    }

    /// 창을 그린다.
    pub(crate) fn show(&mut self, ui: &mut egui::Ui, hud: &Hud) {
        let mut open = self.open;
        egui::Window::new("HUD 편집기")
            .open(&mut open)
            .default_size([300.0, 420.0])
            .resizable(true)
            .show(ui.ctx(), |ui| {
                self.list(ui);
                ui.separator();
                self.properties(ui);
                ui.separator();
                self.footer(ui, hud);
            });
        self.open = open;
    }

    fn list(&mut self, ui: &mut egui::Ui) {
        ui.strong("위젯");
        ui.weak("플레이 화면에서 끌어 옮깁니다 (Ctrl: 8픽셀 격자)");
        let ids: Vec<(String, &'static str)> = self
            .widgets
            .iter()
            .map(|w| (w.id.clone(), w.kind.label()))
            .collect();
        for (id, kind) in ids {
            let picked = self.selected.as_deref() == Some(id.as_str());
            if ui
                .selectable_label(picked, format!("{id}  ({kind})"))
                .clicked()
            {
                self.selected = Some(id);
            }
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.new_name)
                    .hint_text("새 이름")
                    .desired_width(90.0),
            );
            let named = !self.new_name.trim().is_empty();
            if ui.add_enabled(named, egui::Button::new("+ 막대")).clicked() {
                self.add(NEW_BAR.clone());
            }
            if ui.add_enabled(named, egui::Button::new("+ 글자")).clicked() {
                self.add(WidgetKind::Text {
                    format: String::from("HP {hp}"),
                });
            }
        });
    }

    fn properties(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.selected.clone() else {
            ui.weak("위젯을 고르세요.");
            return;
        };
        let Some(index) = self.widgets.iter().position(|w| w.id == id) else {
            self.selected = None;
            return;
        };

        // 편집 전 상태를 기록해 두고, 값이 바뀌었을 때만 기록으로 남긴다.
        let before = self.widgets.clone();
        let mut renamed: Option<String> = None;
        {
            let w = &mut self.widgets[index];
            ui.horizontal(|ui| {
                ui.label("이름");
                let mut name = w.id.clone();
                if ui
                    .add(egui::TextEdit::singleline(&mut name).desired_width(140.0))
                    .changed()
                {
                    renamed = Some(name);
                }
            });

            egui::ComboBox::from_label("앵커")
                .selected_text(w.anchor.label())
                .show_ui(ui, |ui| {
                    for a in Anchor::ALL {
                        ui.selectable_value(&mut w.anchor, a, a.label());
                    }
                });

            ui.horizontal(|ui| {
                ui.label("위치");
                ui.add(egui::DragValue::new(&mut w.pos.0).speed(1.0).prefix("x "));
                ui.add(egui::DragValue::new(&mut w.pos.1).speed(1.0).prefix("y "));
            });
            ui.weak("앵커 모서리에서 오른쪽·아래가 + 입니다 (HUD 픽셀).");

            match &mut w.kind {
                WidgetKind::Bar {
                    source,
                    size,
                    color,
                } => {
                    egui::ComboBox::from_label("값")
                        .selected_text(source.label())
                        .show_ui(ui, |ui| {
                            for s in BarSource::ALL {
                                ui.selectable_value(source, s, s.label());
                            }
                        });
                    ui.horizontal(|ui| {
                        ui.label("크기");
                        ui.add(
                            egui::DragValue::new(&mut size.0)
                                .speed(1.0)
                                .range(1..=1000)
                                .prefix("w "),
                        );
                        ui.add(
                            egui::DragValue::new(&mut size.1)
                                .speed(1.0)
                                .range(1..=1000)
                                .prefix("h "),
                        );
                    });
                    let mut rgb = [color.0, color.1, color.2];
                    if ui.color_edit_button_rgb(&mut rgb).changed() {
                        *color = (rgb[0], rgb[1], rgb[2]);
                    }
                }
                WidgetKind::Text { format } => {
                    ui.horizontal(|ui| {
                        ui.label("문구");
                        ui.add(egui::TextEdit::singleline(format).desired_width(160.0));
                    });
                    ui.weak(format!(
                        "쓸 수 있는 값: {}",
                        crate::hud::PLACEHOLDERS
                            .iter()
                            .map(|p| format!("{{{p}}}"))
                            .collect::<Vec<_>>()
                            .join(" ")
                    ));
                    ui.weak("⚠ 폰트에 한글과 '/' 가 없습니다 — 영문 대문자·숫자만 보입니다.");
                }
                WidgetKind::Items { columns } => {
                    ui.horizontal(|ui| {
                        ui.label("칸 수");
                        ui.add(egui::DragValue::new(columns).speed(1.0).range(1..=20));
                    });
                    ui.weak("아이템 창은 I 로 열고 닫습니다 (편집 중에는 늘 보입니다).");
                }
            }
        }

        if let Some(name) = renamed {
            self.rename(index, &name);
        } else if self.widgets != before {
            self.history.push((before, self.scale));
            self.trim_history();
        }

        ui.add_space(4.0);
        if ui.button("이 위젯 삭제").clicked() {
            self.remember();
            self.widgets.remove(index);
            self.selected = None;
        }
    }

    fn footer(&mut self, ui: &mut egui::Ui, hud: &Hud) {
        let before_scale = self.scale;
        ui.horizontal(|ui| {
            ui.label("HUD 배율");
            ui.add(
                egui::DragValue::new(&mut self.scale)
                    .speed(1.0)
                    .range(1..=6),
            );
            ui.weak("정수만 — 픽셀이 뭉개지지 않게");
        });
        if self.scale != before_scale {
            self.history.push((self.widgets.clone(), before_scale));
            self.trim_history();
        }

        ui.horizontal(|ui| {
            let label = if self.is_dirty() {
                "저장 *"
            } else {
                "저장"
            };
            if ui.button(label).clicked() {
                self.save(hud);
            }
            if ui
                .add_enabled(!self.history.is_empty(), egui::Button::new("되돌리기"))
                .clicked()
                && let Some((widgets, scale)) = self.history.pop()
            {
                self.widgets = widgets;
                self.scale = scale;
            }
            if ui
                .add_enabled(self.is_dirty(), egui::Button::new("원래대로"))
                .clicked()
            {
                self.remember();
                self.widgets = self.saved.clone();
                self.scale = self.saved_scale;
            }
        });

        if let Some((error, message)) = &self.status {
            let text = egui::RichText::new(message);
            if *error {
                ui.colored_label(egui::Color32::from_rgb(240, 110, 100), text);
            } else {
                ui.weak(text);
            }
        }
    }

    /// 지금 상태를 `data/hud.ron` 에 저장한다. **저장 전에 다시 읽어 검증**한다.
    pub(crate) fn save(&mut self, hud: &Hud) {
        let Some(file) = hud.file() else {
            self.status = Some((true, String::from("HUD 정의가 없어 저장할 수 없습니다")));
            return;
        };
        // 있는 파일의 `widgets`·`scale` 만 갈아 끼운다 — 손으로 쓴 주석(폰트 구간 설명)을 지키려고.
        // 파일 모양이 달라 못 끼우면 통째로 다시 쓴다 (그때는 주석이 사라진다고 알린다).
        let base = std::fs::read_to_string(crate::hud::HUD_PATH)
            .unwrap_or_else(|_| crate::hud::embedded_text().to_owned());
        let (text, kept_comments) = match patch(&base, &self.widgets, self.scale) {
            Ok(text) => (text, true),
            Err(_) => {
                let next = HudFile {
                    version: file.version,
                    font: file.font.clone(),
                    panel: file.panel,
                    scale: self.scale.max(1),
                    widgets: self.widgets.clone(),
                };
                (crate::hud::to_ron(&next), false)
            }
        };
        // 써 놓고 나서 읽을 수 없는 파일이 되는 것을 막는다 (이름 중복·없는 값 이름 등).
        if let Err(e) = Hud::read_file(&text) {
            self.status = Some((true, format!("저장하지 않았습니다 — {e}")));
            return;
        }
        match write(Path::new(crate::hud::HUD_PATH), &text) {
            Ok(()) => {
                self.saved = self.widgets.clone();
                self.saved_scale = self.scale;
                let note = if kept_comments {
                    String::new()
                } else {
                    String::from(" (파일을 다시 써서 주석이 사라졌습니다)")
                };
                self.status = Some((false, format!("{} 저장{note}", crate::hud::HUD_PATH)));
            }
            Err(e) => self.status = Some((true, e)),
        }
    }

    fn add(&mut self, kind: WidgetKind) {
        let id = self.new_name.trim().to_owned();
        if self.widgets.iter().any(|w| w.id == id) {
            self.status = Some((true, format!("'{id}' 는 이미 있습니다")));
            return;
        }
        self.remember();
        self.widgets.push(WidgetFile {
            id: id.clone(),
            anchor: Anchor::TopLeft,
            pos: (12, 12),
            kind,
        });
        self.selected = Some(id);
        self.new_name.clear();
    }

    fn rename(&mut self, index: usize, name: &str) {
        let taken = self
            .widgets
            .iter()
            .enumerate()
            .any(|(i, w)| i != index && w.id == name);
        if taken || name.trim().is_empty() {
            // 조용히 무시한다 — 글자를 지우는 도중에 경고가 깜빡이지 않게.
            return;
        }
        self.remember();
        self.widgets[index].id = name.to_owned();
        self.selected = Some(name.to_owned());
    }

    /// 지금 상태를 되돌리기 기록에 넣는다 (바꾸기 **전에** 부른다).
    fn remember(&mut self) {
        self.history.push((self.widgets.clone(), self.scale));
        self.trim_history();
    }

    fn trim_history(&mut self) {
        if self.history.len() > HISTORY_LIMIT {
            self.history.remove(0);
        }
    }
}

/// 파일에서 **`widgets` 블록과 `scale` 줄만** 바꿔 끼운다.
///
/// 파일을 통째로 다시 쓰면 손으로 쓴 주석(폰트 구간 설명 등)이 사라진다 — 지형 데이터의
/// 자동 추가와 같은 이유로 텍스트를 갈아 끼운다. 블록을 찾지 못하면 오류이고,
/// 그때는 부르는 쪽이 통째로 다시 쓴다.
fn patch(text: &str, widgets: &[WidgetFile], scale: u32) -> Result<String, String> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.trim_start().starts_with("widgets:"))
        .ok_or_else(|| String::from("widgets 블록을 찾지 못함"))?;
    // 블록의 끝 — 같은 들여쓰기의 "]," 줄.
    let indent = lines[start].len() - lines[start].trim_start().len();
    let end = lines[start + 1..]
        .iter()
        .position(|l| {
            let t = l.trim_start();
            (t == "]," || t == "]") && l.len() - t.len() == indent
        })
        .map(|i| start + 1 + i)
        .ok_or_else(|| String::from("widgets 블록이 닫히지 않음"))?;

    let mut out: Vec<String> = Vec::with_capacity(lines.len() + widgets.len() * 8);
    for (i, line) in lines.iter().enumerate() {
        if i == start {
            out.push(widgets_block(widgets, indent));
            continue;
        }
        if i > start && i <= end {
            continue; // 옛 블록은 버린다
        }
        if line.trim_start().starts_with("scale:") {
            let pad = " ".repeat(line.len() - line.trim_start().len());
            out.push(format!("{pad}scale: {},", scale.max(1)));
            continue;
        }
        out.push((*line).to_owned());
    }
    let mut text = out.join("\n");
    text.push('\n');
    Ok(text)
}

/// 위젯 목록을 `widgets: [...],` 블록 문자열로. 들여쓰기는 원래 파일에 맞춘다.
fn widgets_block(widgets: &[WidgetFile], indent: usize) -> String {
    let pad = " ".repeat(indent);
    let config = ron::ser::PrettyConfig::new()
        .new_line("\n")
        .indentor("    ")
        // 손으로 쓴 파일과 같은 모양으로 — 이름 없는 구조체, 한 위젯의 kind 는 한 줄.
        .struct_names(false)
        .depth_limit(1);
    let mut out = format!("{pad}widgets: [\n");
    for w in widgets {
        let body = ron::ser::to_string_pretty(w, config.clone()).expect("위젯 직렬화 실패");
        for line in body.lines() {
            out.push_str(&format!("{pad}    {line}\n"));
        }
        // 마지막 줄 뒤에 쉼표.
        let last = out.trim_end().len();
        out.truncate(last);
        out.push_str(",\n");
    }
    out.push_str(&format!("{pad}],"));
    out
}

/// 임시 파일에 쓴 뒤 이름 바꾸기 — 도중에 멈춰도 원래 파일이 반쯤 덮이지 않는다.
fn write(path: &Path, text: &str) -> Result<(), String> {
    let fail = |e: std::io::Error| format!("{}: {e}", path.display());
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(fail)?;
    }
    let tmp = path.with_extension("ron.tmp");
    std::fs::write(&tmp, text).map_err(fail)?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        fail(e)
    })
}

/// 사각형이 화면 안에 있는지 — 편집기가 위젯을 화면 밖으로 밀어 놨는지 보는 데 쓴다.
pub(crate) fn inside(r: Rect, viewport: (f32, f32)) -> bool {
    r.x >= 0.0 && r.y >= 0.0 && r.x + r.w <= viewport.0 && r.y + r.h <= viewport.1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor(widgets: Vec<WidgetFile>) -> HudEditor {
        HudEditor {
            open: true,
            widgets: widgets.clone(),
            saved: widgets,
            scale: 2,
            saved_scale: 2,
            ..HudEditor::default()
        }
    }

    fn bar(id: &str, pos: (i32, i32)) -> WidgetFile {
        WidgetFile {
            id: id.to_owned(),
            anchor: Anchor::BottomLeft,
            pos,
            kind: NEW_BAR.clone(),
        }
    }

    #[test]
    fn nudging_moves_the_selected_widget_and_can_be_undone() {
        let mut ed = editor(vec![bar("hp", (12, -26))]);
        ed.select("hp");
        ed.nudge((4, -8));
        assert_eq!(ed.widgets[0].pos, (16, -34));
        assert!(ed.is_dirty(), "저장 안 됨 표시");

        let (widgets, scale) = ed.history.pop().unwrap();
        ed.widgets = widgets;
        ed.scale = scale;
        assert_eq!(ed.widgets[0].pos, (12, -26));
        assert!(!ed.is_dirty(), "되돌리면 저장 상태와 같아진다");
    }

    #[test]
    fn selecting_an_unknown_widget_says_so() {
        let mut ed = editor(vec![bar("hp", (0, 0))]);
        assert!(!ed.select("없는것"));
        assert!(ed.status.as_ref().unwrap().0, "오류로 알린다");
        assert_eq!(ed.selected, None);
    }

    #[test]
    fn adding_refuses_a_name_that_is_taken() {
        let mut ed = editor(vec![bar("hp", (0, 0))]);
        ed.new_name = String::from("hp");
        ed.add(NEW_BAR.clone());
        assert_eq!(ed.widgets.len(), 1, "이름이 겹치면 늘지 않는다");
        ed.new_name = String::from("mp");
        ed.add(NEW_BAR.clone());
        assert_eq!(ed.widgets.len(), 2);
        assert_eq!(ed.selected.as_deref(), Some("mp"), "새 위젯이 골라진다");
    }

    #[test]
    fn renaming_keeps_names_unique() {
        let mut ed = editor(vec![bar("hp", (0, 0)), bar("exp", (0, 0))]);
        ed.rename(1, "hp");
        assert_eq!(ed.widgets[1].id, "exp", "겹치는 이름은 무시한다");
        ed.rename(1, "exp_bar");
        assert_eq!(ed.widgets[1].id, "exp_bar");
        assert_eq!(ed.selected.as_deref(), Some("exp_bar"));
    }

    #[test]
    fn inside_sees_widgets_pushed_off_screen() {
        let viewport = (800.0, 600.0);
        assert!(inside(
            Rect {
                x: 10.0,
                y: 10.0,
                w: 100.0,
                h: 20.0
            },
            viewport
        ));
        assert!(!inside(
            Rect {
                x: 760.0,
                y: 10.0,
                w: 100.0,
                h: 20.0
            },
            viewport
        ));
    }

    /// 저장은 파일을 통째로 다시 쓰지 않는다 — 주석·폰트 구간이 그대로 남아야 한다.
    #[test]
    fn patching_keeps_comments_and_replaces_only_widgets_and_scale() {
        let before = crate::hud::embedded_text();
        let widgets = vec![bar("hp_text", (12, -46)), bar("새것", (1, 2))];
        let after = patch(before, &widgets, 3).expect("블록을 찾는다");

        assert!(after.contains("⚠ 이 폰트에는 한글도"), "머리 주석이 남는다");
        assert!(
            after.contains(r#"(chars: "012", x: 216, y: 0, size: (8, 8)),"#),
            "폰트 구간이 그대로다"
        );
        assert!(after.contains("scale: 3,"), "배율이 바뀐다");
        assert!(!after.contains("scale: 2,"), "옛 배율은 남지 않는다");
        assert!(after.contains(r#"id: "새것","#), "새 위젯이 들어간다");
        assert!(
            !after.contains(r#"id: "items","#),
            "목록에 없는 옛 위젯은 사라진다"
        );
        // 다시 읽을 수 있어야 한다.
        let read = Hud::read_file(&after).expect("고친 파일도 읽힌다");
        assert_eq!(read.scale, 3);
        assert_eq!(read.widgets.len(), 2);

        // 두 번 끼워도 같은 결과 — 저장을 반복해도 파일이 불어나지 않는다.
        assert_eq!(patch(&after, &widgets, 3).unwrap(), after);
    }

    #[test]
    fn patching_reports_a_file_it_cannot_patch() {
        assert!(patch("(\n    version: 1,\n)\n", &[], 2).is_err());
        assert!(
            patch("(\n    widgets: [\n)\n", &[], 2).is_err(),
            "블록이 닫히지 않으면 오류"
        );
    }
}
