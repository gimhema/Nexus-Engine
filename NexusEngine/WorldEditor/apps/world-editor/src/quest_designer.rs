//! 퀘스트 디자이너 — 퀘스트의 단계·목표·보상을 고친다 (R5).
//!
//! ```text
//! ┌ 퀘스트 디자이너 ─────────────────────────────────────────┐
//! │ [#1 슬라임 소탕 ▼]  [#4] [+ 새 퀘스트] [이 퀘스트 빼기]    │
//! │ 제목 [슬라임 소탕]   설명 [마을 길목의 …]                  │
//! │ 받을 조건 [Quest(quest: 1, state: Completed)]  ☐ 반복 ☐ 자동 완료 │
//! │ ── 단계 0 ──                                              │
//! │  [처치 ▼] 액터 [#100 슬라임 ▼] × [3]          [×]          │
//! │  + 목표   효과: [SetFlag(…)] [×]  + 효과                    │
//! │ ── 보상 ──  [GiveCurrency(currency: 1, amount: 100)] [×]   │
//! │ [저장 *] [원래대로]                                          │
//! └──────────────────────────────────────────────────────────┘
//! ```
//!
//! - 목표는 종류별 칸으로, **조건·효과는 트리거와 같은 RON 한 줄**로 고친다 — 칸마다 바로 문법을 본다.
//! - 저장은 `rules.ron` 의 그 퀘스트 항목과 `display.ron` 의 제목·설명만 갈아 끼우고(그 항목 안의 주석은 남지 않는다),
//!   쓰기 전에 **게임과 같은 검사**로 다시 읽는다 — 없는 액터·아이템·퀘스트를 가리키면 쓰지 않는다.
//! - 퀘스트를 주는 NPC 는 액터 편집기의 "NPC 기능 → 퀘스트" 로 붙인다.

use std::collections::BTreeMap;
use std::path::Path;

use nexus_render_wgpu::egui;

use crate::game_data::{
    DISPLAY_PATH, RULES_PATH, actor_forms, patch_quest, quest_forms, remove_quest, table_forms,
    write_tables,
};
use crate::quest_file::{ObjectiveForm, QuestForm, StageForm};
use crate::trigger_file::{ConditionFile, EffectFile};

#[derive(Debug, Default)]
pub(crate) struct QuestDesigner {
    open: bool,
    saved: BTreeMap<u32, QuestForm>,
    current: Option<u32>,
    edit: Option<QuestForm>,
    is_new: bool,
    new_id: u32,
    /// 고를 수 있는 것들 `(번호, 이름)`.
    actors: Vec<(u32, String)>,
    items: Vec<(u32, String)>,
    status: Option<(bool, String)>,
    saved_now: bool,
}

impl QuestDesigner {
    pub(crate) fn open(&mut self) {
        self.open = true;
        if self.is_dirty() {
            return;
        }
        if let Err(e) = self.reload() {
            self.status = Some((true, e));
            return;
        }
        if self.current.is_none()
            && let Some(first) = self.saved.keys().next().copied()
        {
            self.select(first);
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

    fn reload(&mut self) -> Result<(), String> {
        let (rules, display) = crate::game_data::data_texts()?;
        self.saved = quest_forms(&rules, &display)?;
        self.actors = actor_forms(&rules, &display)?
            .into_iter()
            .map(|(id, a)| (id, a.name))
            .collect();
        self.items = table_forms(&rules, &display)?
            .items
            .into_iter()
            .map(|(id, f)| (id, f.name))
            .collect();
        if self.new_id == 0 || self.saved.contains_key(&self.new_id) {
            self.new_id = self.saved.keys().max().map_or(1, |m| m + 1);
        }
        Ok(())
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
        egui::Window::new("퀘스트 디자이너")
            .id(egui::Id::new("quest_designer"))
            .open(&mut open)
            .default_size([520.0, 560.0])
            .resizable(true)
            .show(ui.ctx(), |ui| {
                self.header(ui);
                ui.separator();
                let height = ui.available_height() - 60.0;
                egui::ScrollArea::vertical()
                    .max_height(height)
                    .show(ui, |ui| {
                        let (actors, items) = (&self.actors, &self.items);
                        match self.edit.as_mut() {
                            Some(form) => quest_form(ui, form, actors, items),
                            None => {
                                ui.weak("퀘스트를 고르거나 새로 만드세요.");
                            }
                        }
                    });
                ui.separator();
                self.footer(ui);
            });
        self.open = open;
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        let mut pick = None;
        ui.horizontal(|ui| {
            let label = self.current.map_or_else(
                || String::from("고르세요"),
                |id| {
                    let title = self.saved.get(&id).map_or("(새)", |f| f.title.as_str());
                    format!("#{id} {title}")
                },
            );
            egui::ComboBox::from_id_salt("quest-pick")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    for (id, f) in &self.saved {
                        if ui
                            .selectable_label(
                                self.current == Some(*id),
                                format!("#{id} {}", f.title),
                            )
                            .clicked()
                        {
                            pick = Some(*id);
                        }
                    }
                });
            ui.add(
                egui::DragValue::new(&mut self.new_id)
                    .range(1..=999_999)
                    .prefix("#"),
            );
            if ui.button("+ 새 퀘스트").clicked() {
                if self.is_dirty() {
                    self.status = Some((true, String::from("먼저 저장하거나 되돌리세요")));
                } else if self.saved.contains_key(&self.new_id) {
                    self.status = Some((true, format!("#{} 은(는) 이미 있습니다", self.new_id)));
                } else {
                    self.current = Some(self.new_id);
                    self.edit = Some(QuestForm::new("새 퀘스트"));
                    self.is_new = true;
                    self.status = Some((false, String::from("저장하면 rules.ron 에 들어갑니다")));
                }
            }
            if self.current.is_some() && !self.is_new && ui.button("이 퀘스트 빼기").clicked()
            {
                self.remove();
            }
        });
        if let Some(id) = pick
            && Some(id) != self.current
        {
            if self.is_dirty() {
                self.status = Some((true, String::from("먼저 저장하거나 되돌리세요")));
            } else {
                self.select(id);
            }
        }
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

    fn save(&mut self) {
        let (Some(id), Some(form)) = (self.current, self.edit.clone()) else {
            return;
        };
        match write_quest(Path::new("."), id, &form) {
            Ok(()) => {
                self.is_new = false;
                let _ = self.reload();
                self.saved_now = true;
                self.status = Some((
                    false,
                    format!("퀘스트 #{id} 저장 — 다음 플레이(F5)부터 적용"),
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
            .and_then(|(r, d)| remove_quest(&r, &d, id))
            .and_then(|(r, d)| write_tables(Path::new("."), &r, &d));
        match result {
            Ok(()) => {
                self.current = None;
                self.edit = None;
                let _ = self.reload();
                self.saved_now = true;
                self.status = Some((false, format!("퀘스트 #{id} 를 뺐습니다")));
            }
            Err(e) => self.status = Some((true, format!("빼지 않았습니다 — {e}"))),
        }
    }
}

/// `root` 아래 두 파일에 퀘스트 하나를 쓴다 — 검사를 통과해야만.
fn write_quest(root: &Path, id: u32, form: &QuestForm) -> Result<(), String> {
    let read = |p: &str| std::fs::read_to_string(root.join(p)).map_err(|e| format!("{p}: {e}"));
    let (rules, display) = patch_quest(&read(RULES_PATH)?, &read(DISPLAY_PATH)?, id, form)?;
    write_tables(root, &rules, &display)
}

/// RON 한 줄 칸 — 고치는 즉시 문법을 보고 오류를 아래에 띄운다.
fn ron_field<T: serde::de::DeserializeOwned>(ui: &mut egui::Ui, text: &mut String, hint: &str) {
    ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .desired_width(300.0)
            .font(egui::TextStyle::Monospace),
    );
    if !text.trim().is_empty()
        && let Err(e) = ron::from_str::<T>(text.trim())
    {
        ui.colored_label(egui::Color32::from_rgb(240, 110, 100), format!("⚠ {e}"));
    }
}

fn pick(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    value: &mut u32,
    list: &[(u32, String)],
    none: Option<&str>,
) {
    let name = if *value == 0 && none.is_some() {
        none.unwrap_or_default().to_owned()
    } else {
        list.iter().find(|(id, _)| id == value).map_or_else(
            || format!("#{value} (없음)"),
            |(id, n)| format!("#{id} {n}"),
        )
    };
    egui::ComboBox::from_id_salt(salt)
        .selected_text(name)
        .width(140.0)
        .show_ui(ui, |ui| {
            if let Some(label) = none {
                ui.selectable_value(value, 0, label);
            }
            for (id, n) in list {
                ui.selectable_value(value, *id, format!("#{id} {n}"));
            }
        });
}

/// 효과 목록 — 한 줄에 하나.
fn effects(ui: &mut egui::Ui, list: &mut Vec<String>, add: &str) {
    let mut remove = None;
    for (i, e) in list.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ron_field::<EffectFile>(ui, e, "GiveCurrency(currency: 1, amount: 100)");
            if ui.small_button("×").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        list.remove(i);
    }
    if ui.small_button(add).clicked() {
        list.push(String::new());
    }
}

fn quest_form(
    ui: &mut egui::Ui,
    f: &mut QuestForm,
    actors: &[(u32, String)],
    items: &[(u32, String)],
) {
    egui::Grid::new("quest-head").num_columns(2).show(ui, |ui| {
        ui.label("제목");
        ui.add(egui::TextEdit::singleline(&mut f.title).desired_width(220.0));
        ui.end_row();
        ui.label("설명");
        ui.add(
            egui::TextEdit::multiline(&mut f.description)
                .desired_rows(2)
                .desired_width(300.0),
        );
        ui.end_row();
        ui.label("받을 조건");
        ui.vertical(|ui| {
            ron_field::<ConditionFile>(
                ui,
                &mut f.requires,
                "비우면 늘 — 예: Quest(quest: 1, state: Completed)",
            );
        });
        ui.end_row();
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut f.repeatable, "반복 가능");
        ui.checkbox(&mut f.auto_complete, "자동 완료")
            .on_hover_text("마지막 단계를 채우면 NPC 에게 보고하지 않고 끝난다 — 트리거 StartQuest 로 시작하는 월드 퀘스트");
    });

    let mut remove_stage = None;
    for (s, stage) in f.stages.iter_mut().enumerate() {
        ui.separator();
        ui.horizontal(|ui| {
            ui.strong(format!("단계 {s}"));
            if ui.small_button("이 단계 빼기").clicked() {
                remove_stage = Some(s);
            }
        });
        stage_form(ui, s, stage, actors, items);
    }
    if let Some(s) = remove_stage {
        f.stages.remove(s);
    }
    if ui.button("+ 단계").clicked() {
        f.stages.push(StageForm {
            objectives: vec![ObjectiveForm::Kill { actor: 0, count: 1 }],
            then: Vec::new(),
        });
    }
    ui.separator();
    ui.strong("보상");
    effects(ui, &mut f.rewards, "+ 보상");
    ui.weak("효과는 트리거의 then 과 같은 형식 — GiveItem · GiveCurrency · GiveExp · GiveLoot(드롭 표) · GrantSkill · SetFlag · StartQuest …");
}

fn stage_form(
    ui: &mut egui::Ui,
    s: usize,
    stage: &mut StageForm,
    actors: &[(u32, String)],
    items: &[(u32, String)],
) {
    let mut remove = None;
    for (i, o) in stage.objectives.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt(("quest-kind", s, i))
                .selected_text(o.label())
                .width(60.0)
                .show_ui(ui, |ui| {
                    for (label, kind) in ObjectiveForm::KINDS {
                        if ui.selectable_label(o.label() == label, label).clicked()
                            && o.label() != label
                        {
                            *o = kind;
                        }
                    }
                });
            match o {
                ObjectiveForm::Kill { actor, count } => {
                    pick(ui, ("quest-kill", s, i), actor, actors, Some("아무나"));
                    ui.label("×");
                    ui.add(egui::DragValue::new(count).range(1..=9_999));
                }
                ObjectiveForm::Collect { item, count, take } => {
                    pick(ui, ("quest-item", s, i), item, items, None);
                    ui.label("×");
                    ui.add(egui::DragValue::new(count).range(1..=9_999));
                    ui.checkbox(take, "보고할 때 가져감");
                }
                ObjectiveForm::Reach { center, radius } => {
                    ui.add(egui::DragValue::new(&mut center.0).speed(0.5).prefix("x "));
                    ui.add(egui::DragValue::new(&mut center.1).speed(0.5).prefix("y "));
                    ui.add(
                        egui::DragValue::new(radius)
                            .speed(0.1)
                            .range(0.1..=100.0)
                            .prefix("반경 "),
                    );
                }
                ObjectiveForm::Talk { actor } => {
                    pick(ui, ("quest-talk", s, i), actor, actors, None);
                }
                ObjectiveForm::Check(text) => {
                    ui.vertical(|ui| {
                        ron_field::<ConditionFile>(ui, text, "Level(cmp: Ge, value: 5)");
                    });
                }
            }
            if ui.small_button("×").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        stage.objectives.remove(i);
    }
    if ui.small_button("+ 목표").clicked() {
        stage
            .objectives
            .push(ObjectiveForm::Kill { actor: 0, count: 1 });
    }
    ui.label("이 단계를 채우면");
    effects(ui, &mut stage.then, "+ 효과");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_texts() -> (String, String) {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        (
            std::fs::read_to_string(root.join(RULES_PATH)).unwrap(),
            std::fs::read_to_string(root.join(DISPLAY_PATH)).unwrap(),
        )
    }

    #[test]
    fn every_quest_round_trips_through_the_form() {
        let (mut rules, mut display) = repo_texts();
        let forms = quest_forms(&rules, &display).unwrap();
        assert_eq!(forms[&1].title, "슬라임 소탕");
        assert_eq!(forms[&2].requires, "Quest(quest:1,state:Completed)");
        for (id, f) in &forms {
            (rules, display) = patch_quest(&rules, &display, *id, f).unwrap();
        }
        assert_eq!(quest_forms(&rules, &display).unwrap(), forms);
        // 블록 밖의 주석은 남는다.
        assert!(rules.contains("// 상인 — 마을 근처의 슬라임을 줄여 달라."));
    }

    #[test]
    fn a_broken_effect_or_dangling_number_is_refused() {
        let (rules, display) = repo_texts();
        let mut form = QuestForm::new("시험");
        form.rewards.push(String::from("GiveCurrency(currency: 1,"));
        let err = patch_quest(&rules, &display, 50, &form).unwrap_err();
        assert!(
            err.starts_with("보상 \"GiveCurrency(currency: 1,\""),
            "{err}"
        );
        form.rewards = vec![String::from("GiveItem(item: 4242, count: 1)")];
        let err = patch_quest(&rules, &display, 50, &form).unwrap_err();
        assert!(err.contains("퀘스트 50: then: 없는 아이템 4242"), "{err}");
        // 상인이 가리키는 퀘스트 1 은 뺄 수 없다.
        let err = remove_quest(&rules, &display, 1).unwrap_err();
        assert!(err.contains("없는 퀘스트 1"), "{err}");
    }
}
