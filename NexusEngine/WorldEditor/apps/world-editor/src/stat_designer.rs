//! 스탯·직업 디자이너 — 기본 스탯·파생 수식·직업을 고치고 **레벨별 수치를 바로 본다** (R2).
//!
//! ```text
//! ┌ 스탯·직업 디자이너 ───────────────────────────────────────────────┐
//! │ ── 기본 스탯 ──              │ ── 미리보기: 전사 ──                  │
//! │ [str] [힘  ] ×               │ Lv   HP   MP  회복 공격 방어  힘 체력… │
//! │ [vit] [체력] ×  [+ 스탯]      │  1  200   60    2   20    5   10    9 │
//! │ ── 파생 수식 ──              │  5  360  100    4   36   13   18   13 │
//! │ 최대 HP [vit * 20 + level*20] │ 10  560  150    7   56   23   28   18 │
//! │ 공격    [str * 2 + equip_a…]  │ …                                     │
//! │   ⚠ 3번째 글자: 모르는 이름 'x' │                                       │
//! │ ── 직업 ── [전사 ▼] [+ 새 직업]  │                                       │
//! │ 이름·설명 · 스탯 표(Lv1·성장) · 단축키 · 이 직업만 다른 수식             │
//! │ [저장 *] [원래대로]                                                     │
//! └────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! - 수식을 **고치는 즉시** 칸 아래에 오류를 띄우고, 미리보기 표가 다시 계산된다. 미리보기는
//!   게임과 같은 계산(`StatRules::sheet`)이라 표에 보이는 값이 플레이에서 나오는 값이다(장비 없이).
//! - 저장은 두 파일의 네 블록(`rules.ron` 의 `stats`·`classes`, `display.ron` 의 `classes`·`stats`)만
//!   갈아 끼우고, 쓰기 전에 **게임과 같은 검사**로 다시 읽는다 — 깨진 수식·없는 스킬이면 쓰지 않는다.
//!   ⚠ 그 블록 **안**의 주석은 남지 않는다 (다른 표 편집기와 같다).
//! - 스탯 이름을 바꾸거나 빼도 수식·장비 보너스를 **자동으로 고쳐 쓰지 않는다** — 저장 검사가 깨진
//!   곳을 알려 준다. 이름은 데이터의 열쇠라 조용히 바뀌면 오히려 위험하다.

use std::path::Path;

use nexus_render_wgpu::egui;
use nexus_sim::DerivedStat;

use crate::class_file::{ClassForm, StatForm};
use crate::game_data::{DISPLAY_PATH, RULES_PATH, patch_stat_form, stat_form, write_tables};

/// 미리보기 표의 레벨들.
const PREVIEW_LEVELS: [u32; 8] = [1, 2, 3, 5, 10, 20, 30, 50];

/// 파생 스탯의 화면 이름.
fn derived_label(d: DerivedStat) -> &'static str {
    match d {
        DerivedStat::MaxHp => "최대 HP",
        DerivedStat::MaxMp => "최대 MP",
        DerivedStat::MpRegen => "MP 회복/초",
        DerivedStat::Attack => "공격력",
        DerivedStat::Defense => "방어력",
    }
}

#[derive(Debug, Default)]
pub(crate) struct StatDesigner {
    open: bool,
    saved: Option<StatForm>,
    edit: Option<StatForm>,
    /// 고르고 있는 직업 (폼의 `classes` 번호).
    class: usize,
    status: Option<(bool, String)>,
    saved_now: bool,
}

impl StatDesigner {
    /// 창을 연다. 저장하지 않은 변경이 있으면 그대로 둔다.
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
        let (rules, display) = crate::game_data::data_texts()?;
        let form = stat_form(&rules, &display)?;
        self.class = self.class.min(form.classes.len().saturating_sub(1));
        self.saved = Some(form.clone());
        self.edit = Some(form);
        Ok(())
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        egui::Window::new("스탯·직업 디자이너")
            .id(egui::Id::new("stat_designer"))
            .open(&mut open)
            .default_size([860.0, 560.0])
            .resizable(true)
            .show(ui.ctx(), |ui| {
                let Some(form) = self.edit.as_mut() else {
                    ui.weak("규칙 파일을 읽지 못했습니다.");
                    return;
                };
                let height = ui.available_height() - 60.0;
                ui.columns(2, |cols| {
                    egui::ScrollArea::vertical()
                        .id_salt("stat-designer-form")
                        .max_height(height)
                        .show(&mut cols[0], |ui| edit_form(ui, form, &mut self.class));
                    egui::ScrollArea::both()
                        .id_salt("stat-designer-preview")
                        .max_height(height)
                        .show(&mut cols[1], |ui| preview(ui, form, self.class));
                });
                ui.separator();
                self.footer(ui);
            });
        self.open = open;
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

    /// 저장 — 게임과 같은 검사를 통과해야만 쓴다.
    pub(crate) fn save(&mut self) {
        let Some(form) = self.edit.clone() else {
            return;
        };
        match save_form(Path::new("."), &form) {
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

/// `root` 아래 두 파일에 폼을 쓴다 — 함께 다시 검증한 뒤에만.
fn save_form(root: &Path, form: &StatForm) -> Result<(), String> {
    let read = |p: &str| std::fs::read_to_string(root.join(p)).map_err(|e| format!("{p}: {e}"));
    let (rules, display) = patch_stat_form(&read(RULES_PATH)?, &read(DISPLAY_PATH)?, form)?;
    write_tables(root, &rules, &display)
}

/// 수식 칸 하나 — 고치는 즉시 검사해 오류를 아래에 띄운다.
fn formula_field(ui: &mut egui::Ui, form_vars: &StatForm, text: &mut String, hint: &str) {
    ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .desired_width(240.0)
            .font(egui::TextStyle::Monospace),
    );
    if let Err(e) = form_vars.check_formula(text) {
        ui.colored_label(egui::Color32::from_rgb(240, 110, 100), format!("⚠ {e}"));
    }
}

fn edit_form(ui: &mut egui::Ui, form: &mut StatForm, class: &mut usize) {
    ui.strong("기본 스탯");
    ui.weak("규칙 이름(수식이 쓰는 이름 — 영문 소문자)과 화면 이름.");
    let mut remove = None;
    for (i, row) in form.stats.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut row.key)
                    .desired_width(70.0)
                    .font(egui::TextStyle::Monospace),
            );
            ui.add(
                egui::TextEdit::singleline(&mut row.label)
                    .hint_text("화면 이름")
                    .desired_width(90.0),
            );
            if ui.small_button("×").on_hover_text("이 스탯 빼기").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        form.remove_stat(i);
    }
    if ui.button("+ 스탯").clicked() {
        let n = form.stats.len() + 1;
        form.push_stat(format!("stat{n}"));
    }

    ui.add_space(8.0);
    ui.strong("파생 스탯 수식");
    ui.weak(format!(
        "정수 연산 · + - * / % · min max clamp · 쓸 수 있는 이름: {}",
        form.variables().join(", ")
    ));
    let vars = form.clone();
    for (d, text) in DerivedStat::ALL.into_iter().zip(form.derived.iter_mut()) {
        ui.label(derived_label(d));
        formula_field(ui, &vars, text, "비우면 0");
    }

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.strong("직업");
        let selected = form
            .classes
            .get(*class)
            .map_or_else(|| String::from("—"), class_title);
        egui::ComboBox::from_id_salt("stat-designer-class")
            .selected_text(selected)
            .show_ui(ui, |ui| {
                for (i, c) in form.classes.iter().enumerate() {
                    ui.selectable_value(class, i, class_title(c));
                }
            });
        if ui.button("+ 새 직업").clicked() {
            let id = form.classes.iter().map(|c| c.id).max().unwrap_or(0) + 1;
            let n = form.stats.len();
            form.classes.push(ClassForm {
                id,
                name: String::from("새 직업"),
                base: vec![1; n],
                growth: vec![1; n],
                ..ClassForm::default()
            });
            *class = form.classes.len() - 1;
        }
        if !form.classes.is_empty()
            && ui
                .small_button("이 직업 빼기")
                .on_hover_text("직업 고르기 화면·장비 제한이 이 번호를 쓰면 저장할 때 막힌다")
                .clicked()
        {
            form.classes.remove(*class);
            *class = class.saturating_sub(1);
        }
    });
    let stats = form.stats.clone();
    let Some(c) = form.classes.get_mut(*class) else {
        ui.weak("직업이 없습니다 — 직업이 없으면 플레이어도 액터 수치를 그대로 씁니다.");
        return;
    };
    egui::Grid::new("stat-designer-class-head")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("번호");
            ui.add(egui::DragValue::new(&mut c.id).range(1..=99_999))
                .on_hover_text(
                    "서버와 같은 ID 공간. 직업 고르기 화면의 NewGame(class: N) 이 이 번호를 쓴다",
                );
            ui.end_row();
            ui.label("이름");
            ui.add(egui::TextEdit::singleline(&mut c.name).desired_width(140.0));
            ui.end_row();
            ui.label("설명");
            ui.add(egui::TextEdit::singleline(&mut c.description).desired_width(240.0));
            ui.end_row();
            ui.label("시작 스킬");
            let mut text = c
                .skills
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            if ui
                .add(egui::TextEdit::singleline(&mut text).desired_width(140.0))
                .on_hover_text("스킬 번호를 쉼표로 — 새 게임에서 1레벨로 배우고 처음 단축키에 차례로 놓인다. 비우면 액터 타입의 것")
                .changed()
            {
                c.skills = text
                    .split(',')
                    .filter_map(|t| t.trim().parse().ok())
                    .collect();
            }
            ui.end_row();
        });
    egui::Grid::new("stat-designer-class-stats")
        .num_columns(3)
        .striped(true)
        .show(ui, |ui| {
            ui.weak("스탯");
            ui.weak("레벨 1");
            ui.weak("레벨당 성장");
            ui.end_row();
            for (i, s) in stats.iter().enumerate() {
                ui.label(if s.label.is_empty() { &s.key } else { &s.label });
                if let Some(v) = c.base.get_mut(i) {
                    ui.add(egui::DragValue::new(v).range(-100_000..=100_000));
                }
                if let Some(v) = c.growth.get_mut(i) {
                    ui.add(egui::DragValue::new(v).range(-100_000..=100_000));
                }
                ui.end_row();
            }
        });
    ui.add_space(4.0);
    ui.label("이 직업만 다른 수식 (비우면 공통 수식)");
    for (d, text) in DerivedStat::ALL.into_iter().zip(c.formulas.iter_mut()) {
        ui.label(derived_label(d));
        formula_field(ui, &vars, text, "공통 수식");
    }
}

fn class_title(c: &ClassForm) -> String {
    format!("#{} {}", c.id, c.name)
}

/// 고른 직업의 레벨별 수치 — 게임과 같은 계산.
fn preview(ui: &mut egui::Ui, form: &StatForm, class: usize) {
    let Some(c) = form.classes.get(class) else {
        ui.weak("미리볼 직업이 없습니다.");
        return;
    };
    ui.strong(format!("미리보기: {} (장비 없이)", class_title(c)));
    match form.preview(class, &PREVIEW_LEVELS) {
        Err(e) => {
            ui.colored_label(egui::Color32::from_rgb(240, 110, 100), format!("⚠ {e}"));
        }
        Ok(rows) => {
            egui::Grid::new("stat-designer-preview")
                .num_columns(1 + DerivedStat::ALL.len() + form.stats.len())
                .striped(true)
                .show(ui, |ui| {
                    ui.weak("Lv");
                    for d in DerivedStat::ALL {
                        ui.weak(derived_label(d));
                    }
                    for s in &form.stats {
                        ui.weak(if s.label.is_empty() { &s.key } else { &s.label });
                    }
                    ui.end_row();
                    for (lv, sheet) in rows {
                        ui.monospace(lv.to_string());
                        for d in DerivedStat::ALL {
                            ui.monospace(sheet.derived.get(d).to_string());
                        }
                        for v in &sheet.base {
                            ui.monospace(v.to_string());
                        }
                        ui.end_row();
                    }
                });
        }
    }
    // 직업끼리 같은 레벨에서 견주기.
    ui.add_space(10.0);
    ui.strong("직업 비교 (레벨 10)");
    egui::Grid::new("stat-designer-compare")
        .num_columns(1 + DerivedStat::ALL.len())
        .striped(true)
        .show(ui, |ui| {
            ui.weak("직업");
            for d in DerivedStat::ALL {
                ui.weak(derived_label(d));
            }
            ui.end_row();
            for (i, c) in form.classes.iter().enumerate() {
                ui.label(class_title(c));
                match form.preview(i, &[10]) {
                    Ok(rows) => {
                        for d in DerivedStat::ALL {
                            ui.monospace(rows[0].1.derived.get(d).to_string());
                        }
                    }
                    Err(_) => {
                        ui.weak("수식 오류");
                    }
                }
                ui.end_row();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saving_writes_both_files_and_refuses_a_broken_formula() {
        let dir = std::env::temp_dir().join(format!("nexus-stat-designer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("data")).unwrap();
        let rules = crate::game_data::GameData::embedded_rules();
        let display = include_str!("../../../data/display.ron");
        std::fs::write(dir.join(RULES_PATH), rules).unwrap();
        std::fs::write(dir.join(DISPLAY_PATH), display).unwrap();

        let mut form = stat_form(rules, display).unwrap();
        form.classes[0].name = String::from("기사");
        form.derived[DerivedStat::Defense as usize] = String::from("agi * 2 + equip_defense");
        save_form(&dir, &form).unwrap();
        let (r, d) = (
            std::fs::read_to_string(dir.join(RULES_PATH)).unwrap(),
            std::fs::read_to_string(dir.join(DISPLAY_PATH)).unwrap(),
        );
        assert!(d.contains("1: (name: \"기사\""));
        assert!(r.contains("defense: \"agi * 2 + equip_defense\","));
        assert_eq!(stat_form(&r, &d).unwrap(), form);

        form.derived[DerivedStat::Attack as usize] = String::from("str * (");
        let err = save_form(&dir, &form).unwrap_err();
        assert!(err.contains("attack"), "{err}");
        assert_eq!(
            std::fs::read_to_string(dir.join(RULES_PATH)).unwrap(),
            r,
            "깨진 수식이면 쓰지 않는다"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
