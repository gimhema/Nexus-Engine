//! 데이터 표 편집기 — 아이템 · 스킬 · 드롭 표를 고친다 (단계 2 P10).
//!
//! ```text
//! ┌ 데이터 표 편집기 ─────────────────────────┐
//! │ [아이템] [스킬] [드롭 표]                   │
//! │ [#501 빨간 포션 ▼]   #502 [이름] [+ 새로]   │
//! │ 이름 [빨간 포션]  색 ■                      │
//! │ 종류 (●소모품 ○장비)  회복 60  최대 겹침 20 │
//! │ [저장] [원래대로]                           │
//! └───────────────────────────────────────────┘
//! ```
//!
//! 액터 편집기와 같은 방식이다: 항목 하나를 폼으로 고치고, 저장은 **그 번호의 항목만** 갈아 끼운다
//! (`ron_patch`), 쓰기 전에 **`GameData::parse` 로 두 파일을 다시 검증**한다.
//!
//! - 아이템·스킬은 규칙 반쪽(`rules.ron`)과 표시 반쪽(`display.ron` — 이름·색)으로 나뉜다.
//!   드롭 표는 규칙에만 있다.
//! - 아이템·스킬은 **한 줄 항목**이다. 줄 끝 주석(`// 빨간 포션`)은 저장해도 살린다.
//! - 없는 아이템을 드롭 표에 넣거나, 드롭 표·스킬을 쓰는 액터가 가리키는 번호가 사라지면
//!   검증에서 막힌다 — 이유를 창에 띄우고 쓰지 않는다.

use std::path::Path;

use nexus_render_wgpu::egui;

use crate::game_data::{
    DISPLAY_PATH, GameData, ItemForm, ItemKindForm, LootRow, RULES_PATH, SkillForm, SlotChoice,
    TableForms, loot_entry, table_forms,
};

/// 편집기의 탭.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tab {
    #[default]
    Items,
    Skills,
    Loot,
}

impl Tab {
    const ALL: [Self; 3] = [Self::Items, Self::Skills, Self::Loot];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Items => "아이템",
            Self::Skills => "스킬",
            Self::Loot => "드롭 표",
        }
    }
}

/// 고치는 중인 항목.
#[derive(Clone, Debug, PartialEq)]
enum Edit {
    Item(ItemForm),
    Skill(SkillForm),
    Loot(Vec<LootRow>),
}

#[derive(Debug, Default)]
pub(crate) struct TableEditor {
    open: bool,
    tab: Tab,
    /// 디스크의 값 — "저장 안 됨" 판정과 "원래대로".
    saved: TableForms,
    current: Option<u32>,
    edit: Option<Edit>,
    /// 파일에 아직 없는 새 항목.
    is_new: bool,
    status: Option<(bool, String)>,
    new_id: u32,
    new_name: String,
    saved_now: bool,
}

impl TableEditor {
    /// 이 탭의 이 번호를 연다.
    pub(crate) fn open_entry(&mut self, tab: Tab, id: u32) {
        self.open = true;
        if self.is_dirty() && (self.tab != tab || self.current != Some(id)) {
            self.status = Some((true, self.dirty_message()));
            return;
        }
        if let Err(e) = self.reload() {
            self.status = Some((true, e));
            return;
        }
        self.tab = tab;
        self.select(id);
        // 새 번호 칸은 늘 빈 번호를 가리키게 둔다.
        if self.new_id == 0 || self.ids().iter().any(|(i, _)| *i == self.new_id) {
            self.new_id = self.next_free_id();
        }
    }

    /// "새로 만들기" 로 연다 — 빈 번호를 골라 둔다.
    pub(crate) fn open_new(&mut self, tab: Tab) {
        self.open = true;
        if self.is_dirty() {
            self.status = Some((true, self.dirty_message()));
            return;
        }
        if let Err(e) = self.reload() {
            self.status = Some((true, e));
            return;
        }
        self.tab = tab;
        self.current = None;
        self.edit = None;
        self.new_id = self.next_free_id();
        self.status = Some((false, String::from("번호를 적고 '+ 새로' 를 누르세요")));
    }

    pub(crate) fn take_saved(&mut self) -> bool {
        std::mem::take(&mut self.saved_now)
    }

    /// 이 항목을 저장하지 않은 채 고치는 중인가 — 파일 작업(삭제)이 막는다 (P9).
    pub(crate) fn blocks(&self, tab: Tab, id: u32) -> bool {
        self.tab == tab && self.current == Some(id) && self.is_dirty()
    }

    /// 항목이 지워졌다 — 열어 둔 것이 그 항목이면 닫는다.
    pub(crate) fn forget(&mut self, tab: Tab, id: u32) {
        if self.tab == tab && self.current == Some(id) {
            self.current = None;
            self.edit = None;
            self.is_new = false;
        }
        let _ = self.reload();
    }

    fn select(&mut self, id: u32) {
        let edit = match self.tab {
            Tab::Items => self.saved.items.get(&id).cloned().map(Edit::Item),
            Tab::Skills => self.saved.skills.get(&id).cloned().map(Edit::Skill),
            Tab::Loot => self.saved.loot.get(&id).cloned().map(Edit::Loot),
        };
        match edit {
            Some(edit) => {
                self.current = Some(id);
                self.edit = Some(edit);
                self.is_new = false;
                self.status = None;
            }
            None => self.status = Some((true, format!("{} #{id} 이 없습니다", self.tab.label()))),
        }
    }

    fn saved_entry(&self) -> Option<Edit> {
        let id = self.current?;
        match self.tab {
            Tab::Items => self.saved.items.get(&id).cloned().map(Edit::Item),
            Tab::Skills => self.saved.skills.get(&id).cloned().map(Edit::Skill),
            Tab::Loot => self.saved.loot.get(&id).cloned().map(Edit::Loot),
        }
    }

    fn is_dirty(&self) -> bool {
        self.edit.is_some() && (self.is_new || self.edit != self.saved_entry())
    }

    fn dirty_message(&self) -> String {
        format!(
            "{} #{} 을(를) 먼저 저장하거나 되돌리세요",
            self.tab.label(),
            self.current.unwrap_or_default()
        )
    }

    fn reload(&mut self) -> Result<(), String> {
        let (rules, display) = crate::game_data::data_texts()?;
        self.saved = table_forms(&rules, &display)?;
        Ok(())
    }

    fn ids(&self) -> Vec<(u32, String)> {
        match self.tab {
            Tab::Items => self
                .saved
                .items
                .iter()
                .map(|(id, f)| (*id, f.name.clone()))
                .collect(),
            Tab::Skills => self
                .saved
                .skills
                .iter()
                .map(|(id, f)| (*id, f.name.clone()))
                .collect(),
            Tab::Loot => self
                .saved
                .loot
                .iter()
                .map(|(id, rows)| (*id, format!("{}줄", rows.len())))
                .collect(),
        }
    }

    fn next_free_id(&self) -> u32 {
        let taken: Vec<u32> = self.ids().into_iter().map(|(id, _)| id).collect();
        let start = taken.iter().max().map_or(1, |m| m + 1);
        (start..).find(|n| !taken.contains(n)).unwrap_or(start)
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui) {
        let mut open = self.open;
        egui::Window::new("데이터 표 편집기")
            .open(&mut open)
            .default_size([360.0, 460.0])
            .resizable(true)
            .show(ui.ctx(), |ui| {
                self.tabs(ui);
                ui.separator();
                self.header(ui);
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| self.form(ui));
                ui.separator();
                self.footer(ui);
            });
        self.open = open;
    }

    fn tabs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for tab in Tab::ALL {
                if ui.selectable_label(self.tab == tab, tab.label()).clicked() && self.tab != tab {
                    if self.is_dirty() {
                        self.status = Some((true, self.dirty_message()));
                    } else {
                        self.tab = tab;
                        self.current = None;
                        self.edit = None;
                        self.new_id = self.next_free_id();
                        self.status = None;
                    }
                }
            }
        });
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        let mut pick = None;
        ui.horizontal(|ui| {
            let ids = self.ids();
            let label = self.current.map_or_else(
                || String::from("고르세요"),
                |id| {
                    let name = ids
                        .iter()
                        .find(|(i, _)| *i == id)
                        .map_or("", |(_, n)| n.as_str());
                    format!("#{id} {name}")
                },
            );
            egui::ComboBox::from_id_salt("table-pick")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    for (id, name) in &ids {
                        if ui
                            .selectable_label(self.current == Some(*id), format!("#{id} {name}"))
                            .clicked()
                        {
                            pick = Some(*id);
                        }
                    }
                });
            if self.is_dirty() {
                ui.colored_label(egui::Color32::from_rgb(240, 190, 90), "저장 안 됨");
            }
        });
        if let Some(id) = pick
            && Some(id) != self.current
        {
            if self.is_dirty() {
                self.status = Some((true, self.dirty_message()));
            } else {
                self.select(id);
            }
        }
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut self.new_id)
                    .range(1..=999_999)
                    .prefix("#"),
            );
            if self.tab != Tab::Loot {
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_name)
                        .hint_text("이름")
                        .desired_width(120.0),
                );
            }
            if ui.button("+ 새로").clicked() {
                self.create();
            }
        });
    }

    fn create(&mut self) {
        if self.is_dirty() {
            self.status = Some((true, self.dirty_message()));
            return;
        }
        let name = self.new_name.trim().to_owned();
        if self.tab != Tab::Loot && name.is_empty() {
            self.status = Some((true, String::from("이름을 적으세요")));
            return;
        }
        if self.ids().iter().any(|(id, _)| *id == self.new_id) {
            self.status = Some((true, format!("#{} 은(는) 이미 있습니다", self.new_id)));
            return;
        }
        self.current = Some(self.new_id);
        self.edit = Some(match self.tab {
            Tab::Items => Edit::Item(ItemForm::new(&name)),
            Tab::Skills => Edit::Skill(SkillForm::new(&name)),
            Tab::Loot => Edit::Loot(Vec::new()),
        });
        self.is_new = true;
        self.new_name.clear();
        self.status = Some((
            false,
            format!(
                "새 {} #{} — 저장하면 표에 들어갑니다",
                self.tab.label(),
                self.new_id
            ),
        ));
    }

    fn form(&mut self, ui: &mut egui::Ui) {
        let items: Vec<(u32, String)> = self
            .saved
            .items
            .iter()
            .map(|(id, f)| (*id, f.name.clone()))
            .collect();
        match self.edit.as_mut() {
            None => {
                ui.weak("콘텐츠 브라우저에서 두 번 누르거나 위에서 고르세요.");
            }
            Some(Edit::Item(f)) => item_form(ui, f),
            Some(Edit::Skill(f)) => skill_form(ui, f),
            Some(Edit::Loot(rows)) => loot_form(ui, rows, &items),
        }
    }

    fn footer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let dirty = self.is_dirty();
            if ui
                .add_enabled(
                    dirty,
                    egui::Button::new(if dirty { "저장 *" } else { "저장" }),
                )
                .clicked()
            {
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
                } else {
                    self.edit = self.saved_entry();
                }
                self.status = None;
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

    pub(crate) fn save(&mut self) {
        let (Some(id), Some(edit)) = (self.current, self.edit.clone()) else {
            return;
        };
        let result = std::fs::read_to_string(RULES_PATH)
            .map_err(|e| format!("{RULES_PATH}: {e}"))
            .and_then(|rules| {
                let display = std::fs::read_to_string(DISPLAY_PATH)
                    .map_err(|e| format!("{DISPLAY_PATH}: {e}"))?;
                patch(&rules, &display, id, &edit)
            })
            .and_then(|(rules, display)| {
                crate::game_data::write_tables(Path::new("."), &rules, &display)
            });
        match result {
            Ok(()) => {
                self.is_new = false;
                let _ = self.reload();
                self.saved_now = true;
                self.status = Some((false, format!("{} #{id} 저장", self.tab.label())));
            }
            Err(e) => self.status = Some((true, format!("저장하지 않았습니다 — {e}"))),
        }
    }
}

/// 항목 하나를 새 번호로 복사한 두 텍스트와 새 이름 — 콘텐츠 브라우저의 "복제" (P9 파일 작업).
///
/// 저장과 **같은 모양**으로 끼운다 (`patch`) — 그래서 검사도 같다. 이름이 있는 표(아이템·스킬)는
/// "복사본" 을 붙인다 — 목록에서 두 개가 같은 이름으로 보이지 않게.
pub(crate) fn duplicate(
    rules: &str,
    display: &str,
    tab: Tab,
    from: u32,
    to: u32,
) -> Result<(String, String, String), String> {
    let forms = table_forms(rules, display)?;
    let copy = |name: &str| {
        if name.is_empty() {
            String::new()
        } else {
            format!("{name} 복사본")
        }
    };
    let (edit, taken) = match tab {
        Tab::Items => (
            forms.items.get(&from).cloned().map(|mut f| {
                f.name = copy(&f.name);
                Edit::Item(f)
            }),
            forms.items.contains_key(&to),
        ),
        Tab::Skills => (
            forms.skills.get(&from).cloned().map(|mut f| {
                f.name = copy(&f.name);
                Edit::Skill(f)
            }),
            forms.skills.contains_key(&to),
        ),
        Tab::Loot => (
            forms.loot.get(&from).cloned().map(Edit::Loot),
            forms.loot.contains_key(&to),
        ),
    };
    if taken {
        return Err(format!("{} #{to} 은(는) 이미 있습니다", tab.label()));
    }
    let edit = edit.ok_or_else(|| format!("{} #{from} 이 없습니다", tab.label()))?;
    let name = match &edit {
        Edit::Item(f) => f.name.clone(),
        Edit::Skill(f) => f.name.clone(),
        Edit::Loot(_) => format!("드롭 표 #{to}"),
    };
    let (rules, display) = patch(rules, display, to, &edit)?;
    Ok((rules, display, name))
}

/// 두 텍스트에 항목을 끼우고 **게임이 읽는 것과 같은 검사**를 통과하는지 본다.
fn patch(rules: &str, display: &str, id: u32, edit: &Edit) -> Result<(String, String), String> {
    use crate::ron_patch::upsert_entry;
    let (rules, display) = match edit {
        Edit::Item(f) => (
            upsert_entry(rules, "items", id, &f.rules_entry(id))?,
            upsert_entry(display, "items", id, &f.display_entry(id))?,
        ),
        Edit::Skill(f) => (
            upsert_entry(rules, "skills", id, &f.rules_entry(id))?,
            upsert_entry(display, "skills", id, &f.display_entry(id))?,
        ),
        Edit::Loot(rows) => (
            upsert_entry(rules, "loot", id, &loot_entry(id, rows))?,
            display.to_owned(),
        ),
    };
    GameData::parse(&rules, &display)?;
    Ok((rules, display))
}

fn item_form(ui: &mut egui::Ui, f: &mut ItemForm) {
    egui::Grid::new("item-form").num_columns(2).show(ui, |ui| {
        ui.label("이름");
        ui.add(egui::TextEdit::singleline(&mut f.name).desired_width(180.0));
        ui.end_row();
        ui.label("색 (땅에 떨어졌을 때)");
        let mut rgb = [f.color.0, f.color.1, f.color.2];
        if ui.color_edit_button_rgb(&mut rgb).changed() {
            f.color = (rgb[0], rgb[1], rgb[2]);
        }
        ui.end_row();

        ui.label("종류");
        ui.horizontal(|ui| {
            let consumable = matches!(f.kind, ItemKindForm::Consumable { .. });
            if ui.selectable_label(consumable, "소모품").clicked() && !consumable {
                f.kind = ItemKindForm::Consumable {
                    heal: 10,
                    max_stack: 20,
                };
            }
            if ui.selectable_label(!consumable, "장비").clicked() && consumable {
                f.kind = ItemKindForm::Equipment {
                    slot: SlotChoice::Weapon,
                    attack: 1,
                    defense: 0,
                };
            }
        });
        ui.end_row();

        match &mut f.kind {
            ItemKindForm::Consumable { heal, max_stack } => {
                ui.label("회복");
                ui.add(egui::DragValue::new(heal).range(0..=1_000_000));
                ui.end_row();
                ui.label("최대 겹침");
                ui.add(egui::DragValue::new(max_stack).range(1..=9_999));
                ui.end_row();
            }
            ItemKindForm::Equipment {
                slot,
                attack,
                defense,
            } => {
                ui.label("자리");
                egui::ComboBox::from_id_salt("item-slot")
                    .selected_text(slot.label())
                    .show_ui(ui, |ui| {
                        for s in SlotChoice::ALL {
                            ui.selectable_value(slot, s, s.label());
                        }
                    });
                ui.end_row();
                ui.label("공격");
                ui.add(egui::DragValue::new(attack).range(0..=100_000));
                ui.end_row();
                ui.label("방어");
                ui.add(egui::DragValue::new(defense).range(0..=100_000));
                ui.end_row();
            }
        }
    });
    ui.weak("장비는 늘 1개씩 겹칩니다. 번호는 서버 아이템 테이블과 같은 번호 공간입니다.");
}

fn skill_form(ui: &mut egui::Ui, f: &mut SkillForm) {
    egui::Grid::new("skill-form").num_columns(2).show(ui, |ui| {
        ui.label("이름");
        ui.add(egui::TextEdit::singleline(&mut f.name).desired_width(180.0));
        ui.end_row();
        ui.label("사거리 (m)");
        ui.add(
            egui::DragValue::new(&mut f.range)
                .speed(0.1)
                .range(0.0..=100.0),
        );
        ui.end_row();
        ui.label("쿨타임 (ms)");
        ui.add(
            egui::DragValue::new(&mut f.cooldown_ms)
                .speed(10)
                .range(0..=600_000),
        );
        ui.end_row();
        ui.label("피해 배율");
        ui.add(
            egui::DragValue::new(&mut f.damage_mult)
                .speed(0.05)
                .range(0.0..=100.0),
        );
        ui.end_row();
    });
    ui.weak("피해 = max(1, 공격 × 배율 − 방어) — 서버 CombatProcessor 와 같은 식.");
}

fn loot_form(ui: &mut egui::Ui, rows: &mut Vec<LootRow>, items: &[(u32, String)]) {
    ui.weak("줄마다 따로 굴립니다. 확률은 천분율 (1000 = 반드시).");
    let mut remove = None;
    egui::Grid::new("loot-form").num_columns(4).show(ui, |ui| {
        ui.strong("아이템");
        ui.strong("개수");
        ui.strong("확률 ‰");
        ui.label("");
        ui.end_row();
        for (i, row) in rows.iter_mut().enumerate() {
            let name = items.iter().find(|(id, _)| *id == row.item).map_or_else(
                || format!("#{} (없는 아이템)", row.item),
                |(id, n)| format!("#{id} {n}"),
            );
            egui::ComboBox::from_id_salt(("loot-item", i))
                .width(150.0)
                .selected_text(name)
                .show_ui(ui, |ui| {
                    for (id, n) in items {
                        ui.selectable_value(&mut row.item, *id, format!("#{id} {n}"));
                    }
                });
            ui.add(egui::DragValue::new(&mut row.count).range(1..=9_999));
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut row.chance_per_mille).range(0..=1000));
                ui.weak(format!("{:.1}%", row.chance_per_mille as f32 / 10.0));
            });
            if ui.small_button("빼기").clicked() {
                remove = Some(i);
            }
            ui.end_row();
        }
    });
    if let Some(i) = remove {
        rows.remove(i);
    }
    if ui.button("+ 줄").clicked() {
        rows.push(LootRow {
            item: items.first().map_or(1, |(id, _)| *id),
            count: 1,
            chance_per_mille: 500,
        });
    }
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

    /// 모든 항목을 **바꾸지 않고** 다시 써도 같은 값으로 읽힌다 — 폼 ↔ 텍스트가 새지 않는다.
    #[test]
    fn every_table_entry_round_trips_through_the_form() {
        let (rules, display) = repo_texts();
        let forms = table_forms(&rules, &display).unwrap();
        assert!(!forms.items.is_empty() && !forms.skills.is_empty() && !forms.loot.is_empty());
        let (mut r, mut d) = (rules.clone(), display.clone());
        for (id, f) in &forms.items {
            (r, d) = patch(&r, &d, *id, &Edit::Item(f.clone())).unwrap();
        }
        for (id, f) in &forms.skills {
            (r, d) = patch(&r, &d, *id, &Edit::Skill(f.clone())).unwrap();
        }
        for (id, rows) in &forms.loot {
            (r, d) = patch(&r, &d, *id, &Edit::Loot(rows.clone())).unwrap();
        }
        assert_eq!(table_forms(&r, &d).unwrap(), forms);
        // 한 줄 항목의 줄 끝 주석(이름 노릇을 하던 것)이 살아 있다.
        assert!(r.contains("// 빨간 포션"), "아이템 줄 끝 주석");
        assert!(r.contains("// 플레이어 베기"), "스킬 줄 끝 주석");
    }

    #[test]
    fn a_new_item_and_skill_land_in_both_halves() {
        let (rules, display) = repo_texts();
        let mut item = ItemForm::new("파란 포션");
        item.kind = ItemKindForm::Consumable {
            heal: 30,
            max_stack: 10,
        };
        let (r, d) = patch(&rules, &display, 502, &Edit::Item(item.clone())).unwrap();
        let skill = SkillForm {
            range: 6.0,
            cooldown_ms: 2000,
            damage_mult: 1.5,
            name: String::from("화살"),
        };
        let (r, d) = patch(&r, &d, 4, &Edit::Skill(skill.clone())).unwrap();
        let forms = table_forms(&r, &d).unwrap();
        assert_eq!(forms.items[&502], item);
        assert_eq!(forms.skills[&4], skill);
        // 이름은 표시 파일에만 — 규칙 파일에 이름이 들어가지 않는다.
        assert!(!r.contains("화살") && d.contains("화살"));
    }

    #[test]
    fn a_loot_table_that_names_a_missing_item_is_refused() {
        let (rules, display) = repo_texts();
        let rows = vec![LootRow {
            item: 77_777,
            count: 1,
            chance_per_mille: 1000,
        }];
        let err = patch(&rules, &display, 1, &Edit::Loot(rows)).unwrap_err();
        assert!(err.contains("77777"), "{err}");
    }

    #[test]
    fn loot_rows_can_be_emptied_and_refilled() {
        let (rules, display) = repo_texts();
        let (r, d) = patch(&rules, &display, 1, &Edit::Loot(Vec::new())).unwrap();
        assert!(table_forms(&r, &d).unwrap().loot[&1].is_empty());
        let rows = vec![LootRow {
            item: 501,
            count: 2,
            chance_per_mille: 1000,
        }];
        let (r, d) = patch(&r, &d, 1, &Edit::Loot(rows.clone())).unwrap();
        assert_eq!(table_forms(&r, &d).unwrap().loot[&1], rows);
    }
}
