//! 위젯 편집기 — 에디터 안에서 화면(UI)을 만들고 배치한다 (단계 2 P7).
//!
//! ```text
//! ┌ 위젯 편집기 ──────────────┐      게임 화면 / 미리보기
//! │ 화면 [hud ▼] [+ 새 화면]   │      ┌──────────────────┐
//! │ ▾ items_window (창)       │      │        ┌ ITEMS ─┐ │
//! │    items_title (글자)     │      │        │ ▣ ▣ ▣ │ │
//! │    items       (아이템)    │      │ HP 161 └───────┘ │
//! │ hp_bar (막대) ◀ 선택       │      │ ▣▣▣▣░░ ◀ 테두리   │
//! │ 앵커 [왼쪽 아래 ▼]        │      └──────────────────┘
//! │ 위치 x 12  y -26          │       ← 화면에서 끌어 옮긴다
//! │ [저장] [되돌리기] [원래대로]│         모서리를 끌면 크기
//! └───────────────────────────┘
//! ```
//!
//! - 편집 대상은 **화면 파일** `ui/<번호>.ui.ron` 의 위젯 트리다. 이 창이 고치고 저장한다.
//! - 고친 값은 **그 자리에서 화면에 반영된다** — 화면은 매 프레임 데이터를 보고 다시 그린다.
//! - **플레이 중이 아니어도 편집한다.** 메인 화면·설정 화면은 게임 밖 화면이기 때문이다
//!   (값은 [`Values::preview`] 의 미리보기 숫자로 채운다).
//! - 위젯은 화면에서 **끌어서** 옮기고, **오른쪽 아래 모서리**를 끌면 크기를 정한다.
//!   끌기는 편집과 같은 `PointerInput` 으로 들어오고, 편집기가 열려 있는 동안에는 클릭이
//!   게임 명령(이동·공격)이나 버튼 동작으로 가지 않는다.
//! - 되돌리기는 **이 창 안에서만** 쌓인다 — 존 편집 언두와 섞지 않는다 (다른 일이므로).
//! - 저장은 임시 파일 → 이름 바꾸기, 저장 전에 **다시 읽어 검증**한다.
//!   화면 파일은 편집기가 소유하므로 통째로 다시 쓰고, 테마(`data/ui.ron`)는 `scale` 줄만
//!   갈아 끼운다 — 그래서 테마의 주석은 남는다.

use std::path::Path;

use nexus_core::Vec2;
use nexus_render_wgpu::egui;

use crate::edit::PointerInput;
use crate::screen::{
    Action, Anchor, BarSource, Rect, ScreenFile, Screens, Show, Values, Widget, WidgetKind,
};

/// 되돌리기 기록 상한 — 존 편집(256)보다 짧게. 화면은 위젯 몇십 개뿐이다.
const HISTORY_LIMIT: usize = 64;

/// 크기 조절 손잡이의 한 변 (화면 픽셀).
const HANDLE_PX: f32 = 10.0;

/// 끌고 있는 것 — 옮기기인가 크기인가.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Move,
    Resize,
}

/// 끌고 있는 위젯.
#[derive(Clone, Debug)]
struct Drag {
    id: String,
    mode: Mode,
    /// 누른 지점 (화면 픽셀).
    from: Vec2,
    /// 누를 때의 `pos`·`size` — 델타를 여기에 더한다 (끌기 중 오차가 쌓이지 않게).
    start_pos: (i32, i32),
    start_size: (u32, u32),
}

/// 편집 중인 상태. 창이 닫혀 있어도 고치던 내용은 남는다.
#[derive(Debug, Default)]
pub(crate) struct ScreenEditor {
    open: bool,
    /// 편집 중인 화면 번호 (`ui/<번호>.ui.ron`).
    current: String,
    /// 편집 중인 화면 — 창을 열 때 [`Screens`] 에서 복사한다.
    screen: Option<ScreenFile>,
    /// 마지막으로 저장(또는 읽은) 상태 — "저장 안 됨" 판정과 "원래대로".
    saved: Option<ScreenFile>,
    scale: u32,
    saved_scale: u32,
    /// 고른 위젯의 이름.
    selected: Option<String>,
    history: Vec<(ScreenFile, u32)>,
    drag: Option<Drag>,
    status: Option<(bool, String)>,
    new_widget: String,
    new_screen: String,
}

impl ScreenEditor {
    /// 창을 연다. 열려 있으면 닫는다 (메뉴·단축키가 토글로 쓴다).
    pub(crate) fn toggle(&mut self, screens: &Screens) {
        if self.open {
            self.open = false;
        } else {
            self.open(screens);
        }
    }

    /// 창을 열고 지금 화면을 복사해 온다.
    pub(crate) fn open(&mut self, screens: &Screens) {
        self.open = true;
        self.scale = screens.theme().map_or(2, |t| t.scale.max(1));
        self.saved_scale = self.scale;
        if self.screen.is_none() {
            let id = screens
                .ids()
                .into_iter()
                .find(|id| id == "hud")
                .or_else(|| screens.ids().into_iter().next());
            match id {
                Some(id) => self.load(screens, &id),
                None => {
                    self.status = Some((
                        true,
                        String::from("화면 파일이 없습니다 — ui/ 폴더를 확인하세요"),
                    ));
                }
            }
        }
    }

    /// 편집할 화면을 바꾼다. 저장하지 않은 변경이 있으면 막는다 (다른 파일로 새면 잃는다).
    fn load(&mut self, screens: &Screens, id: &str) {
        if self.is_dirty() {
            self.status = Some((true, format!("'{}' 를 먼저 저장하세요", self.current)));
            return;
        }
        let Some(screen) = screens.screen(id) else {
            self.status = Some((true, format!("'{id}' 화면이 없습니다")));
            return;
        };
        self.current = id.to_owned();
        self.screen = Some(screen.clone());
        self.saved = Some(screen.clone());
        self.history.clear();
        self.selected = screen.widgets.first().map(|w| w.id.clone());
        self.drag = None;
        self.status = None;
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    /// 편집 중인 화면 번호 — 플레이 중이 아닐 때 이 화면을 미리 보여 준다.
    pub(crate) fn preview_screen(&self) -> Option<&str> {
        self.open.then_some(self.current.as_str())
    }

    /// 고른 위젯 이름 — 화면이 테두리를 그리는 데 쓴다.
    pub(crate) fn selected(&self) -> Option<&str> {
        self.open.then_some(self.selected.as_deref()).flatten()
    }

    /// 저장하지 않은 변경이 있다.
    pub(crate) fn is_dirty(&self) -> bool {
        self.screen != self.saved || (self.screen.is_some() && self.scale != self.saved_scale)
    }

    /// 마지막 알림 — 상태 바가 가져간다.
    pub(crate) fn take_status(&mut self) -> Option<(bool, String)> {
        self.status.take()
    }

    /// 이름으로 고른다 — 자동 검증(`NEXUS_SCRIPT`)용.
    pub(crate) fn select(&mut self, id: &str) -> bool {
        let found = self
            .screen
            .as_ref()
            .is_some_and(|s| find(&s.widgets, id).is_some());
        if found {
            self.selected = Some(id.to_owned());
        } else {
            self.status = Some((true, format!("'{id}' 위젯이 없습니다")));
        }
        found
    }

    /// 편집할 화면을 번호로 바꾼다 — 스크립트용.
    pub(crate) fn switch(&mut self, screens: &Screens, id: &str) {
        self.load(screens, id);
    }

    /// 고른 위젯을 `(dx, dy)` UI 픽셀만큼 옮긴다 — 끌기와 같은 경로(기록 포함).
    pub(crate) fn nudge(&mut self, delta: (i32, i32)) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        self.remember();
        if let Some(screen) = self.screen.as_mut()
            && let Some(w) = find_mut(&mut screen.widgets, &id)
        {
            w.pos = (w.pos.0 + delta.0, w.pos.1 + delta.1);
        }
    }

    /// 지금 편집 중인 값을 [`Screens`] 에 반영한다 — 매 프레임 부른다 (화면에 바로 보이게).
    pub(crate) fn apply(&self, screens: &mut Screens) {
        if !self.open {
            return;
        }
        if let Some(screen) = self.screen.as_ref() {
            screens.set_screen(&self.current, screen.clone());
        }
        screens.set_scale(self.scale);
    }

    /// 화면의 포인터. **처리했으면 `true`** — 그러면 클릭이 게임 명령·버튼으로 가지 않는다.
    pub(crate) fn handle_pointer(
        &mut self,
        screens: &Screens,
        values: &Values,
        viewport: (f32, f32),
        p: &PointerInput,
    ) -> bool {
        if !self.open {
            return false;
        }
        let Some(screen) = self.screen.clone() else {
            return false;
        };
        let Some(at) = p.screen else {
            return false;
        };

        if p.pressed && p.over_viewport {
            let laid = screens.layout(&screen, values, viewport);
            // 고른 위젯의 오른쪽 아래 손잡이가 가장 먼저 잡힌다 (크기 조절).
            if let Some(id) = self.selected.clone()
                && let Some(l) = laid.iter().find(|l| l.widget.id == id)
                && handle_rect(l.rect).contains(at)
            {
                self.remember();
                self.drag = Some(Drag {
                    id,
                    mode: Mode::Resize,
                    from: at,
                    start_pos: l.widget.pos,
                    start_size: screens.to_ui_size(Vec2::new(l.rect.w, l.rect.h), false),
                });
                return true;
            }
            // 위에 그려진 것이 먼저 잡힌다 — 목록 뒤쪽·자식이 위다.
            match laid.iter().rev().find(|l| l.rect.contains(at)) {
                Some(l) => {
                    self.selected = Some(l.widget.id.clone());
                    self.remember();
                    self.drag = Some(Drag {
                        id: l.widget.id.clone(),
                        mode: Mode::Move,
                        from: at,
                        start_pos: l.widget.pos,
                        start_size: l.widget.size.unwrap_or((1, 1)),
                    });
                }
                // 빈 곳을 누르면 선택만 해제한다 — 게임·버튼으로 새지 않게 여기서 멈춘다.
                None => self.selected = None,
            }
            return true;
        }

        let Some(drag) = self.drag.clone() else {
            return false;
        };
        let delta = at - drag.from;
        if let Some(screen) = self.screen.as_mut()
            && let Some(w) = find_mut(&mut screen.widgets, &drag.id)
        {
            match drag.mode {
                Mode::Move => {
                    let (dx, dy) = screens.drag_delta(delta, p.snap);
                    w.pos = (drag.start_pos.0 + dx, drag.start_pos.1 + dy);
                }
                Mode::Resize => {
                    let s = screens.scale();
                    let size = Vec2::new(
                        drag.start_size.0 as f32 * s + delta.x,
                        drag.start_size.1 as f32 * s + delta.y,
                    );
                    w.size = Some(screens.to_ui_size(size, p.snap));
                }
            }
        }
        if p.released {
            self.drag = None;
            // 화면 밖으로 밀어 놓으면 보이지 않아 다시 잡을 수 없다 — 알려만 준다.
            if let Some(screen) = self.screen.as_ref()
                && let Some(l) = screens
                    .layout(screen, values, viewport)
                    .into_iter()
                    .find(|l| l.widget.id == drag.id)
                && !inside(l.rect, viewport)
            {
                self.status = Some((true, format!("'{}' 이 화면 밖으로 나갔습니다", drag.id)));
            }
        }
        true
    }

    /// 창을 그린다.
    pub(crate) fn show(&mut self, ui: &mut egui::Ui, screens: &Screens) {
        let mut open = self.open;
        egui::Window::new("위젯 편집기")
            .open(&mut open)
            .default_size([320.0, 520.0])
            .resizable(true)
            .show(ui.ctx(), |ui| {
                self.screens_row(ui, screens);
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| self.tree(ui));
                ui.separator();
                self.properties(ui);
                ui.separator();
                self.footer(ui);
            });
        self.open = open;
    }

    /// 화면 고르기 + 새 화면.
    fn screens_row(&mut self, ui: &mut egui::Ui, screens: &Screens) {
        let ids = screens.ids();
        let mut pick: Option<String> = None;
        ui.horizontal(|ui| {
            ui.label("화면");
            egui::ComboBox::from_id_salt("screen-pick")
                .selected_text(if self.current.is_empty() {
                    String::from("없음")
                } else {
                    self.current.clone()
                })
                .show_ui(ui, |ui| {
                    for id in &ids {
                        let name = screens.screen(id).map_or("", |s| s.name.as_str());
                        if ui
                            .selectable_label(*id == self.current, format!("{id}  ({name})"))
                            .clicked()
                        {
                            pick = Some(id.clone());
                        }
                    }
                });
            if self.is_dirty() {
                ui.colored_label(egui::Color32::from_rgb(240, 190, 90), "저장 안 됨");
            }
        });
        if let Some(id) = pick
            && id != self.current
        {
            self.load(screens, &id);
        }

        if let Some(screen) = self.screen.as_mut() {
            ui.horizontal(|ui| {
                ui.label("이름");
                ui.add(egui::TextEdit::singleline(&mut screen.name).desired_width(150.0));
            });
        }
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.new_screen)
                    .hint_text("새 화면 번호 (영문 소문자)")
                    .desired_width(150.0),
            );
            if ui.button("+ 새 화면").clicked() {
                self.create_screen();
            }
        });
    }

    /// 위젯 계층 — 들여쓰기로 부모·자식을 보여 준다.
    fn tree(&mut self, ui: &mut egui::Ui) {
        let Some(screen) = self.screen.as_ref() else {
            ui.weak("화면을 고르세요.");
            return;
        };
        ui.strong("위젯");
        ui.weak("화면에서 끌어 옮깁니다 (Ctrl: 8픽셀 격자, 오른쪽 아래 모서리: 크기)");
        let rows = ids_with_depth(&screen.widgets);
        if rows.is_empty() {
            ui.weak("아직 위젯이 없습니다 — 아래에서 추가하세요.");
        }
        for (id, depth, kind) in rows {
            let picked = self.selected.as_deref() == Some(id.as_str());
            ui.horizontal(|ui| {
                ui.add_space(depth as f32 * 14.0);
                if ui
                    .selectable_label(picked, format!("{id}  ({kind})"))
                    .clicked()
                {
                    self.selected = Some(id.clone());
                }
            });
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.new_widget)
                    .hint_text("새 위젯 이름")
                    .desired_width(110.0),
            );
            let named = !self.new_widget.trim().is_empty();
            for (label, kind) in [
                ("+ 창", WidgetKind::Panel { frame: true }),
                (
                    "+ 글자",
                    WidgetKind::Text {
                        format: String::from("HP {hp}"),
                    },
                ),
                (
                    "+ 버튼",
                    WidgetKind::Button {
                        label: String::from("BUTTON"),
                        action: Action::None,
                    },
                ),
            ] {
                if ui.add_enabled(named, egui::Button::new(label)).clicked() {
                    self.add(kind);
                }
            }
        });
        ui.weak("고른 위젯이 있으면 그 자식으로 들어갑니다.");
    }

    fn properties(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.selected.clone() else {
            ui.weak("위젯을 고르세요.");
            return;
        };
        let Some(screen) = self.screen.as_ref() else {
            return;
        };
        if find(&screen.widgets, &id).is_none() {
            self.selected = None;
            return;
        }

        // 편집 전 상태를 기록해 두고, 값이 바뀌었을 때만 기록으로 남긴다.
        let before = screen.clone();
        let mut renamed: Option<String> = None;
        let mut reparent: Option<Option<String>> = None;
        let parents = parent_choices(&before.widgets, &id);
        let parent_now = parent_of(&before.widgets, &id);

        {
            let screen = self.screen.as_mut().expect("위에서 확인했다");
            let w = find_mut(&mut screen.widgets, &id).expect("위에서 확인했다");
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
            ui.weak("앵커 기준점에서 오른쪽·아래가 + 입니다 (UI 픽셀).");

            // 크기 — 고정 / 내용에 맞춤.
            let bar = matches!(w.kind, WidgetKind::Bar { .. });
            let mut fixed = w.size.is_some();
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!bar, egui::Checkbox::new(&mut fixed, "크기 고정"))
                    .changed()
                {
                    w.size = if fixed { Some((60, 20)) } else { None };
                }
                if let Some(size) = w.size.as_mut() {
                    ui.add(
                        egui::DragValue::new(&mut size.0)
                            .speed(1.0)
                            .range(1..=2000)
                            .prefix("w "),
                    );
                    ui.add(
                        egui::DragValue::new(&mut size.1)
                            .speed(1.0)
                            .range(1..=2000)
                            .prefix("h "),
                    );
                }
            });
            if w.size.is_none() {
                ui.weak(match w.kind {
                    WidgetKind::Panel { .. } => "창: 부모 사각형을 가득 채웁니다.",
                    _ => "내용 크기에 맞춥니다.",
                });
            }

            egui::ComboBox::from_label("보이기")
                .selected_text(w.show.label())
                .show_ui(ui, |ui| {
                    for s in Show::ALL {
                        ui.selectable_value(&mut w.show, s, s.label());
                    }
                });

            kind_fields(ui, &mut w.kind);

            // 부모 바꾸기 — 자기 자손은 고를 수 없다 (트리가 끊긴다).
            egui::ComboBox::from_label("부모")
                .selected_text(parent_now.clone().unwrap_or_else(|| String::from("최상위")))
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(parent_now.is_none(), "최상위")
                        .clicked()
                    {
                        reparent = Some(None);
                    }
                    for p in &parents {
                        if ui
                            .selectable_label(parent_now.as_deref() == Some(p.as_str()), p)
                            .clicked()
                        {
                            reparent = Some(Some(p.clone()));
                        }
                    }
                });
        }

        if let Some(name) = renamed {
            self.rename(&id, &name);
        } else if let Some(parent) = reparent {
            self.reparent(&id, parent.as_deref());
        } else if self.screen.as_ref() != Some(&before) {
            self.history.push((before, self.scale));
            self.trim_history();
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("위로").clicked() {
                self.reorder(&id, -1);
            }
            if ui.button("아래로").clicked() {
                self.reorder(&id, 1);
            }
            if ui.button("이 위젯 삭제").clicked() {
                self.remember();
                if let Some(screen) = self.screen.as_mut() {
                    take(&mut screen.widgets, &id);
                }
                self.selected = None;
            }
        });
        ui.weak("삭제는 자식까지 함께 지웁니다. 순서 뒤쪽이 화면에서 위에 그려집니다.");
    }

    fn footer(&mut self, ui: &mut egui::Ui) {
        let before_scale = self.scale;
        ui.horizontal(|ui| {
            ui.label("UI 배율");
            ui.add(
                egui::DragValue::new(&mut self.scale)
                    .speed(1.0)
                    .range(1..=6),
            );
            ui.weak("정수만 — 픽셀이 뭉개지지 않게 (테마 전체에 적용)");
        });
        if self.scale != before_scale
            && let Some(screen) = self.screen.clone()
        {
            self.history.push((screen, before_scale));
            self.trim_history();
        }

        ui.horizontal(|ui| {
            let label = if self.is_dirty() {
                "저장 *"
            } else {
                "저장"
            };
            if ui.button(label).clicked() {
                self.save();
            }
            if ui
                .add_enabled(!self.history.is_empty(), egui::Button::new("되돌리기"))
                .clicked()
                && let Some((screen, scale)) = self.history.pop()
            {
                self.screen = Some(screen);
                self.scale = scale;
            }
            if ui
                .add_enabled(self.is_dirty(), egui::Button::new("원래대로"))
                .clicked()
            {
                self.remember();
                self.screen = self.saved.clone();
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

    /// 새 화면을 만든다 — 파일은 저장할 때 생긴다.
    fn create_screen(&mut self) {
        let id = self.new_screen.trim().to_ascii_lowercase();
        if !crate::screen::valid_screen_id(&id) {
            self.status = Some((
                true,
                String::from("화면 번호는 영문 소문자·숫자·_·- 만 (파일 이름이 됩니다)"),
            ));
            return;
        }
        if crate::screen::screen_path(&id).exists() {
            self.status = Some((true, format!("ui/{id}.ui.ron 이 이미 있습니다")));
            return;
        }
        if self.is_dirty() {
            self.status = Some((true, format!("'{}' 를 먼저 저장하세요", self.current)));
            return;
        }
        self.current = id.clone();
        self.screen = Some(ScreenFile::new(&id));
        // 새 화면은 아직 파일이 없으므로 "저장 안 됨" 으로 시작한다.
        self.saved = None;
        self.history.clear();
        self.selected = None;
        self.new_screen.clear();
        self.status = Some((false, format!("새 화면 '{id}' — 저장하면 파일이 생깁니다")));
    }

    /// 새 위젯을 넣는다. 고른 위젯이 있으면 그 **자식**으로.
    fn add(&mut self, kind: WidgetKind) {
        let name = self.new_widget.trim().to_owned();
        let Some(screen) = self.screen.as_ref() else {
            return;
        };
        if name.is_empty() {
            return;
        }
        if find(&screen.widgets, &name).is_some() {
            self.status = Some((true, format!("'{name}' 이름이 이미 있습니다")));
            return;
        }
        self.remember();
        let widget = Widget::new(&name, kind);
        let parent = self.selected.clone();
        let screen = self.screen.as_mut().expect("위에서 확인했다");
        insert_into(&mut screen.widgets, parent.as_deref(), widget);
        self.selected = Some(name);
        self.new_widget.clear();
    }

    /// 이름을 바꾼다 — 겹치면 무시한다 (글자를 지우는 도중에 경고가 깜빡이지 않게).
    fn rename(&mut self, id: &str, name: &str) {
        let Some(screen) = self.screen.as_ref() else {
            return;
        };
        let taken = name != id && find(&screen.widgets, name).is_some();
        if taken || name.trim().is_empty() {
            return;
        }
        self.remember();
        if let Some(screen) = self.screen.as_mut()
            && let Some(w) = find_mut(&mut screen.widgets, id)
        {
            w.id = name.to_owned();
        }
        self.selected = Some(name.to_owned());
    }

    /// 부모를 바꾼다. 자기 자손 밑으로는 넣을 수 없다.
    fn reparent(&mut self, id: &str, parent: Option<&str>) {
        if parent == Some(id) {
            return;
        }
        let Some(screen) = self.screen.as_ref() else {
            return;
        };
        let Some(sub) = find(&screen.widgets, id) else {
            return;
        };
        if let Some(parent) = parent
            && find(&sub.children, parent).is_some()
        {
            self.status = Some((true, String::from("자기 자손을 부모로 삼을 수 없습니다")));
            return;
        }
        self.remember();
        let screen = self.screen.as_mut().expect("위에서 확인했다");
        if let Some(sub) = take(&mut screen.widgets, id) {
            insert_into(&mut screen.widgets, parent, sub);
        }
    }

    /// 형제 사이 순서를 바꾼다 — 뒤가 위에 그려진다.
    fn reorder(&mut self, id: &str, by: i32) {
        let Some(screen) = self.screen.as_ref() else {
            return;
        };
        let parent = parent_of(&screen.widgets, id);
        self.remember();
        let screen = self.screen.as_mut().expect("위에서 확인했다");
        let list = match parent.as_deref() {
            Some(p) => match find_mut(&mut screen.widgets, p) {
                Some(w) => &mut w.children,
                None => return,
            },
            None => &mut screen.widgets,
        };
        let Some(i) = list.iter().position(|w| w.id == id) else {
            return;
        };
        let target = i as i32 + by;
        if target < 0 || target as usize >= list.len() {
            self.history.pop(); // 아무것도 안 바뀌었으면 기록도 남기지 않는다
            return;
        }
        list.swap(i, target as usize);
    }

    /// 지금 상태를 파일에 저장한다. **저장 전에 다시 읽어 검증**한다.
    pub(crate) fn save(&mut self) {
        let Some(screen) = self.screen.clone() else {
            self.status = Some((true, String::from("저장할 화면이 없습니다")));
            return;
        };
        if !crate::screen::valid_screen_id(&self.current) {
            self.status = Some((
                true,
                format!("'{}' 는 파일 이름이 될 수 없습니다", self.current),
            ));
            return;
        }
        let text = crate::screen::to_ron(&screen);
        if let Err(e) = Screens::read_screen(&text) {
            self.status = Some((true, format!("저장하지 않았습니다 — {e}")));
            return;
        }
        let path = crate::screen::screen_path(&self.current);
        if let Err(e) = write(&path, &text) {
            self.status = Some((true, e));
            return;
        }
        let scale_note = match self.save_scale() {
            Ok(()) => String::new(),
            Err(e) => format!(" (배율은 저장하지 못함 — {e})"),
        };
        self.saved = Some(screen);
        self.saved_scale = self.scale;
        self.status = Some((false, format!("{} 저장{scale_note}", path.display())));
    }

    /// 테마의 `scale` 줄만 갈아 끼운다 — 주석을 지키려고 (화면 파일과 규칙이 다르다).
    fn save_scale(&self) -> Result<(), String> {
        if self.scale == self.saved_scale {
            return Ok(());
        }
        let path = Path::new(crate::screen::THEME_PATH);
        let base = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let text = crate::screen::patch_scale(&base, self.scale)?;
        Screens::read_theme(&text)?;
        write(path, &text)
    }

    /// 지금 상태를 되돌리기 기록에 넣는다 (바꾸기 **전에** 부른다).
    fn remember(&mut self) {
        if let Some(screen) = self.screen.clone() {
            self.history.push((screen, self.scale));
            self.trim_history();
        }
    }

    fn trim_history(&mut self) {
        if self.history.len() > HISTORY_LIMIT {
            self.history.remove(0);
        }
    }
}

/// 종류마다 다른 항목.
fn kind_fields(ui: &mut egui::Ui, kind: &mut WidgetKind) {
    match kind {
        WidgetKind::Panel { frame } => {
            ui.checkbox(frame, "창 그림 (끄면 자리만 잡는 투명 그룹)");
        }
        WidgetKind::Text { format } => {
            ui.horizontal(|ui| {
                ui.label("문구");
                ui.add(egui::TextEdit::singleline(format).desired_width(160.0));
            });
            ui.weak(format!(
                "쓸 수 있는 값: {}",
                crate::screen::PLACEHOLDERS
                    .iter()
                    .map(|p| format!("{{{p}}}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            ));
            ui.weak("⚠ 폰트에 한글과 '/' 가 없습니다 — 영문·숫자만 보입니다.");
        }
        WidgetKind::Bar { source, color } => {
            egui::ComboBox::from_label("값")
                .selected_text(source.label())
                .show_ui(ui, |ui| {
                    for s in BarSource::ALL {
                        ui.selectable_value(source, s, s.label());
                    }
                });
            let mut rgb = [color.0, color.1, color.2];
            if ui.color_edit_button_rgb(&mut rgb).changed() {
                *color = (rgb[0], rgb[1], rgb[2]);
            }
        }
        WidgetKind::Items { columns } => {
            ui.horizontal(|ui| {
                ui.label("칸 수");
                ui.add(egui::DragValue::new(columns).speed(1.0).range(1..=20));
            });
        }
        WidgetKind::Button { label, action } => {
            ui.horizontal(|ui| {
                ui.label("글자");
                ui.add(egui::TextEdit::singleline(label).desired_width(120.0));
            });
            let choices = [
                Action::OpenScreen(String::from("settings")),
                Action::Close,
                Action::OpenLevel(String::from("village")),
                Action::OpenStartLevel,
                Action::Resume,
                Action::Quit,
                Action::None,
            ];
            egui::ComboBox::from_label("동작")
                .selected_text(action.label())
                .show_ui(ui, |ui| {
                    for choice in choices {
                        let same =
                            std::mem::discriminant(&choice) == std::mem::discriminant(action);
                        if ui.selectable_label(same, choice.label()).clicked() {
                            *action = choice;
                        }
                    }
                });
            match action {
                Action::OpenScreen(target) => {
                    ui.horizontal(|ui| {
                        ui.label("화면 번호");
                        ui.add(egui::TextEdit::singleline(target).desired_width(120.0));
                    });
                }
                Action::OpenLevel(target) => {
                    ui.horizontal(|ui| {
                        ui.label("레벨 번호");
                        ui.add(egui::TextEdit::singleline(target).desired_width(120.0));
                    });
                }
                _ => {}
            }
        }
    }
}

/// 오른쪽 아래 크기 조절 손잡이.
fn handle_rect(r: Rect) -> Rect {
    Rect::new(
        r.x + r.w - HANDLE_PX * 0.5,
        r.y + r.h - HANDLE_PX * 0.5,
        HANDLE_PX,
        HANDLE_PX,
    )
}

/// 사각형이 화면 안에 있는지 — 위젯을 화면 밖으로 밀어 놨는지 보는 데 쓴다.
fn inside(r: Rect, viewport: (f32, f32)) -> bool {
    r.x >= 0.0 && r.y >= 0.0 && r.x + r.w <= viewport.0 && r.y + r.h <= viewport.1
}

// ─────────────────────────────────────────────────────────────────────────────
// 트리 조작 — 이름으로 찾고, 떼어내고, 붙인다
// ─────────────────────────────────────────────────────────────────────────────

/// 이름으로 찾는다 (자식까지).
fn find<'a>(widgets: &'a [Widget], id: &str) -> Option<&'a Widget> {
    for w in widgets {
        if w.id == id {
            return Some(w);
        }
        if let Some(found) = find(&w.children, id) {
            return Some(found);
        }
    }
    None
}

fn find_mut<'a>(widgets: &'a mut [Widget], id: &str) -> Option<&'a mut Widget> {
    for w in widgets.iter_mut() {
        if w.id == id {
            return Some(w);
        }
        if let Some(found) = find_mut(&mut w.children, id) {
            return Some(found);
        }
    }
    None
}

/// 떼어낸다 — 자식까지 통째로. 삭제·부모 바꾸기가 쓴다.
fn take(widgets: &mut Vec<Widget>, id: &str) -> Option<Widget> {
    if let Some(i) = widgets.iter().position(|w| w.id == id) {
        return Some(widgets.remove(i));
    }
    for w in widgets.iter_mut() {
        if let Some(found) = take(&mut w.children, id) {
            return Some(found);
        }
    }
    None
}

/// 넣는다 — `parent` 가 `None` 이면 최상위 맨 뒤(가장 위에 그려짐)에.
fn insert_into(widgets: &mut Vec<Widget>, parent: Option<&str>, widget: Widget) -> bool {
    match parent {
        None => {
            widgets.push(widget);
            true
        }
        Some(parent) => match find_mut(widgets, parent) {
            Some(w) => {
                w.children.push(widget);
                true
            }
            None => {
                widgets.push(widget);
                true
            }
        },
    }
}

/// `(이름, 깊이, 종류)` 를 그리는 순서대로.
fn ids_with_depth(widgets: &[Widget]) -> Vec<(String, usize, &'static str)> {
    fn walk(widgets: &[Widget], depth: usize, out: &mut Vec<(String, usize, &'static str)>) {
        for w in widgets {
            out.push((w.id.clone(), depth, w.kind.label()));
            walk(&w.children, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    walk(widgets, 0, &mut out);
    out
}

/// 부모 이름 — 최상위면 `None`.
fn parent_of(widgets: &[Widget], id: &str) -> Option<String> {
    for w in widgets {
        if w.children.iter().any(|c| c.id == id) {
            return Some(w.id.clone());
        }
        if let Some(found) = parent_of(&w.children, id) {
            return Some(found);
        }
    }
    None
}

/// 부모로 고를 수 있는 위젯 — 자기 자신과 자손은 뺀다.
fn parent_choices(widgets: &[Widget], id: &str) -> Vec<String> {
    let sub = find(widgets, id);
    ids_with_depth(widgets)
        .into_iter()
        .map(|(name, _, _)| name)
        .filter(|name| name != id && sub.is_none_or(|sub| find(&sub.children, name).is_none()))
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn editor(widgets: Vec<Widget>) -> ScreenEditor {
        let screen = ScreenFile {
            version: 1,
            name: String::from("x"),
            widgets,
        };
        ScreenEditor {
            open: true,
            current: String::from("test"),
            screen: Some(screen.clone()),
            saved: Some(screen),
            scale: 2,
            saved_scale: 2,
            ..ScreenEditor::default()
        }
    }

    fn bar(id: &str) -> Widget {
        Widget {
            size: Some((110, 9)),
            ..Widget::new(
                id,
                WidgetKind::Bar {
                    source: BarSource::Hp,
                    color: (0.85, 0.25, 0.25),
                },
            )
        }
    }

    fn panel(id: &str, children: Vec<Widget>) -> Widget {
        Widget {
            children,
            ..Widget::new(id, WidgetKind::Panel { frame: true })
        }
    }

    fn tree_of(ed: &ScreenEditor) -> Vec<(String, usize)> {
        ids_with_depth(&ed.screen.as_ref().unwrap().widgets)
            .into_iter()
            .map(|(id, depth, _)| (id, depth))
            .collect()
    }

    #[test]
    fn nudging_moves_the_selected_widget_and_can_be_undone() {
        let mut ed = editor(vec![bar("hp")]);
        ed.select("hp");
        ed.nudge((4, -8));
        let w = find(&ed.screen.as_ref().unwrap().widgets, "hp").unwrap();
        assert_eq!(w.pos, (4, -8));
        assert!(ed.is_dirty(), "저장 안 됨 표시");

        let (screen, scale) = ed.history.pop().unwrap();
        ed.screen = Some(screen);
        ed.scale = scale;
        assert!(!ed.is_dirty(), "되돌리면 저장 상태와 같아진다");
    }

    #[test]
    fn selecting_an_unknown_widget_says_so() {
        let mut ed = editor(vec![bar("hp")]);
        assert!(!ed.select("없는것"));
        assert!(ed.status.as_ref().unwrap().0, "오류로 알린다");
        assert_eq!(ed.selected, None);
    }

    #[test]
    fn a_new_widget_becomes_a_child_of_the_selection() {
        let mut ed = editor(vec![panel("window", vec![])]);
        ed.select("window");
        ed.new_widget = String::from("title");
        ed.add(WidgetKind::Text {
            format: String::from("HP {hp}"),
        });
        assert_eq!(
            tree_of(&ed),
            vec![(String::from("window"), 0), (String::from("title"), 1)]
        );
        assert_eq!(ed.selected.as_deref(), Some("title"), "새 위젯이 골라진다");

        // 이름이 겹치면 늘지 않는다.
        ed.selected = None;
        ed.new_widget = String::from("title");
        ed.add(WidgetKind::Panel { frame: false });
        assert_eq!(tree_of(&ed).len(), 2);
        assert!(ed.status.as_ref().unwrap().0);
    }

    #[test]
    fn deleting_takes_the_children_with_it() {
        let mut ed = editor(vec![
            panel("window", vec![bar("hp"), panel("inner", vec![bar("exp")])]),
            bar("solo"),
        ]);
        let screen = ed.screen.as_mut().unwrap();
        take(&mut screen.widgets, "window");
        assert_eq!(tree_of(&ed), vec![(String::from("solo"), 0)]);
    }

    #[test]
    fn reparenting_refuses_a_descendant_and_can_move_to_the_top() {
        let mut ed = editor(vec![panel("window", vec![panel("inner", vec![bar("hp")])])]);
        ed.reparent("window", Some("inner"));
        assert!(ed.status.as_ref().unwrap().0, "자손을 부모로 삼을 수 없다");
        assert_eq!(tree_of(&ed).len(), 3);

        ed.reparent("hp", None);
        assert_eq!(
            tree_of(&ed),
            vec![
                (String::from("window"), 0),
                (String::from("inner"), 1),
                (String::from("hp"), 0)
            ]
        );
    }

    #[test]
    fn reordering_swaps_siblings_only() {
        let mut ed = editor(vec![bar("a"), bar("b"), bar("c")]);
        ed.reorder("a", 1);
        assert_eq!(
            tree_of(&ed)
                .iter()
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>(),
            vec!["b", "a", "c"]
        );
        // 맨 위에서 더 위로 — 아무것도 바뀌지 않고 기록도 남지 않는다.
        let before = ed.history.len();
        ed.reorder("b", -1);
        assert_eq!(ed.history.len(), before);
    }

    #[test]
    fn parent_choices_leave_out_self_and_descendants() {
        let widgets = vec![
            panel("window", vec![panel("inner", vec![bar("hp")])]),
            bar("solo"),
        ];
        assert_eq!(parent_choices(&widgets, "window"), vec!["solo"]);
        assert_eq!(
            parent_choices(&widgets, "hp"),
            vec!["window", "inner", "solo"]
        );
    }

    #[test]
    fn inside_sees_widgets_pushed_off_screen() {
        let viewport = (800.0, 600.0);
        assert!(inside(Rect::new(10.0, 10.0, 100.0, 20.0), viewport));
        assert!(!inside(Rect::new(760.0, 10.0, 100.0, 20.0), viewport));
    }

    #[test]
    fn the_resize_handle_sits_on_the_bottom_right_corner() {
        let r = Rect::new(100.0, 50.0, 200.0, 40.0);
        let h = handle_rect(r);
        assert!(h.contains(Vec2::new(300.0, 90.0)), "모서리 점");
        assert!(!h.contains(Vec2::new(100.0, 50.0)), "왼쪽 위는 아니다");
    }
}
