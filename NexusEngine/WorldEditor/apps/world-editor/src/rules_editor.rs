//! 게임 규칙 설정 — `rules.ron` 의 **한 벌짜리 값**을 고친다 (단계 2, P10 에서 남긴 것).
//!
//! ```text
//! ┌ 게임 규칙 설정 ──────────────────────────────┐
//! │ 난수 시드 [1]    플레이어 공격 [베기 #1 ▼]      │
//! │ ── 마커 종류별 기본 액터 ──                     │
//! │ 플레이어 스폰 [#1 플레이어 ▼]  NPC [...] …      │
//! │ ── 진영 관계 (대칭) ──                          │
//! │ [1] ↔ [201] [적대 ▼] [×]      [+ 관계]          │
//! │ ── 시작 소지품 ──                               │
//! │ [빨간 포션 #501 ▼] × [3] [×]  [+ 아이템]        │
//! │ [저장 *] [원래대로]                              │
//! └───────────────────────────────────────────────┘
//! ```
//!
//! 번호 표(액터·아이템·스킬·드롭 표)는 항목마다 편집기가 따로 있고, 여기는 **파일에 하나씩만 있는 값**을
//! 모은 곳이다 — 언리얼의 Project Settings 자리. 콘텐츠 브라우저에서 `data/rules.ron` 을 두 번 누르면 열린다.
//!
//! 저장은 다른 표 편집기와 같다: 그 줄·그 블록만 갈아 끼우고(`ron_patch`), 쓰기 전에
//! `GameData::parse` 로 두 파일을 다시 검증한다. ⚠ `relations`·`starting_kit`·`default_actors`
//! 블록 **안**의 주석은 남지 않는다.
//!
//! 진영 번호에는 이름 표가 없다 — 서버 `EFactionId` 와 같은 번호 공간이라 번호로만 적고,
//! 옆에 **그 진영을 쓰는 액터**를 보여 줘 무엇인지 알 수 있게 한다.

use std::collections::BTreeMap;
use std::path::Path;

use nexus_render_wgpu::egui;

use crate::game_data::{
    MARKER_KINDS, RelationChoice, RelationRow, SettingsForm, actor_forms, data_texts,
    patch_settings, settings_form, table_forms, write_tables,
};

#[derive(Debug, Default)]
pub(crate) struct RulesEditor {
    open: bool,
    /// 디스크의 값 — "저장 안 됨" 판정과 "원래대로".
    saved: Option<SettingsForm>,
    edit: Option<SettingsForm>,
    /// 고를 수 있는 것들 `(번호, 이름)`.
    actors: Vec<(u32, String)>,
    items: Vec<(u32, String)>,
    skills: Vec<(u32, String)>,
    /// 진영 번호 → 그 진영을 쓰는 액터 이름들.
    factions: BTreeMap<u32, Vec<String>>,
    status: Option<(bool, String)>,
    saved_now: bool,
}

impl RulesEditor {
    /// 창을 연다. 저장하지 않은 변경이 있으면 그대로 둔다 (다시 읽지 않는다).
    pub(crate) fn open(&mut self) {
        self.open = true;
        if self.is_dirty() {
            return;
        }
        if let Err(e) = self.reload() {
            self.status = Some((true, e));
        }
    }

    pub(crate) fn take_saved(&mut self) -> bool {
        std::mem::take(&mut self.saved_now)
    }

    fn is_dirty(&self) -> bool {
        self.edit.is_some() && self.edit != self.saved
    }

    fn reload(&mut self) -> Result<(), String> {
        let (rules, display) = data_texts()?;
        let form = settings_form(&rules)?;
        let actors = actor_forms(&rules, &display)?;
        let tables = table_forms(&rules, &display)?;
        self.actors = actors.iter().map(|(id, a)| (*id, a.name.clone())).collect();
        self.factions.clear();
        for a in actors.values() {
            self.factions
                .entry(a.faction)
                .or_default()
                .push(a.name.clone());
        }
        self.items = tables
            .items
            .iter()
            .map(|(id, i)| (*id, i.name.clone()))
            .collect();
        self.skills = tables
            .skills
            .iter()
            .map(|(id, s)| (*id, s.name.clone()))
            .collect();
        self.saved = Some(form.clone());
        self.edit = Some(form);
        Ok(())
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        egui::Window::new("게임 규칙 설정")
            .id(egui::Id::new("rules_editor"))
            .open(&mut open)
            .default_size([420.0, 520.0])
            .resizable(true)
            .show(ui.ctx(), |ui| {
                egui::ScrollArea::vertical()
                    .max_height(ui.available_height() - 60.0)
                    .show(ui, |ui| self.form(ui));
                ui.separator();
                self.footer(ui);
            });
        self.open = open;
    }

    fn form(&mut self, ui: &mut egui::Ui) {
        let Some(f) = self.edit.as_mut() else {
            ui.weak("규칙 파일을 읽지 못했습니다.");
            return;
        };
        let (actors, items, skills, factions) =
            (&self.actors, &self.items, &self.skills, &self.factions);

        egui::Grid::new("rules-top").num_columns(2).show(ui, |ui| {
            ui.label("난수 시드");
            ui.add(egui::DragValue::new(&mut f.seed));
            ui.end_row();
            ui.label("플레이어 공격");
            pick(ui, "rules-attack", &mut f.player_attack, skills, "스킬");
            ui.end_row();
        });
        ui.weak("시드가 같으면 드롭·스크립트 난수가 같게 나온다 (재현용).");

        ui.add_space(6.0);
        ui.strong("마커 종류별 기본 액터");
        egui::Grid::new("rules-defaults")
            .num_columns(2)
            .show(ui, |ui| {
                for (slot, (_, label)) in f.default_actors.iter_mut().zip(MARKER_KINDS) {
                    ui.label(label);
                    pick(ui, label, slot, actors, "액터");
                    ui.end_row();
                }
            });
        ui.weak("마커가 액터 타입을 정하지 않았을 때 쓴다 (예전 존 파일 포함).");

        ui.add_space(6.0);
        ui.strong("진영 관계");
        ui.weak("대칭이다. 적지 않은 쌍은 중립, 같은 진영은 늘 우호.");
        let mut remove = None;
        for (i, r) in f.relations.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                faction(ui, &mut r.a, factions);
                ui.label("↔");
                faction(ui, &mut r.b, factions);
                egui::ComboBox::from_id_salt(("rules-rel", i))
                    .width(70.0)
                    .selected_text(r.relation.label())
                    .show_ui(ui, |ui| {
                        for c in RelationChoice::ALL {
                            ui.selectable_value(&mut r.relation, c, c.label());
                        }
                    });
                if ui.small_button("×").on_hover_text("이 줄 빼기").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            f.relations.remove(i);
        }
        if ui.button("+ 관계").clicked() {
            f.relations.push(RelationRow::default());
        }

        ui.add_space(6.0);
        ui.strong("플레이어 시작 소지품");
        let mut remove = None;
        for (i, (item, count)) in f.starting_kit.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                pick(ui, ("rules-kit", i), item, items, "아이템");
                ui.label("×");
                ui.add(egui::DragValue::new(count).range(1..=999));
                if ui.small_button("×").on_hover_text("이 줄 빼기").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            f.starting_kit.remove(i);
        }
        if ui.button("+ 아이템").clicked() {
            let first = items.first().map_or(0, |(id, _)| *id);
            f.starting_kit.push((first, 1));
        }
        ui.weak("저장 파일로 이어 할 때는 쓰지 않는다 — 새로 시작할 때만.");
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
                self.edit = self.saved.clone();
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

    /// 저장 — 검증을 통과해야만 쓴다.
    pub(crate) fn save(&mut self) {
        let Some(form) = self.edit.clone() else {
            return;
        };
        match save_settings(Path::new("."), &form) {
            Ok(()) => {
                self.saved = Some(form);
                self.saved_now = true;
                self.status = Some((
                    false,
                    String::from("저장했습니다 — 다음 플레이(F5)부터 적용"),
                ));
            }
            Err(e) => self.status = Some((true, format!("저장하지 않았습니다 — {e}"))),
        }
    }
}

/// `root` 아래의 규칙 파일에 설정을 쓴다 (표시 파일은 그대로).
fn save_settings(root: &Path, form: &SettingsForm) -> Result<(), String> {
    let read = |p: &str| std::fs::read_to_string(root.join(p)).map_err(|e| format!("{p}: {e}"));
    let rules = read(crate::game_data::RULES_PATH)?;
    let display = read(crate::game_data::DISPLAY_PATH)?;
    let text = patch_settings(&rules, &display, form)?;
    write_tables(root, &text, &display)
}

/// 번호 고르기 — `(번호, 이름)` 목록에서. 목록에 없는 번호면 "(없음 #번호)" 로 보인다.
fn pick(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    value: &mut u32,
    choices: &[(u32, String)],
    what: &str,
) {
    let name = |id: u32| {
        choices.iter().find(|(i, _)| *i == id).map_or_else(
            || format!("(없는 {what} #{id})"),
            |(_, n)| format!("{n} #{id}"),
        )
    };
    egui::ComboBox::from_id_salt(salt)
        .width(180.0)
        .selected_text(name(*value))
        .show_ui(ui, |ui| {
            for (id, _) in choices {
                ui.selectable_value(value, *id, name(*id));
            }
        });
}

/// 진영 번호 칸 — 옆에 그 진영을 쓰는 액터를 알려 준다 (이름 표가 없으므로).
fn faction(ui: &mut egui::Ui, value: &mut u32, factions: &BTreeMap<u32, Vec<String>>) {
    let hint = factions
        .get(value)
        .map_or_else(|| String::from("쓰는 액터 없음"), |who| who.join(", "));
    ui.add(egui::DragValue::new(value).range(0..=99_999))
        .on_hover_text(hint);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_data::{DISPLAY_PATH, RULES_PATH};

    fn repo_texts() -> (String, String) {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        (
            std::fs::read_to_string(root.join(RULES_PATH)).unwrap(),
            std::fs::read_to_string(root.join(DISPLAY_PATH)).unwrap(),
        )
    }

    #[test]
    fn unchanged_settings_write_the_same_values_back() {
        let (rules, display) = repo_texts();
        let form = settings_form(&rules).unwrap();
        let text = patch_settings(&rules, &display, &form).unwrap();
        assert_eq!(settings_form(&text).unwrap(), form);
        // 블록 밖의 주석은 남는다.
        assert!(text.contains("// 진영 관계 (대칭)"), "{text}");
        assert!(text.contains("// 플레이어 시작 소지품."), "{text}");
    }

    #[test]
    fn edited_settings_land_in_the_file() {
        let (rules, display) = repo_texts();
        let mut form = settings_form(&rules).unwrap();
        form.seed = 42;
        form.relations.push(RelationRow {
            a: 100,
            b: 201,
            relation: RelationChoice::Hostile,
        });
        form.starting_kit = vec![(909, 5)];
        form.default_actors[2] = 101;
        let text = patch_settings(&rules, &display, &form).unwrap();
        let back = settings_form(&text).unwrap();
        assert_eq!(back, form);
        assert!(text.contains("seed: 42,"));
        assert!(text.contains("(a: 100, b: 201, relation: Hostile),"));
        assert!(text.contains("Monster: 101,"));
        // 비우면 한 줄 빈 블록.
        form.starting_kit.clear();
        form.relations.clear();
        let text = patch_settings(&rules, &display, &form).unwrap();
        assert!(text.contains("starting_kit: [],") && text.contains("relations: [],"));
    }

    #[test]
    fn settings_that_name_missing_things_are_refused() {
        let (rules, display) = repo_texts();
        let base = settings_form(&rules).unwrap();
        let mut bad = base.clone();
        bad.player_attack = 99_999;
        assert!(patch_settings(&rules, &display, &bad).is_err(), "없는 스킬");
        let mut bad = base.clone();
        bad.starting_kit.push((77_777, 1));
        assert!(
            patch_settings(&rules, &display, &bad).is_err(),
            "없는 아이템"
        );
        let mut bad = base;
        bad.default_actors[0] = 88_888;
        assert!(patch_settings(&rules, &display, &bad).is_err(), "없는 액터");
    }
}
