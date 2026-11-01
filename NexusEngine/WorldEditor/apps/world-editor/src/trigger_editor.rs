//! 트리거 편집기 — `rules.ron` 의 트리거를 고치고 이름을 붙인다 (T5).
//!
//! ```text
//! ┌ 트리거 편집기 ───────────────────────────────────────────┐
//! │ [#2 슬라임 사냥 보상 ▼]  [#5] [+ 새 트리거] [이 트리거 빼기] │
//! │ 이름   [슬라임 사냥 보상]                                  │
//! │ 언제   [Killed(actor: Some(100))]                          │
//! │ 만약   [All([...])]                                        │
//! │ 한다   [GiveExp(100)]                 [×]   + 효과          │
//! │ [저장 *] [원래대로]                                         │
//! └──────────────────────────────────────────────────────────┘
//! ```
//!
//! 칸은 **파일과 같은 RON 한 줄**이고 칸마다 바로 문법을 본다 — 퀘스트 디자이너의 조건·효과 칸과 같다.
//! 저장은 그 트리거 항목과 `display.ron` 의 이름만 갈아 끼우고(그 항목 안의 주석은 남지 않는다),
//! 쓰기 전에 **게임과 같은 검사**로 다시 읽는다.

use std::collections::BTreeMap;
use std::path::Path;

use nexus_render_wgpu::egui;

use crate::game_data::{
    DISPLAY_PATH, RULES_PATH, patch_trigger, remove_trigger, trigger_forms, write_tables,
};
use crate::trigger_file::{ConditionFile, EffectFile, OnFile, TriggerForm};

#[derive(Debug, Default)]
pub(crate) struct TriggerEditor {
    open: bool,
    saved: BTreeMap<u32, TriggerForm>,
    current: Option<u32>,
    edit: Option<TriggerForm>,
    is_new: bool,
    new_id: u32,
    status: Option<(bool, String)>,
    saved_now: bool,
}

impl TriggerEditor {
    pub(crate) fn open(&mut self) {
        self.open = true;
        if self.is_dirty() {
            return;
        }
        match crate::game_data::data_texts().and_then(|(r, d)| trigger_forms(&r, &d)) {
            Ok(saved) => {
                self.saved = saved;
                if self.new_id == 0 || self.saved.contains_key(&self.new_id) {
                    self.new_id = self.saved.keys().max().map_or(1, |m| m + 1);
                }
                if self.current.is_none()
                    && let Some(first) = self.saved.keys().next().copied()
                {
                    self.select(first);
                }
            }
            Err(e) => self.status = Some((true, e)),
        }
    }

    /// 이 번호를 연다 — 콘텐츠 브라우저의 두 번 누르기.
    pub(crate) fn open_entry(&mut self, id: u32) {
        self.open();
        if !self.is_dirty() {
            self.select(id);
        }
    }

    pub(crate) fn take_saved(&mut self) -> bool {
        std::mem::take(&mut self.saved_now)
    }

    fn is_dirty(&self) -> bool {
        self.edit.is_some()
            && (self.is_new
                || self.edit.as_ref() != self.current.and_then(|id| self.saved.get(&id)))
    }

    fn select(&mut self, id: u32) {
        self.current = Some(id);
        self.edit = self.saved.get(&id).cloned();
        self.is_new = false;
        self.status = None;
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        egui::Window::new("트리거 편집기")
            .id(egui::Id::new("trigger_editor"))
            .open(&mut open)
            .default_size([520.0, 400.0])
            .resizable(true)
            .show(ui.ctx(), |ui| {
                self.header(ui);
                ui.separator();
                match self.edit.as_mut() {
                    Some(form) => trigger_form(ui, form),
                    None => {
                        ui.weak("트리거를 고르거나 새로 만드세요.");
                    }
                }
                ui.separator();
                self.footer(ui);
            });
        self.open = open;
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let title = |id: u32, f: &TriggerForm| {
                if f.name.is_empty() {
                    format!("#{id}")
                } else {
                    format!("#{id} {}", f.name)
                }
            };
            let shown = match (self.current, self.is_new) {
                (Some(id), true) => format!("#{id} (새 트리거)"),
                (Some(id), false) => self
                    .saved
                    .get(&id)
                    .map_or_else(String::new, |f| title(id, f)),
                (None, _) => String::from("(없음)"),
            };
            let mut pick = None;
            egui::ComboBox::from_id_salt("trigger_pick")
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for (id, f) in &self.saved {
                        if ui
                            .selectable_label(self.current == Some(*id), title(*id, f))
                            .clicked()
                        {
                            pick = Some(*id);
                        }
                    }
                });
            if let Some(id) = pick {
                self.select(id);
            }
            ui.add(
                egui::DragValue::new(&mut self.new_id)
                    .range(1..=1_000_000)
                    .prefix("#"),
            );
            if ui.button("+ 새 트리거").clicked() {
                if self.saved.contains_key(&self.new_id) {
                    self.status = Some((
                        true,
                        format!("트리거 #{} 은(는) 이미 있습니다", self.new_id),
                    ));
                } else {
                    self.current = Some(self.new_id);
                    self.edit = Some(TriggerForm::new());
                    self.is_new = true;
                    self.status = None;
                }
            }
            if ui
                .add_enabled(
                    self.current.is_some() && !self.is_new,
                    egui::Button::new("이 트리거 빼기"),
                )
                .clicked()
            {
                self.remove();
            }
        });
    }

    fn footer(&mut self, ui: &mut egui::Ui) {
        let dirty = self.is_dirty();
        ui.horizontal(|ui| {
            let label = if dirty { "저장 *" } else { "저장" };
            if ui.add_enabled(dirty, egui::Button::new(label)).clicked() {
                self.save();
            }
            if ui
                .add_enabled(dirty, egui::Button::new("원래대로"))
                .clicked()
            {
                if self.is_new {
                    self.current = None;
                    self.edit = None;
                    self.is_new = false;
                } else if let Some(id) = self.current {
                    self.select(id);
                }
                self.status = None;
            }
        });
        if let Some((error, text)) = &self.status {
            let color = if *error {
                egui::Color32::from_rgb(240, 110, 100)
            } else {
                egui::Color32::from_rgb(140, 210, 140)
            };
            ui.colored_label(color, text);
        }
    }

    fn refresh(&mut self) {
        if let Ok(saved) = crate::game_data::data_texts().and_then(|(r, d)| trigger_forms(&r, &d)) {
            self.saved = saved;
        }
        if self.saved.contains_key(&self.new_id) {
            self.new_id = self.saved.keys().max().map_or(1, |m| m + 1);
        }
    }

    fn save(&mut self) {
        let (Some(id), Some(form)) = (self.current, self.edit.clone()) else {
            return;
        };
        match write_trigger(Path::new("."), id, &form) {
            Ok(()) => {
                self.is_new = false;
                self.refresh();
                self.saved_now = true;
                self.status = Some((
                    false,
                    format!("트리거 #{id} 저장 — 다음 플레이(F5)부터 적용"),
                ));
            }
            Err(e) => self.status = Some((true, format!("저장하지 않았습니다 — {e}"))),
        }
    }

    fn remove(&mut self) {
        let Some(id) = self.current else { return };
        let read = |p: &str| std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"));
        let result = read(RULES_PATH)
            .and_then(|r| Ok((r, read(DISPLAY_PATH)?)))
            .and_then(|(r, d)| remove_trigger(&r, &d, id))
            .and_then(|(r, d)| write_tables(Path::new("."), &r, &d));
        match result {
            Ok(()) => {
                self.current = None;
                self.edit = None;
                self.refresh();
                self.saved_now = true;
                self.status = Some((false, format!("트리거 #{id} 를 뺐습니다")));
            }
            Err(e) => self.status = Some((true, format!("빼지 않았습니다 — {e}"))),
        }
    }
}

/// `root` 아래 두 파일에 트리거 하나를 쓴다 — 검사를 통과해야만.
fn write_trigger(root: &Path, id: u32, form: &TriggerForm) -> Result<(), String> {
    let read = |p: &str| std::fs::read_to_string(root.join(p)).map_err(|e| format!("{p}: {e}"));
    let (rules, display) = patch_trigger(&read(RULES_PATH)?, &read(DISPLAY_PATH)?, id, form)?;
    write_tables(root, &rules, &display)
}

/// RON 한 줄 칸 — 읽히지 않으면 칸 아래에 이유를 보인다.
fn line<T: serde::de::DeserializeOwned>(ui: &mut egui::Ui, label: &str, text: &mut String) {
    ui.label(label);
    ui.add(egui::TextEdit::singleline(text).desired_width(400.0));
    ui.end_row();
    if let Err(e) = ron::from_str::<T>(text) {
        ui.label("");
        ui.colored_label(egui::Color32::from_rgb(240, 110, 100), e.to_string());
        ui.end_row();
    }
}

fn trigger_form(ui: &mut egui::Ui, f: &mut TriggerForm) {
    let mut remove = None;
    egui::Grid::new("trigger-form")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("이름");
            ui.add(egui::TextEdit::singleline(&mut f.name).desired_width(240.0));
            ui.end_row();
            line::<OnFile>(ui, "언제", &mut f.on);
            line::<ConditionFile>(ui, "만약", &mut f.when);
            for (i, effect) in f.then.iter_mut().enumerate() {
                line::<EffectFile>(ui, "한다", effect);
                ui.label("");
                if ui.small_button("× 이 효과 빼기").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
    if let Some(i) = remove {
        f.then.remove(i);
    }
    if ui.button("+ 효과").clicked() {
        f.then.push(String::from("GiveExp(10)"));
    }
    ui.weak("언제: Killed(actor: Some(100)) · LevelReached(3) · PickedUp(item: None) · Entered(center: (0.0, 0.0), radius: 3.0)");
    ui.weak("한 번만 일어나게 하려면 플래그로 막는다: Not(Flag(…)) + SetFlag(…)");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_data::GameData;

    const EMBEDDED_RULES_TEXT: &str = include_str!("../../../data/rules.ron");
    const EMBEDDED_DISPLAY_TEXT: &str = include_str!("../../../data/display.ron");

    /// 싣는 트리거는 전부 폼을 거쳐 다시 써도 게임이 같은 내용으로 읽는다.
    #[test]
    fn every_trigger_round_trips_through_the_form() {
        let (r, d) = (EMBEDDED_RULES_TEXT, EMBEDDED_DISPLAY_TEXT);
        let forms = trigger_forms(r, d).unwrap();
        assert!(!forms.is_empty());
        let before = GameData::parse(r, d).unwrap();
        let (mut rules, mut display) = (r.to_owned(), d.to_owned());
        for (id, form) in &forms {
            (rules, display) = patch_trigger(&rules, &display, *id, form).unwrap();
        }
        let after = GameData::parse(&rules, &display).unwrap();
        assert_eq!(before.triggers(), after.triggers());
    }

    #[test]
    fn a_name_lands_in_display_and_bad_entries_are_refused() {
        let (r, d) = (EMBEDDED_RULES_TEXT, EMBEDDED_DISPLAY_TEXT);
        let mut form = TriggerForm::new();
        form.name = String::from("첫 레벨업");
        form.on = String::from("LevelReached(2)");
        form.then = vec![String::from("GiveExp(5)")];
        let (rules, display) = patch_trigger(r, d, 77, &form).unwrap();
        assert_eq!(
            trigger_forms(&rules, &display).unwrap()[&77].name,
            "첫 레벨업"
        );
        let (rules, display) = remove_trigger(&rules, &display, 77).unwrap();
        assert!(!trigger_forms(&rules, &display).unwrap().contains_key(&77));

        form.then = vec![String::from("GiveItem(item: 99999, count: 1)")];
        let e = patch_trigger(r, d, 77, &form).unwrap_err();
        assert!(e.contains("없는 아이템 99999"), "{e}");
        form.then = vec![String::from("문법 오류(")];
        assert!(patch_trigger(r, d, 77, &form).is_err());
    }
}
