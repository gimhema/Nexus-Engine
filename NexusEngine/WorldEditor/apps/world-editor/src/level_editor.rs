//! 레벨 편집기 — 레벨이 쓸 존과 띄울 화면을 고른다 (단계 2 P8).
//!
//! ```text
//! ┌ 레벨 편집기 ─────────────────────────────┐
//! │ [village (마을) ▼]   [번호] [+ 새 레벨]    │
//! │ 이름      [마을]                          │
//! │ 존        [zones/village.zone.ron ▼]      │
//! │ HUD       [hud ▼]                         │
//! │ 들어갈 때 [없음 ▼]                        │
//! │ 일시정지  [pause ▼]                       │
//! │ 로딩 화면 [loading ▼]  최소 [800 ms]        │
//! │ ★ 시작 레벨 / [시작 레벨로 지정]           │
//! │ [저장] [원래대로] [이 레벨 플레이]          │
//! └──────────────────────────────────────────┘
//! ```
//!
//! - 레벨 파일은 **편집기가 소유한다** — 저장하면 통째로 다시 쓴다 (`level::to_ron`).
//! - 시작 레벨은 `data/project.ron` 의 **`startup_level` 줄만** 바꾼다 — 그 파일의 주석은 남는다.
//! - 쓰기 전에 **다시 읽어 검증**하고(`read_level`), 임시 파일 → 이름 바꾸기로 쓴다.
//! - 고를 수 있는 존·화면은 디스크의 파일 목록이다. 없는 화면을 가리켜도 파일은 읽히지만
//!   그 화면은 뜨지 않는다 — 목록에서 고르면 그런 일이 없다.

use std::path::Path;

use nexus_render_wgpu::egui;

use crate::level::{LevelFile, PROJECT_PATH, level_path, read_level, read_project};

#[derive(Debug, Default)]
pub(crate) struct LevelEditor {
    open: bool,
    /// 편집 중인 레벨 번호.
    current: String,
    edit: Option<LevelFile>,
    /// 디스크의 값. 새 레벨이면 `None`.
    saved: Option<LevelFile>,
    status: Option<(bool, String)>,
    new_id: String,
    /// 고를 수 있는 것들 — 창을 열 때 디스크에서 읽는다.
    levels: Vec<String>,
    zones: Vec<String>,
    screens: Vec<String>,
    startup: String,
    /// 저장했다 — 에디터가 레벨 목록·브라우저를 다시 읽는다.
    saved_now: bool,
    /// "이 레벨 플레이" — 에디터가 레벨을 연다.
    play: Option<String>,
}

impl LevelEditor {
    /// 이 레벨을 연다. 저장하지 않은 변경이 있으면 막는다.
    pub(crate) fn open_level(&mut self, id: &str) {
        self.open = true;
        self.reload_lists();
        if self.is_dirty() && self.current != id {
            self.status = Some((true, format!("'{}' 를 먼저 저장하세요", self.current)));
            return;
        }
        let path = level_path(id);
        match std::fs::read_to_string(&path)
            .map_err(|e| format!("{}: {e}", path.display()))
            .and_then(|t| read_level(&t))
        {
            Ok(level) => {
                self.current = id.to_owned();
                self.edit = Some(level.clone());
                self.saved = Some(level);
                self.status = None;
            }
            Err(e) => self.status = Some((true, e)),
        }
    }

    /// "새 레벨" 로 연다.
    pub(crate) fn open_new(&mut self) {
        self.open = true;
        self.reload_lists();
        self.status = Some((
            false,
            String::from("번호(영문 소문자)를 적고 '+ 새 레벨' 을 누르세요"),
        ));
    }

    pub(crate) fn take_saved(&mut self) -> bool {
        std::mem::take(&mut self.saved_now)
    }

    pub(crate) fn take_play(&mut self) -> Option<String> {
        self.play.take()
    }

    fn is_dirty(&self) -> bool {
        self.edit.is_some() && self.edit != self.saved
    }

    /// 이 레벨을 저장하지 않은 채 고치고 있는가 — 파일 작업이 막는다 (P9).
    pub(crate) fn blocks(&self, id: &str) -> bool {
        self.current == id && self.is_dirty()
    }

    /// 레벨이 옮겨지거나 지워졌다 — 열어 둔 것이 그 레벨이면 닫는다.
    pub(crate) fn forget(&mut self, id: &str) {
        if self.current == id {
            self.current.clear();
            self.edit = None;
            self.saved = None;
        }
        self.reload_lists();
    }

    fn reload_lists(&mut self) {
        let root = Path::new(".");
        self.levels = crate::content::scan_files(root, "levels", ".level.ron")
            .iter()
            .filter_map(|p| {
                p.rsplit('/')
                    .next()?
                    .strip_suffix(".level.ron")
                    .map(str::to_owned)
            })
            .collect();
        self.zones = crate::content::scan_files(root, "zones", ".zone.ron");
        self.screens = crate::content::scan_files(root, "ui", ".ui.ron")
            .iter()
            .filter_map(|p| {
                p.rsplit('/')
                    .next()?
                    .strip_suffix(".ui.ron")
                    .map(str::to_owned)
            })
            .collect();
        self.startup = std::fs::read_to_string(PROJECT_PATH)
            .ok()
            .and_then(|t| read_project(&t).ok())
            .map(|p| p.startup_level)
            .unwrap_or_default();
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui) {
        let mut open = self.open;
        egui::Window::new("레벨 편집기")
            .open(&mut open)
            .default_size([340.0, 360.0])
            .resizable(true)
            .show(ui.ctx(), |ui| {
                self.header(ui);
                ui.separator();
                self.form(ui);
                ui.separator();
                self.footer(ui);
            });
        self.open = open;
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        let mut pick = None;
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("level-pick")
                .selected_text(if self.current.is_empty() {
                    String::from("레벨을 고르세요")
                } else {
                    self.current.clone()
                })
                .show_ui(ui, |ui| {
                    for id in &self.levels {
                        let star = if *id == self.startup { "  ★" } else { "" };
                        if ui
                            .selectable_label(*id == self.current, format!("{id}{star}"))
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
            self.open_level(&id);
        }
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.new_id)
                    .hint_text("새 레벨 번호 (영문 소문자)")
                    .desired_width(170.0),
            );
            if ui.button("+ 새 레벨").clicked() {
                self.create();
            }
        });
    }

    fn create(&mut self) {
        let id = self.new_id.trim().to_ascii_lowercase();
        if !crate::screen::valid_screen_id(&id) {
            self.status = Some((
                true,
                String::from("레벨 번호는 영문 소문자·숫자·_·- 만 (파일 이름이 됩니다)"),
            ));
            return;
        }
        if level_path(&id).exists() {
            self.status = Some((true, format!("levels/{id}.level.ron 이 이미 있습니다")));
            return;
        }
        if self.is_dirty() {
            self.status = Some((true, format!("'{}' 를 먼저 저장하세요", self.current)));
            return;
        }
        self.current = id.clone();
        // 새 레벨은 UI 레벨로 시작한다 — 메인 화면을 바탕으로. 존을 고르면 게임 레벨이 된다.
        self.edit = Some(LevelFile {
            version: 1,
            name: id.clone(),
            on_enter: self.screens.first().cloned(),
            ..LevelFile::default()
        });
        self.saved = None;
        self.new_id.clear();
        self.status = Some((false, format!("새 레벨 '{id}' — 저장하면 파일이 생깁니다")));
    }

    fn form(&mut self, ui: &mut egui::Ui) {
        let Some(level) = self.edit.as_mut() else {
            ui.weak("콘텐츠 브라우저의 '레벨' 에서 두 번 누르거나 위에서 고르세요.");
            return;
        };
        let (zones, screens) = (&self.zones, &self.screens);
        egui::Grid::new("level-form").num_columns(2).show(ui, |ui| {
            ui.label("이름");
            ui.add(egui::TextEdit::singleline(&mut level.name).desired_width(200.0));
            ui.end_row();

            ui.label("존");
            egui::ComboBox::from_id_salt("level-zone")
                .width(200.0)
                .selected_text(
                    level
                        .zone
                        .clone()
                        .unwrap_or_else(|| String::from("없음 (UI 레벨)")),
                )
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut level.zone, None, "없음 (UI 레벨)");
                    for z in zones {
                        ui.selectable_value(&mut level.zone, Some(z.clone()), z);
                    }
                });
            ui.end_row();

            for (label, slot, salt) in [
                ("HUD", &mut level.hud, "level-hud"),
                ("들어갈 때", &mut level.on_enter, "level-enter"),
                ("일시정지", &mut level.pause, "level-pause"),
                ("로딩 화면", &mut level.loading, "level-loading"),
            ] {
                ui.label(label);
                egui::ComboBox::from_id_salt(salt)
                    .width(200.0)
                    .selected_text(slot.clone().unwrap_or_else(|| String::from("없음")))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(slot, None, "없음");
                        for s in screens {
                            ui.selectable_value(slot, Some(s.clone()), s);
                        }
                    });
                ui.end_row();
            }

            ui.label("로딩 최소 시간");
            ui.add_enabled(
                level.loading.is_some(),
                egui::DragValue::new(&mut level.loading_ms)
                    .range(0..=crate::level::MAX_LOADING_MS)
                    .speed(10.0)
                    .suffix(" ms"),
            );
            ui.end_row();
        });
        ui.weak("존이 없으면 UI 만 있는 레벨(메인 화면)입니다 — '들어갈 때' 화면이 바탕이 됩니다.");
        ui.weak("로딩 화면은 이 레벨을 여는 동안 뜹니다 — 막대의 값을 '로딩 진행' 으로, 글자에 {loading} 을 쓰면 진행률이 보입니다.");
    }

    fn footer(&mut self, ui: &mut egui::Ui) {
        let has = self.edit.is_some();
        ui.horizontal(|ui| {
            if self.current == self.startup && !self.current.is_empty() {
                ui.colored_label(egui::Color32::from_rgb(230, 190, 80), "★ 시작 레벨");
            } else if ui
                .add_enabled(
                    has && self.saved.is_some(),
                    egui::Button::new("시작 레벨로 지정"),
                )
                .clicked()
            {
                self.set_startup();
            }
        });
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
                self.edit = self.saved.clone();
                if self.edit.is_none() {
                    self.current.clear();
                }
            }
            // 저장한 레벨만 연다 — 레벨을 여는 것은 디스크의 파일을 읽는 일이다.
            if ui
                .add_enabled(has && !dirty, egui::Button::new("이 레벨 플레이"))
                .clicked()
            {
                self.play = Some(self.current.clone());
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
        let Some(level) = self.edit.clone() else {
            return;
        };
        match save_level(Path::new("."), &self.current, &level) {
            Ok(path) => {
                self.saved = Some(level);
                self.saved_now = true;
                self.reload_lists();
                self.status = Some((false, format!("{path} 저장")));
            }
            Err(e) => self.status = Some((true, format!("저장하지 않았습니다 — {e}"))),
        }
    }

    fn set_startup(&mut self) {
        let path = Path::new(".").join(PROJECT_PATH);
        let result = std::fs::read_to_string(&path)
            .map_err(|e| format!("{PROJECT_PATH}: {e}"))
            .and_then(|t| crate::level::patch_startup(&t, &self.current))
            .and_then(|t| write(&path, &t));
        match result {
            Ok(()) => {
                self.startup = self.current.clone();
                self.saved_now = true;
                self.status = Some((false, format!("시작 레벨 = '{}'", self.current)));
            }
            Err(e) => self.status = Some((true, e)),
        }
    }
}

/// 레벨 파일을 쓴다 — 번호 규칙과 다시 읽기 검증을 통과해야만.
fn save_level(root: &Path, id: &str, level: &LevelFile) -> Result<String, String> {
    if !crate::screen::valid_screen_id(id) {
        return Err(format!("'{id}' 는 파일 이름이 될 수 없습니다"));
    }
    let text = crate::level::to_ron(level);
    read_level(&text)?;
    let path = root.join(level_path(id));
    write(&path, &text)?;
    Ok(level_path(id).to_string_lossy().replace('\\', "/"))
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

    #[test]
    fn saving_a_level_writes_the_editor_format_and_refuses_bad_ones() {
        let dir = std::env::temp_dir().join(format!("nexus-level-{}", std::process::id()));
        let level = LevelFile {
            version: 1,
            name: String::from("동굴"),
            zone: Some(String::from("zones/cave.zone.ron")),
            hud: Some(String::from("hud")),
            pause: Some(String::from("pause")),
            ..LevelFile::default()
        };
        let path = save_level(&dir, "cave", &level).unwrap();
        assert_eq!(path, "levels/cave.level.ron");
        let text = std::fs::read_to_string(dir.join(&path)).unwrap();
        assert_eq!(read_level(&text).unwrap(), level);
        assert!(!text.contains('\r'), "줄바꿈은 LF");

        // 존도 화면도 없는 레벨은 검증에서 막힌다 — 파일을 쓰지 않는다.
        let empty = LevelFile {
            version: 1,
            name: String::from("빈"),
            ..LevelFile::default()
        };
        assert!(save_level(&dir, "empty", &empty).is_err());
        assert!(!dir.join("levels/empty.level.ron").exists());
        // 파일 이름이 될 수 없는 번호.
        assert!(save_level(&dir, "Cave/../x", &level).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
