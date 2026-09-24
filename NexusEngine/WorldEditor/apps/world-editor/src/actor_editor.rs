//! 액터 편집기 — 액터 타입 하나의 수치·이름·그림을 한 폼에서 고친다 (단계 2 P8).
//!
//! ```text
//! ┌ 액터 편집기 ─────────────────────────────┐
//! │ [#102 고블린 ▼]  [+ 새 액터]               │
//! │ ── 표시 (display.ron) ──                   │
//! │ 이름 [고블린]  시트 [player.sheet ▼]  색 ■  │
//! │ ── 규칙 (rules.ron) ──                     │
//! │ 이동 속도 · HP · 공격 · 방어 · 불사 · 진영 │
//! │ AI [공격 ▼] · 어그로 범위 · 귀환 거리       │
//! │ 기본 공격 [x] 2 · 드롭 표 [ ] · 스크립트 ▼ │
//! │ 경험치 보상 · 성장 HP/공격/방어             │
//! │ [저장 *] [원래대로]                          │
//! └──────────────────────────────────────────┘
//! ```
//!
//! - 액터는 **표의 항목**이다 (콘텐츠 브라우저의 가상 항목). 수치는 `rules.ron`, 이름·시트·색은
//!   `display.ron` — 서버 테이블과 짝을 맞춘 반쪽 구조 그대로 두 파일에 나눠 쓴다.
//! - 저장은 **그 번호의 항목만 갈아 끼운다** (`ron_patch`) — 다른 항목의 주석은 남는다.
//!   **고친 항목 안의 주석은 남지 않는다** (그 줄들은 다시 쓰이므로).
//! - 두 파일을 고친 결과를 **`GameData::parse` 로 다시 검증한 뒤에만** 쓴다 — 없는 스킬·드롭 표를
//!   가리키면 쓰지 않고 이유를 알린다. 두 파일 모두 임시 파일에 먼저 쓴 뒤 이름을 바꾼다.
//! - 저장하면 에디터가 게임 데이터를 다시 읽는다 — 인스펙터·플레이가 바로 새 값을 본다.
//!   **시트(그림)를 바꾼 것은 다시 시작해야 보인다** (텍스처 해제 API 가 없다).

use std::collections::BTreeMap;
use std::path::Path;

use nexus_render_wgpu::egui;

use crate::game_data::{ActorForm, AiChoice, DISPLAY_PATH, GameData, RULES_PATH};

/// 편집 중인 상태.
#[derive(Debug, Default)]
pub(crate) struct ActorEditor {
    open: bool,
    /// 디스크에서 읽은 값 — "저장 안 됨" 판정과 "원래대로".
    saved: BTreeMap<u32, ActorForm>,
    current: Option<u32>,
    /// 고치는 중인 폼.
    edit: Option<ActorForm>,
    /// 파일에 아직 없는 새 액터.
    is_new: bool,
    status: Option<(bool, String)>,
    new_id: u32,
    new_name: String,
    /// 고를 수 있는 스크립트(`data/` 기준)·시트 경로.
    scripts: Vec<String>,
    sheets: Vec<String>,
    /// 저장했다 — 에디터가 게임 데이터·브라우저를 다시 읽는다.
    saved_now: bool,
}

impl ActorEditor {
    /// 이 액터를 연다. 저장하지 않은 변경이 있으면 막는다.
    pub(crate) fn open_actor(&mut self, id: u32) {
        if self.is_dirty() && self.current != Some(id) {
            self.open = true;
            self.status = Some((true, self.dirty_message()));
            return;
        }
        self.open = true;
        if let Err(e) = self.reload() {
            self.status = Some((true, e));
            return;
        }
        match self.saved.get(&id) {
            Some(form) => {
                self.current = Some(id);
                self.edit = Some(form.clone());
                self.is_new = false;
                self.status = None;
            }
            None => self.status = Some((true, format!("#{id} 액터가 없습니다"))),
        }
    }

    /// "새 액터" 로 연다 — 빈 번호를 골라 둔다.
    pub(crate) fn open_new(&mut self) {
        self.open = true;
        if let Err(e) = self.reload() {
            self.status = Some((true, e));
            return;
        }
        self.new_id = self.next_free_id();
        self.status = Some((
            false,
            String::from("번호와 이름을 적고 '+ 새 액터' 를 누르세요"),
        ));
    }

    /// 저장했는가 — 한 번 읽으면 지운다.
    pub(crate) fn take_saved(&mut self) -> bool {
        std::mem::take(&mut self.saved_now)
    }

    fn is_dirty(&self) -> bool {
        match (self.current, &self.edit) {
            (Some(id), Some(edit)) => self.is_new || self.saved.get(&id) != Some(edit),
            _ => false,
        }
    }

    fn dirty_message(&self) -> String {
        format!(
            "#{} 을(를) 먼저 저장하세요",
            self.current.unwrap_or_default()
        )
    }

    /// 디스크에서 다시 읽는다 — 폼 목록과 고를 수 있는 파일들.
    fn reload(&mut self) -> Result<(), String> {
        let (rules, display) = crate::game_data::data_texts()?;
        self.saved = crate::game_data::actor_forms(&rules, &display)?;
        let root = Path::new(".");
        self.scripts = crate::content::scan_files(root, "data/scripts", ".rhai")
            .into_iter()
            .map(|p| p.trim_start_matches("data/").to_owned())
            .collect();
        self.sheets = crate::content::scan_files(root, "assets", ".sheet.ron");
        // 새 액터 번호 칸은 늘 빈 번호를 가리키게 둔다.
        if self.new_id == 0 || self.saved.contains_key(&self.new_id) {
            self.new_id = self.next_free_id();
        }
        Ok(())
    }

    fn next_free_id(&self) -> u32 {
        // 몬스터 구간(100~)의 다음 빈 번호 — 번호 구간은 관례일 뿐이라 바꿔 적어도 된다.
        let mut id = 100;
        while self.saved.contains_key(&id) {
            id += 1;
        }
        id
    }

    /// 창을 그린다.
    pub(crate) fn show(&mut self, ui: &mut egui::Ui) {
        let mut open = self.open;
        egui::Window::new("액터 편집기")
            .open(&mut open)
            .default_size([340.0, 560.0])
            .resizable(true)
            .show(ui.ctx(), |ui| {
                self.header(ui);
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| self.form(ui));
                ui.separator();
                self.footer(ui);
            });
        self.open = open;
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        let mut pick: Option<u32> = None;
        ui.horizontal(|ui| {
            let label = match (self.current, &self.edit) {
                (Some(id), Some(f)) => format!("#{id} {}", f.name),
                _ => String::from("액터를 고르세요"),
            };
            egui::ComboBox::from_id_salt("actor-pick")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    for (id, f) in &self.saved {
                        if ui
                            .selectable_label(
                                self.current == Some(*id),
                                format!("#{id} {}", f.name),
                            )
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
            self.open_actor(id);
        }
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut self.new_id)
                    .range(1..=99_999)
                    .prefix("#"),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.new_name)
                    .hint_text("새 액터 이름")
                    .desired_width(120.0),
            );
            if ui.button("+ 새 액터").clicked() {
                self.create();
            }
        });
    }

    /// 새 액터를 만든다 — 파일에는 저장할 때 들어간다.
    fn create(&mut self) {
        if self.is_dirty() {
            self.status = Some((true, self.dirty_message()));
            return;
        }
        let name = self.new_name.trim().to_owned();
        if name.is_empty() {
            self.status = Some((true, String::from("이름을 적으세요")));
            return;
        }
        if self.saved.contains_key(&self.new_id) {
            self.status = Some((true, format!("#{} 은(는) 이미 있습니다", self.new_id)));
            return;
        }
        self.current = Some(self.new_id);
        self.edit = Some(ActorForm::new(&name));
        self.is_new = true;
        self.new_name.clear();
        self.status = Some((
            false,
            format!("새 액터 #{} — 저장하면 두 표에 들어갑니다", self.new_id),
        ));
    }

    fn form(&mut self, ui: &mut egui::Ui) {
        let Some(f) = self.edit.as_mut() else {
            ui.weak("콘텐츠 브라우저의 '액터' 에서 두 번 누르거나 위에서 고르세요.");
            return;
        };
        let (scripts, sheets) = (&self.scripts, &self.sheets);

        ui.strong("표시 (display.ron)");
        egui::Grid::new("actor-look").num_columns(2).show(ui, |ui| {
            ui.label("이름");
            ui.add(egui::TextEdit::singleline(&mut f.name).desired_width(180.0));
            ui.end_row();

            ui.label("시트");
            egui::ComboBox::from_id_salt("actor-sheet")
                .width(180.0)
                .selected_text(if f.sheet.is_empty() {
                    String::from("(내장 플레이스홀더)")
                } else {
                    short(&f.sheet)
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut f.sheet, String::new(), "(내장 플레이스홀더)");
                    for s in sheets {
                        ui.selectable_value(&mut f.sheet, s.clone(), s);
                    }
                });
            ui.end_row();

            ui.label("색 (곱하기)");
            let mut rgb = [f.tint.0, f.tint.1, f.tint.2];
            ui.horizontal(|ui| {
                if ui.color_edit_button_rgb(&mut rgb).changed() {
                    f.tint = (rgb[0], rgb[1], rgb[2]);
                }
                if ui.small_button("흰색").clicked() {
                    f.tint = (1.0, 1.0, 1.0);
                }
            });
            ui.end_row();
        });
        ui.weak("⚠ 색은 곱하기라 어둡게·탁하게만 됩니다. 시트를 바꾼 것은 다시 시작해야 보입니다.");

        ui.add_space(6.0);
        ui.strong("규칙 (rules.ron)");
        egui::Grid::new("actor-rules")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("이동 속도 (m/s)");
                ui.add(
                    egui::DragValue::new(&mut f.move_speed)
                        .speed(0.1)
                        .range(0.0..=50.0),
                );
                ui.end_row();
                ui.label("HP");
                ui.add(egui::DragValue::new(&mut f.max_hp).range(1..=1_000_000));
                ui.end_row();
                ui.label("공격");
                ui.add(egui::DragValue::new(&mut f.attack).range(0..=100_000));
                ui.end_row();
                ui.label("방어");
                ui.add(egui::DragValue::new(&mut f.defense).range(0..=100_000));
                ui.end_row();
                ui.label("불사");
                ui.checkbox(&mut f.immortal, "공격을 받지 않음");
                ui.end_row();
                ui.label("진영");
                ui.add(egui::DragValue::new(&mut f.faction).range(0..=65_535));
                ui.end_row();

                ui.label("AI");
                egui::ComboBox::from_id_salt("actor-ai")
                    .width(180.0)
                    .selected_text(f.ai.label())
                    .show_ui(ui, |ui| {
                        for a in AiChoice::ALL {
                            ui.selectable_value(&mut f.ai, a, a.label());
                        }
                    });
                ui.end_row();
                ui.label("어그로 범위 (m)");
                ui.add(
                    egui::DragValue::new(&mut f.aggro_range)
                        .speed(0.1)
                        .range(0.0..=200.0),
                );
                ui.end_row();
                ui.label("귀환 거리 (m)");
                ui.add(
                    egui::DragValue::new(&mut f.leash_range)
                        .speed(0.1)
                        .range(0.0..=500.0),
                );
                ui.end_row();

                ui.label("기본 공격 스킬");
                optional_number(ui, &mut f.basic_attack, "actor-skill");
                ui.end_row();
                ui.label("드롭 표");
                optional_number(ui, &mut f.loot, "actor-loot");
                ui.end_row();

                ui.label("스크립트");
                egui::ComboBox::from_id_salt("actor-script")
                    .width(180.0)
                    .selected_text(f.script.clone().unwrap_or_else(|| String::from("없음")))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut f.script, None, "없음");
                        for s in scripts {
                            ui.selectable_value(&mut f.script, Some(s.clone()), s);
                        }
                    });
                ui.end_row();

                ui.label("경험치 보상");
                ui.add(egui::DragValue::new(&mut f.exp_reward).range(0..=1_000_000));
                ui.end_row();
                ui.label("레벨당 성장");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut f.growth.0).prefix("HP "));
                    ui.add(egui::DragValue::new(&mut f.growth.1).prefix("공 "));
                    ui.add(egui::DragValue::new(&mut f.growth.2).prefix("방 "));
                });
                ui.end_row();
            });
        ui.weak("스킬·드롭 표 번호는 rules.ron 의 skills·loot 에 있어야 합니다 (저장할 때 검사).");
    }

    fn footer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let dirty = self.is_dirty();
            let label = if dirty { "저장 *" } else { "저장" };
            // 바뀐 것이 없으면 저장하지 않는다 — 다시 쓰면 그 항목 안의 주석만 사라진다.
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
                    self.edit = self.saved.get(&id).cloned();
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

    /// 두 파일에 이 액터 항목만 갈아 끼워 저장한다. **다시 검증한 뒤에만** 쓴다.
    pub(crate) fn save(&mut self) {
        let (Some(id), Some(form)) = (self.current, self.edit.clone()) else {
            return;
        };
        if form.name.trim().is_empty() {
            self.status = Some((true, String::from("이름이 비어 있습니다")));
            return;
        }
        match save_actor(Path::new("."), id, &form) {
            Ok(()) => {
                self.is_new = false;
                let _ = self.reload();
                self.edit = self.saved.get(&id).cloned().or(Some(form));
                self.saved_now = true;
                self.status = Some((false, format!("#{id} 저장 — {RULES_PATH} · {DISPLAY_PATH}")));
            }
            Err(e) => self.status = Some((true, format!("저장하지 않았습니다 — {e}"))),
        }
    }
}

/// `없음` 칸 + 번호. 켜면 1 부터.
fn optional_number(ui: &mut egui::Ui, value: &mut Option<u32>, id: &str) {
    ui.push_id(id, |ui| {
        ui.horizontal(|ui| {
            let mut on = value.is_some();
            if ui.checkbox(&mut on, "").changed() {
                *value = on.then_some(1);
            }
            if let Some(v) = value.as_mut() {
                ui.add(egui::DragValue::new(v).range(1..=99_999));
            } else {
                ui.weak("없음");
            }
        });
    });
}

/// 긴 경로를 팩 이름 + 파일 이름으로 줄인다.
fn short(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() > 2 {
        format!("…/{}/{}", parts[parts.len() - 2], parts[parts.len() - 1])
    } else {
        path.to_owned()
    }
}

/// `root` 아래 두 파일에 액터 항목을 쓴다 — 검증을 통과해야만.
fn save_actor(root: &Path, id: u32, form: &ActorForm) -> Result<(), String> {
    let rules_path = root.join(RULES_PATH);
    let display_path = root.join(DISPLAY_PATH);
    let rules = std::fs::read_to_string(&rules_path).map_err(|e| format!("{RULES_PATH}: {e}"))?;
    let display =
        std::fs::read_to_string(&display_path).map_err(|e| format!("{DISPLAY_PATH}: {e}"))?;
    let (rules, display) = patch_actor(&rules, &display, id, form)?;
    // 둘 다 임시 파일에 먼저 쓰고 이름을 바꾼다 — 한쪽만 바뀐 채 멈추는 창을 좁힌다.
    let tmp_rules = rules_path.with_extension("ron.tmp");
    let tmp_display = display_path.with_extension("ron.tmp");
    let io = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    std::fs::write(&tmp_rules, &rules).map_err(|e| io(&tmp_rules, e))?;
    if let Err(e) = std::fs::write(&tmp_display, &display) {
        let _ = std::fs::remove_file(&tmp_rules);
        return Err(io(&tmp_display, e));
    }
    std::fs::rename(&tmp_rules, &rules_path).map_err(|e| io(&rules_path, e))?;
    std::fs::rename(&tmp_display, &display_path).map_err(|e| io(&display_path, e))
}

/// 두 텍스트에 액터 항목을 끼우고 **게임이 읽는 것과 같은 검사**를 통과하는지 본다.
fn patch_actor(
    rules: &str,
    display: &str,
    id: u32,
    form: &ActorForm,
) -> Result<(String, String), String> {
    let rules = crate::ron_patch::upsert_entry(rules, "actors", id, &form.rules_entry(id))?;
    let display = crate::ron_patch::upsert_entry(display, "actors", id, &form.display_entry(id))?;
    GameData::parse(&rules, &display)?;
    Ok((rules, display))
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

    /// 모든 액터를 **바꾸지 않고** 다시 써도 같은 값으로 읽힌다 — 폼 ↔ 텍스트가 새지 않는다.
    #[test]
    fn every_actor_round_trips_through_the_form() {
        let (rules, display) = repo_texts();
        let forms = crate::game_data::actor_forms(&rules, &display).unwrap();
        assert!(forms.len() >= 5);
        let (mut r, mut d) = (rules.clone(), display.clone());
        for (id, form) in &forms {
            (r, d) = patch_actor(&r, &d, *id, form).unwrap();
        }
        assert_eq!(crate::game_data::actor_forms(&r, &d).unwrap(), forms);
    }

    #[test]
    fn a_changed_actor_keeps_other_entries_and_their_comments() {
        let (rules, display) = repo_texts();
        let forms = crate::game_data::actor_forms(&rules, &display).unwrap();
        let mut goblin = forms[&102].clone();
        goblin.max_hp = 777;
        goblin.name = String::from("큰 고블린");
        let (r, d) = patch_actor(&rules, &display, 102, &goblin).unwrap();

        let after = crate::game_data::actor_forms(&r, &d).unwrap();
        assert_eq!(after[&102].max_hp, 777);
        assert_eq!(after[&102].name, "큰 고블린");
        assert_eq!(after[&100], forms[&100], "다른 액터는 그대로");
        // 다른 항목 위의 주석은 남는다.
        assert!(r.contains("서버 MonsterEntityData 기본값처럼 공격형"));
        assert!(d.contains("곱하기라 **어둡게·탁하게만** 된다"));
    }

    #[test]
    fn a_new_actor_is_added_to_both_tables() {
        let (rules, display) = repo_texts();
        let mut form = ActorForm::new("박쥐");
        form.ai = AiChoice::Aggressive;
        form.aggro_range = 5.0;
        form.basic_attack = Some(2);
        let (r, d) = patch_actor(&rules, &display, 150, &form).unwrap();
        let after = crate::game_data::actor_forms(&r, &d).unwrap();
        assert_eq!(after[&150], form);
    }

    #[test]
    fn a_broken_reference_is_refused_before_writing() {
        let (rules, display) = repo_texts();
        let mut form = ActorForm::new("망가진");
        form.basic_attack = Some(99_999);
        let err = patch_actor(&rules, &display, 151, &form).unwrap_err();
        assert!(err.contains("99999"), "{err}");
    }

    #[test]
    fn saving_writes_both_files_through_temp_files() {
        let dir = std::env::temp_dir().join(format!("nexus-actor-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("data")).unwrap();
        let (rules, display) = repo_texts();
        std::fs::write(dir.join(RULES_PATH), &rules).unwrap();
        std::fs::write(dir.join(DISPLAY_PATH), &display).unwrap();

        let mut form = ActorForm::new("박쥐");
        form.max_hp = 33;
        save_actor(&dir, 150, &form).unwrap();

        let r = std::fs::read_to_string(dir.join(RULES_PATH)).unwrap();
        let d = std::fs::read_to_string(dir.join(DISPLAY_PATH)).unwrap();
        assert_eq!(crate::game_data::actor_forms(&r, &d).unwrap()[&150], form);
        assert!(!r.contains('\r'), "줄바꿈은 LF");
        let leftovers = std::fs::read_dir(dir.join("data"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(leftovers, 0, "임시 파일이 남지 않는다");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
