//! 스크립트 편집기 창 — 에디터 안에서 액터 스크립트(`data/scripts/*.rhai`)를 쓰고 컴파일한다 (P2).
//!
//! ```text
//! [파일 ▼] [새 스크립트: ____ 만들기]            [컴파일] [저장 *]
//! 컴파일 성공 — 훅: on_spawn, on_tick · 쓰는 액터: 고블린 (#102)
//! ┌──┬──────────────────────────────────────────────┐
//! │ 1│ fn on_spawn(me) {                            │
//! │ 2│     this.timer = 0.0;                        │  ← 오류 난 줄은 빨간 바탕
//! └──┴──────────────────────────────────────────────┘
//! ```
//!
//! - **컴파일**은 실행 없이 검사만 한다 (`nexus_script::check`) — 문법·선언하지 않은 변수·훅 인자 수.
//!   플레이 때와 같은 판정이다.
//! - **저장**(버튼 또는 편집 중 Ctrl+S)은 임시 파일에 쓴 뒤 이름을 바꾼다. 저장할 때 컴파일도 한다.
//! - 플레이는 늘 **디스크의 파일**로 돈다. 저장하지 않은 변경이 있으면 F5 가 먼저 저장한다
//!   (`Editor::start_play`) — 고친 것이 적용되지 않은 채 모르고 플레이하지 않게.
//! - 파일 이름은 **소문자** — 에셋과 같은 규칙(리눅스에서 "파일 없음" 이 되지 않게).

use std::path::{Path, PathBuf};

use nexus_render_wgpu::egui;
use nexus_script::{Compiled, Diagnostic};

use crate::game_data::{DATA_DIR, script_path_ok};

/// 스크립트를 두는 폴더 (`data/` 기준).
pub(crate) const SCRIPT_DIR: &str = "scripts";

/// 새 스크립트의 틀 — 훅 목록을 보여 주고 바로 컴파일된다.
fn template() -> String {
    let mut s = String::from(
        "// 액터 스크립트. 필요한 훅만 남기고 지워도 된다.\n\
         // 상태는 this 에 둔다 (액터마다 따로). 행동은 move_to / attack / stop 으로 요청한다.\n\n",
    );
    for sig in nexus_script::hook_signatures() {
        s.push_str(&sig);
        s.push_str(" {\n}\n\n");
    }
    s.pop();
    s
}

/// 편집기 상태. 창이 닫혀 있어도 열던 파일과 고치던 내용은 남긴다.
#[derive(Debug, Default)]
pub(crate) struct ScriptEditor {
    open: bool,
    /// `data/` 기준 경로 목록 (`scripts/goblin.rhai`). 창을 열 때·저장할 때 다시 읽는다.
    files: Vec<String>,
    /// 열린 파일 (`data/` 기준).
    path: Option<String>,
    text: String,
    /// 마지막으로 읽거나 저장한 내용 — 이것과 다르면 "저장 안 됨".
    saved: String,
    /// 마지막 컴파일 결과. 고치기 시작하면 지운다 (옛 오류 줄을 짚지 않게).
    result: Option<Result<Compiled, Diagnostic>>,
    /// 마지막 저장·열기의 결과 문구.
    status: Option<(bool, String)>,
    new_name: String,
}

/// 편집기가 편집기 밖에 요청하는 것.
#[derive(Debug, Default)]
pub(crate) struct ScriptEditorActions {
    /// 편집 중 F5 — 전역 단축키는 글자 입력 중에 꺼져 있으므로 여기서 받는다.
    pub(crate) toggle_play: bool,
}

impl ScriptEditor {
    /// 창을 연다. `path` 가 있으면 그 파일을 연다 (자동 검증용).
    pub(crate) fn open(&mut self, path: Option<&str>) {
        self.open = true;
        self.files = list(&Path::new(DATA_DIR).join(SCRIPT_DIR));
        if let Some(path) = path {
            self.load(path);
        } else if self.path.is_none()
            && let Some(first) = self.files.first().cloned()
        {
            self.load(&first);
        }
    }

    /// 저장하지 않은 변경이 있다.
    pub(crate) fn is_dirty(&self) -> bool {
        self.path.is_some() && self.text != self.saved
    }

    /// 지금 내용을 컴파일한다 (저장하지 않는다).
    pub(crate) fn compile(&mut self) {
        self.result = Some(nexus_script::check(&self.text));
    }

    /// 지금 내용을 저장하고 컴파일 결과를 갱신한다. 오류가 있어도 저장은 한다 —
    /// 고치던 것을 잃지 않는 게 먼저다 (플레이는 그 파일로 시작하지 못하고 이유를 알린다).
    pub(crate) fn save(&mut self) -> Result<String, String> {
        let Some(path) = self.path.clone() else {
            return Err(String::from("열린 스크립트가 없음"));
        };
        let result = save(Path::new(DATA_DIR), &path, &self.text);
        match &result {
            Ok(_) => {
                self.saved.clone_from(&self.text);
                self.compile();
                self.files = list(&Path::new(DATA_DIR).join(SCRIPT_DIR));
                self.status = Some((false, format!("{DATA_DIR}/{path} 저장")));
            }
            Err(e) => self.status = Some((true, e.clone())),
        }
        result.map(|()| format!("{DATA_DIR}/{path}"))
    }

    fn load(&mut self, path: &str) {
        match std::fs::read_to_string(Path::new(DATA_DIR).join(path)) {
            Ok(text) => {
                // 줄바꿈은 LF 로 다룬다 — Windows 에서 CRLF 로 받아 온 파일도 저장하면 LF 가 된다.
                let text = text.replace("\r\n", "\n");
                self.path = Some(path.to_owned());
                self.saved.clone_from(&text);
                self.text = text;
                self.status = None;
                self.compile();
            }
            Err(e) => self.status = Some((true, format!("{DATA_DIR}/{path}: {e}"))),
        }
    }

    fn create(&mut self) {
        match new_path(&self.new_name) {
            Ok(path) if self.files.contains(&path) => {
                self.status = Some((true, format!("{path} 는 이미 있음")));
            }
            Ok(path) => {
                self.path = Some(path);
                self.text = template();
                // 아직 디스크에 없다 — 저장해야 생긴다. 그래서 "저장 안 됨" 으로 시작한다.
                self.saved.clear();
                self.new_name.clear();
                self.compile();
                self.status = Some((false, String::from("새 스크립트 — 저장하면 파일이 생긴다")));
            }
            Err(e) => self.status = Some((true, e)),
        }
    }

    /// 창을 그린다. `users` 는 열린 스크립트를 쓰는 액터 이름 — 결과 줄에 보여 준다.
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        users: &dyn Fn(&str) -> Vec<String>,
    ) -> ScriptEditorActions {
        let mut actions = ScriptEditorActions::default();
        let mut open = self.open;
        egui::Window::new("스크립트 편집기")
            .open(&mut open)
            .default_size([720.0, 520.0])
            .resizable(true)
            .show(ui.ctx(), |ui| {
                self.toolbar(ui);
                self.result_line(ui, users);
                ui.separator();
                self.help(ui);
                self.code(ui, &mut actions);
            });
        self.open = open;
        actions
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let shown = self.path.clone().unwrap_or_else(|| String::from("(없음)"));
            let mut pick = None;
            egui::ComboBox::from_id_salt("script_file")
                .selected_text(&shown)
                .width(200.0)
                .show_ui(ui, |ui| {
                    for f in &self.files {
                        if ui
                            .selectable_label(Some(f) == self.path.as_ref(), f)
                            .clicked()
                        {
                            pick = Some(f.clone());
                        }
                    }
                });
            if let Some(f) = pick
                && Some(&f) != self.path.as_ref()
            {
                if self.is_dirty() {
                    // 조용히 버리지 않는다 — 먼저 저장하라고 알린다.
                    self.status = Some((
                        true,
                        String::from("저장하지 않은 변경이 있음 — 먼저 저장하세요"),
                    ));
                } else {
                    self.load(&f);
                }
            }

            ui.separator();
            ui.add(
                egui::TextEdit::singleline(&mut self.new_name)
                    .hint_text("새 이름 (예: boss)")
                    .desired_width(120.0),
            );
            if ui
                .add_enabled(
                    !self.new_name.trim().is_empty(),
                    egui::Button::new("만들기"),
                )
                .clicked()
            {
                if self.is_dirty() {
                    self.status = Some((
                        true,
                        String::from("저장하지 않은 변경이 있음 — 먼저 저장하세요"),
                    ));
                } else {
                    self.create();
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let has_file = self.path.is_some();
                let label = if self.is_dirty() {
                    "저장 *"
                } else {
                    "저장"
                };
                if ui
                    .add_enabled(has_file, egui::Button::new(label).shortcut_text("Ctrl+S"))
                    .clicked()
                {
                    let _ = self.save();
                }
                if ui
                    .add_enabled(has_file, egui::Button::new("컴파일"))
                    .clicked()
                {
                    self.compile();
                }
            });
        });
    }

    fn result_line(&self, ui: &mut egui::Ui, users: &dyn Fn(&str) -> Vec<String>) {
        const OK: egui::Color32 = egui::Color32::from_rgb(120, 210, 130);
        const BAD: egui::Color32 = egui::Color32::from_rgb(240, 110, 100);
        match &self.result {
            Some(Ok(c)) => {
                let hooks = if c.hooks.is_empty() {
                    String::from("없음 — 아무것도 하지 않는다")
                } else {
                    c.hooks.join(", ")
                };
                ui.colored_label(OK, format!("컴파일 성공 — 훅: {hooks}"));
            }
            Some(Err(d)) => {
                ui.colored_label(BAD, format!("컴파일 오류 — {d}"));
            }
            None => {
                ui.weak("고친 뒤 아직 컴파일하지 않음");
            }
        }
        if let Some(path) = &self.path {
            let who = users(path);
            if who.is_empty() {
                ui.weak(
                    "이 스크립트를 쓰는 액터 없음 — data/rules.ron 의 actors 에 script 를 적는다",
                );
            } else {
                ui.weak(format!("쓰는 액터: {}", who.join(", ")));
            }
        }
        if let Some((error, msg)) = &self.status {
            if *error {
                ui.colored_label(BAD, msg);
            } else {
                ui.weak(msg);
            }
        }
    }

    fn help(&self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("도움말 — 훅과 쓸 수 있는 함수")
            .default_open(false)
            .show(ui, |ui| {
                for sig in nexus_script::hook_signatures() {
                    ui.monospace(sig);
                }
                ui.add_space(4.0);
                for line in HELP {
                    ui.monospace(*line);
                }
            });
    }

    fn code(&mut self, ui: &mut egui::Ui, actions: &mut ScriptEditorActions) {
        if self.path.is_none() {
            ui.weak("열린 스크립트가 없습니다 — 위에서 고르거나 새로 만드세요.");
            return;
        }
        let error_line = match &self.result {
            Some(Err(d)) => d.line,
            _ => None,
        };
        // 줄바꿈하지 않는다 — 긴 줄이 접히면 옆의 줄 번호와 어긋난다. 가로 스크롤로 본다.
        let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
            let mut job = highlight(ui, buf.as_str(), error_line);
            job.wrap.max_width = f32::INFINITY;
            ui.fonts_mut(|f| f.layout_job(job))
        };
        // 줄 번호 — 같은 글꼴·여백의 읽기 전용 입력칸이라 줄 높이가 코드와 정확히 맞는다.
        let lines = self.text.split('\n').count().max(1);
        let mut numbers: String = (1..=lines).map(|n| format!("{n:>3}\n")).collect();
        numbers.pop();

        let before = self.text.clone();
        egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut numbers)
                        .font(egui::TextStyle::Monospace)
                        .interactive(false)
                        .desired_width(28.0)
                        .frame(egui::Frame::NONE),
                );
                let response = ui.add(
                    egui::TextEdit::multiline(&mut self.text)
                        .code_editor()
                        .lock_focus(true)
                        .desired_width(f32::INFINITY)
                        .desired_rows(24)
                        .frame(egui::Frame::NONE)
                        .layouter(&mut layouter),
                );
                if response.has_focus() {
                    let (save, play) = ui.input_mut(|i| {
                        (
                            i.consume_shortcut(&egui::KeyboardShortcut::new(
                                egui::Modifiers::COMMAND,
                                egui::Key::S,
                            )),
                            i.key_pressed(egui::Key::F5),
                        )
                    });
                    if save {
                        let _ = self.save();
                    }
                    actions.toggle_play |= play;
                }
            });
        });
        if self.text != before {
            // 고친 뒤의 옛 결과는 틀린 줄을 짚을 수 있다.
            self.result = None;
        }
    }
}

const HELP: &[&str] = &[
    "읽기   u.x  u.y  u.hp  u.max_hp  u.alive  u.moving  u.id",
    "       distance(a, b)          nearest_enemy(me, 거리)   enemies(me, 거리)",
    "       can_see(me, u)          attack_range(me)          time()",
    "       rand(n)  rand_float()   — 시드 있는 난수",
    "행동   move_to(me, x, y)  move_to(me, u)  stop(me)  attack(me, u)  attack(me, u, 스킬)",
    "기타   print(\"…\")  — 플레이 패널의 기록에 뜬다",
    "주의   정수·실수를 섞지 않는다: move_to(me, 1.0, 2.0) · 정수는 .to_float()",
];

/// `data/scripts` 아래 `.rhai` 파일 — `data/` 기준 경로, `/` 구분, 이름 순.
pub(crate) fn list(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    walk(dir, &mut out);
    let base = dir.parent().unwrap_or(dir);
    let mut paths: Vec<String> = out
        .iter()
        .filter_map(|p| p.strip_prefix(base).ok())
        .map(|p| {
            p.components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        })
        .collect();
    paths.sort();
    paths
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().is_some_and(|e| e == "rhai") {
            out.push(path);
        }
    }
}

/// 새 스크립트 이름 → `data/` 기준 경로. 소문자로 바꾸고 `.rhai` 를 붙인다.
pub(crate) fn new_path(input: &str) -> Result<String, String> {
    let name = input.trim().to_lowercase();
    let name = name.strip_suffix(".rhai").unwrap_or(&name);
    let path = format!("{SCRIPT_DIR}/{name}.rhai");
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        || !script_path_ok(&path)
    {
        return Err(format!(
            "'{}' — 이름은 영문 소문자·숫자·_·- 만 (예: boss_slime)",
            input.trim()
        ));
    }
    Ok(path)
}

/// `base/path` 에 저장한다. 줄바꿈은 LF, 임시 파일에 쓴 뒤 이름을 바꾼다 (존 파일과 같은 규칙).
pub(crate) fn save(base: &Path, path: &str, text: &str) -> Result<(), String> {
    if !script_path_ok(path) {
        return Err(format!("{path}: 스크립트 경로 규칙에 맞지 않음"));
    }
    let target = base.join(path);
    let fail = |e: std::io::Error| format!("{}: {e}", target.display());
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir).map_err(fail)?;
    }
    let text = text.replace("\r\n", "\n");
    let tmp = target.with_extension("rhai.tmp");
    std::fs::write(&tmp, text).map_err(fail)?;
    std::fs::rename(&tmp, &target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        fail(e)
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// 문법 강조
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Plain,
    Keyword,
    /// 훅·엔진이 준 함수.
    Api,
    Number,
    String,
    Comment,
}

const KEYWORDS: &[&str] = &[
    "fn", "let", "const", "if", "else", "while", "loop", "for", "in", "do", "until", "return",
    "break", "continue", "true", "false", "this", "switch", "throw", "try", "catch",
];

const API: &[&str] = &[
    "on_spawn",
    "on_tick",
    "on_attack",
    "on_damaged",
    "on_death",
    "distance",
    "nearest_enemy",
    "enemies",
    "can_see",
    "attack_range",
    "time",
    "rand",
    "rand_float",
    "move_to",
    "stop",
    "attack",
    "print",
];

/// 한 줄을 `(바이트 범위, 종류)` 로 자른다. 여러 줄 문자열·주석은 다루지 않는다 — 스크립트에 거의 없다.
fn tokens(line: &str) -> Vec<(std::ops::Range<usize>, Kind)> {
    let mut out = Vec::new();
    let mut chars = line.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        let end_of = |chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>| {
            chars.peek().map_or(line.len(), |&(i, _)| i)
        };
        if c == '/' && line[start..].starts_with("//") {
            out.push((start..line.len(), Kind::Comment));
            break;
        } else if c == '"' || c == '`' {
            let mut escaped = false;
            for (_, d) in chars.by_ref() {
                if escaped {
                    escaped = false;
                } else if d == '\\' {
                    escaped = true;
                } else if d == c {
                    break;
                }
            }
            out.push((start..end_of(&mut chars), Kind::String));
        } else if c.is_ascii_digit() {
            while chars
                .peek()
                .is_some_and(|&(_, d)| d.is_ascii_alphanumeric() || d == '.' || d == '_')
            {
                chars.next();
            }
            out.push((start..end_of(&mut chars), Kind::Number));
        } else if c.is_alphabetic() || c == '_' {
            while chars
                .peek()
                .is_some_and(|&(_, d)| d.is_alphanumeric() || d == '_')
            {
                chars.next();
            }
            let end = end_of(&mut chars);
            let word = &line[start..end];
            let kind = if KEYWORDS.contains(&word) {
                Kind::Keyword
            } else if API.contains(&word) {
                Kind::Api
            } else {
                Kind::Plain
            };
            out.push((start..end, kind));
        } else {
            out.push((start..end_of(&mut chars), Kind::Plain));
        }
    }
    out
}

/// 코드 전체를 색칠한다. `error_line`(1 부터)은 빨간 바탕.
fn highlight(ui: &egui::Ui, text: &str, error_line: Option<usize>) -> egui::text::LayoutJob {
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let dark = ui.visuals().dark_mode;
    let color = |k: Kind| match (k, dark) {
        (Kind::Plain, _) => ui.visuals().text_color(),
        (Kind::Keyword, true) => egui::Color32::from_rgb(200, 140, 230),
        (Kind::Keyword, false) => egui::Color32::from_rgb(140, 60, 170),
        (Kind::Api, true) => egui::Color32::from_rgb(110, 180, 240),
        (Kind::Api, false) => egui::Color32::from_rgb(30, 100, 180),
        (Kind::Number, true) => egui::Color32::from_rgb(230, 190, 120),
        (Kind::Number, false) => egui::Color32::from_rgb(160, 100, 20),
        (Kind::String, true) => egui::Color32::from_rgb(150, 210, 140),
        (Kind::String, false) => egui::Color32::from_rgb(40, 130, 50),
        (Kind::Comment, _) => ui.visuals().weak_text_color(),
    };
    let mut job = egui::text::LayoutJob::default();
    for (i, line) in text.split('\n').enumerate() {
        let background = if error_line == Some(i + 1) {
            egui::Color32::from_rgba_unmultiplied(200, 50, 40, 70)
        } else {
            egui::Color32::TRANSPARENT
        };
        if i > 0 {
            job.append(
                "\n",
                0.0,
                egui::TextFormat::simple(font.clone(), color(Kind::Plain)),
            );
        }
        for (range, kind) in tokens(line) {
            job.append(
                &line[range],
                0.0,
                egui::TextFormat {
                    font_id: font.clone(),
                    color: color(kind),
                    background,
                    italics: kind == Kind::Comment,
                    ..Default::default()
                },
            );
        }
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str) -> Vec<(&str, Kind)> {
        tokens(line)
            .into_iter()
            .filter(|(r, _)| !line[r.clone()].trim().is_empty())
            .map(|(r, k)| (&line[r], k))
            .collect()
    }

    #[test]
    fn tokens_cover_keywords_api_numbers_strings_and_comments() {
        assert_eq!(
            kinds(r#"let 목표 = nearest_enemy(me, 10.0); // 찾기"#),
            [
                ("let", Kind::Keyword),
                ("목표", Kind::Plain),
                ("=", Kind::Plain),
                ("nearest_enemy", Kind::Api),
                ("(", Kind::Plain),
                ("me", Kind::Plain),
                (",", Kind::Plain),
                ("10.0", Kind::Number),
                (")", Kind::Plain),
                (";", Kind::Plain),
                ("// 찾기", Kind::Comment),
            ]
        );
        assert_eq!(
            kinds(r#"print("a \"b\" // 아님");"#)[2],
            (r#""a \"b\" // 아님""#, Kind::String),
            "문자열 안의 // 는 주석이 아니다"
        );
    }

    #[test]
    fn tokens_cover_every_byte_of_the_line() {
        // 강조가 글자를 빠뜨리거나 겹치면 편집 중 커서가 어긋난다.
        for line in [
            "fn on_tick(me, dt) {",
            "  x += 1; // 한글 주석",
            "`${a}` + \"끝",
            "",
        ] {
            let mut at = 0;
            for (r, _) in tokens(line) {
                assert_eq!(r.start, at, "{line:?}");
                at = r.end;
            }
            assert_eq!(at, line.len(), "{line:?}");
        }
    }

    #[test]
    fn new_script_names_are_lowercase_and_safe() {
        assert_eq!(
            new_path("Boss_Slime").as_deref(),
            Ok("scripts/boss_slime.rhai")
        );
        assert_eq!(new_path(" wolf.rhai ").as_deref(), Ok("scripts/wolf.rhai"));
        for bad in ["", "../x", "a/b", "보스", "a b"] {
            assert!(new_path(bad).is_err(), "{bad:?} 를 받아들였다");
        }
    }

    #[test]
    fn the_template_compiles_with_every_hook() {
        assert_eq!(nexus_script::check(&template()).unwrap().hooks.len(), 5);
    }

    #[test]
    fn save_writes_lf_through_a_temp_file_and_list_finds_it() {
        let dir = std::env::temp_dir().join(format!("nexus-script-editor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        save(&dir, "scripts/wolf.rhai", "fn on_tick(me, dt) {\r\n}\r\n").unwrap();
        save(&dir, "scripts/pack/alpha.rhai", "").unwrap();
        let written = std::fs::read_to_string(dir.join("scripts/wolf.rhai")).unwrap();
        assert_eq!(written, "fn on_tick(me, dt) {\n}\n");
        assert!(!dir.join("scripts/wolf.rhai.tmp").exists());
        assert_eq!(
            list(&dir.join("scripts")),
            ["scripts/pack/alpha.rhai", "scripts/wolf.rhai"]
        );
        assert!(
            save(&dir, "scripts/Wolf.rhai", "").is_err(),
            "대문자 경로는 거부"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_bundled_goblin_script_compiles() {
        let src = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/scripts/goblin.rhai"),
        )
        .unwrap();
        let c = nexus_script::check(&src).unwrap();
        assert_eq!(c.hooks, ["on_spawn", "on_tick", "on_damaged", "on_death"]);
    }
}
