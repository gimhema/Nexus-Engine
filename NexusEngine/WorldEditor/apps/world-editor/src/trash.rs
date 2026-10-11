//! 휴지통 (P20) — `trash/<시각>/…` 을 보고, 되살리고, 비운다.
//!
//! 파일 작업(P9)의 삭제는 지우지 않고 여기로 옮긴다:
//! - **파일** — `trash/<시각>/원래경로` 그대로. 되살리면 원래 자리로 옮긴다 (같은 자리에 파일이 있으면 거부).
//! - **표 항목**(액터·아이템·스킬·드롭 표) — `trash/<시각>/<표>-<번호>.ron` 기록. 뺀 항목 텍스트가
//!   `// <파일> 의 <표> 에서 뺀 항목` 머리줄 아래에 들어 있다. 되살리면 그 표의 번호 순서 자리에 다시 끼운다
//!   (같은 번호가 이미 있으면 거부, 끼운 결과가 게임 데이터 검사를 통과해야 쓴다).
//!
//! 비우기(영구 삭제)는 되돌릴 수 없다 — 창이 한 번 더 묻는다.

use std::path::{Path, PathBuf};

use nexus_render_wgpu::egui;

use crate::asset_ops::TRASH_DIR;
use crate::game_data::{DISPLAY_PATH, GameData, RULES_PATH, write_tables};

/// 휴지통 한 칸 — 삭제 한 번(같은 시각)에 옮긴 것들.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Bin {
    /// `trash/<시각>` — 작업 디렉터리 기준, `/` 구분.
    pub(crate) dir: String,
    /// 유닉스 시각 (초).
    pub(crate) stamp: u64,
    pub(crate) entries: Vec<Entry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Entry {
    /// 옮겨 둔 파일 — 원래 경로 (`levels/town.level.ron`).
    File(String),
    /// 표에서 뺀 항목의 기록 — 기록 파일 이름 (`actor-150.ron`).
    Table(String),
}

impl Entry {
    pub(crate) fn label(&self) -> &str {
        match self {
            Self::File(p) | Self::Table(p) => p,
        }
    }
}

/// 휴지통을 훑는다 — 최근 것이 먼저. 폴더가 없으면 빈 목록.
pub(crate) fn scan(root: &Path) -> Vec<Bin> {
    let Ok(dirs) = std::fs::read_dir(root.join(TRASH_DIR)) else {
        return Vec::new();
    };
    let mut bins: Vec<Bin> = dirs
        .flatten()
        .filter(|d| d.path().is_dir())
        .filter_map(|d| {
            let name = d.file_name().to_string_lossy().into_owned();
            let stamp = name.parse().ok()?;
            let dir = format!("{TRASH_DIR}/{name}");
            let mut files = Vec::new();
            walk(&d.path(), "", &mut files);
            files.sort();
            let entries = files
                .into_iter()
                .map(|f| {
                    // 바로 아래의 `.ron` 파일은 표 항목 기록, 폴더 안의 것은 옮긴 파일이다.
                    if f.contains('/') {
                        Entry::File(f)
                    } else {
                        Entry::Table(f)
                    }
                })
                .collect();
            Some(Bin {
                dir,
                stamp,
                entries,
            })
        })
        .collect();
    bins.sort_by_key(|b| std::cmp::Reverse(b.stamp));
    bins
}

fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for e in read.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if e.path().is_dir() {
            walk(&e.path(), &rel, out);
        } else {
            out.push(rel);
        }
    }
}

/// 되살린 결과 — 에디터가 무엇을 다시 읽을지.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Restored {
    pub(crate) data: bool,
    pub(crate) levels: bool,
    pub(crate) screens: bool,
}

/// 하나를 되살린다. 성공하면 그 칸에서 빠지고, 칸이 비면 칸 폴더도 지운다.
pub(crate) fn restore(root: &Path, bin: &Bin, entry: &Entry) -> Result<Restored, String> {
    let mut done = Restored::default();
    match entry {
        Entry::File(original) => {
            let dest = root.join(original);
            if dest.exists() {
                return Err(format!(
                    "{original} 자리에 이미 파일이 있습니다 — 그 파일을 옮기거나 이름을 바꾼 뒤 되살리세요"
                ));
            }
            if let Some(dir) = dest.parent() {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
            let from = root.join(&bin.dir).join(original);
            std::fs::rename(&from, &dest).map_err(|e| format!("{original}: {e}"))?;
            done.levels = original.starts_with("levels/");
            done.screens = original.starts_with("ui/");
            done.data = original.starts_with("data/");
        }
        Entry::Table(note) => {
            let path = root.join(&bin.dir).join(note);
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{note}: {e}"))?;
            let read =
                |p: &str| std::fs::read_to_string(root.join(p)).map_err(|e| format!("{p}: {e}"));
            let (mut rules, mut display) = (read(RULES_PATH)?, read(DISPLAY_PATH)?);
            for part in note_parts(&text)? {
                let target = if part.file == RULES_PATH {
                    &mut rules
                } else if part.file == DISPLAY_PATH {
                    &mut display
                } else {
                    return Err(format!("{note}: 모르는 파일 {}", part.file));
                };
                if crate::ron_patch::remove_entry(target, &part.map, part.id).is_ok() {
                    return Err(format!(
                        "{} 의 {} 에 #{} 이(가) 이미 있습니다 — 같은 번호를 쓰는 항목을 먼저 지우거나 번호를 바꾸세요",
                        part.file, part.map, part.id
                    ));
                }
                *target = crate::ron_patch::upsert_entry(target, &part.map, part.id, &part.entry)?;
            }
            GameData::parse(&rules, &display)?;
            write_tables(root, &rules, &display)?;
            std::fs::remove_file(&path).map_err(|e| format!("{note}: {e}"))?;
            done.data = true;
        }
    }
    prune(&root.join(&bin.dir));
    Ok(done)
}

/// 비워진 폴더를 지운다 — 칸 폴더까지 (안쪽부터).
fn prune(dir: &Path) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for e in read.flatten() {
        if e.path().is_dir() {
            prune(&e.path());
        }
    }
    let _ = std::fs::remove_dir(dir); // 비어 있지 않으면 실패한다 — 그게 맞다.
}

/// 영구 삭제 — 칸 하나(`Some`) 또는 휴지통 전체(`None`).
pub(crate) fn purge(root: &Path, bin: Option<&Bin>) -> Result<(), String> {
    let dir: PathBuf = match bin {
        Some(b) => root.join(&b.dir),
        None => root.join(TRASH_DIR),
    };
    if !dir.exists() {
        return Ok(());
    }
    std::fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))
}

/// 표 항목 기록의 한 반쪽.
#[derive(Debug, PartialEq, Eq)]
struct NotePart {
    file: String,
    map: String,
    id: u32,
    /// 들여쓰기를 걷어 낸 항목 텍스트 (위의 주석 포함).
    entry: String,
}

/// 기록을 반쪽들로 나눈다 — `// <파일> 의 <표> 에서 뺀 항목` 머리줄마다 하나.
fn note_parts(text: &str) -> Result<Vec<NotePart>, String> {
    let mut parts: Vec<(String, String, Vec<&str>)> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("// ")
            && let Some((file, rest)) = rest.split_once(" 의 ")
            && let Some(map) = rest.strip_suffix(" 에서 뺀 항목")
        {
            parts.push((file.to_owned(), map.to_owned(), Vec::new()));
        } else if let Some((_, _, body)) = parts.last_mut() {
            body.push(line);
        }
    }
    if parts.is_empty() {
        return Err(String::from("기록의 모양을 알 수 없음 (머리줄이 없음)"));
    }
    parts
        .into_iter()
        .map(|(file, map, body)| {
            let entry = crate::ron_patch::dedent(&body.join("\n"));
            let id = entry
                .lines()
                .find(|l| !l.trim_start().starts_with("//") && !l.trim().is_empty())
                .and_then(|l| l.split(':').next())
                .and_then(|n| n.trim().parse().ok())
                .ok_or_else(|| format!("{file} 의 {map}: 항목 번호를 읽지 못함"))?;
            Ok(NotePart {
                file,
                map,
                id,
                entry,
            })
        })
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// 창
// ─────────────────────────────────────────────────────────────────────────────

/// 휴지통 창. 열 때·작업한 뒤에 다시 훑는다.
#[derive(Debug, Default)]
pub(crate) struct TrashWindow {
    open: bool,
    bins: Vec<Bin>,
    /// 영구 삭제 확인 — `Some(None)` 이면 전체, `Some(Some(i))` 면 그 칸.
    confirm: Option<Option<usize>>,
    status: Option<(bool, String)>,
    /// 되살렸다 — 에디터가 다시 읽을 것.
    changed: Option<Restored>,
}

impl TrashWindow {
    pub(crate) fn open(&mut self) {
        self.open = true;
        self.bins = scan(Path::new("."));
        self.confirm = None;
    }

    /// 되살린 것이 있으면 한 번 돌려준다.
    pub(crate) fn take_changed(&mut self) -> Option<Restored> {
        self.changed.take()
    }

    pub(crate) fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        egui::Window::new("휴지통")
            .id(egui::Id::new("trash_window"))
            .open(&mut open)
            .default_size([460.0, 380.0])
            .resizable(true)
            .show(ctx, |ui| self.body(ui));
        self.open = open;
    }

    fn body(&mut self, ui: &mut egui::Ui) {
        let root = Path::new(".");
        ui.weak("삭제한 리소스는 여기 남습니다. 되살리면 원래 자리로 돌아가고, 표 항목은 표에 다시 들어갑니다.");
        ui.horizontal(|ui| {
            if ui.button("새로 고침").clicked() {
                self.bins = scan(root);
            }
            if ui
                .add_enabled(!self.bins.is_empty(), egui::Button::new("휴지통 비우기…"))
                .clicked()
            {
                self.confirm = Some(None);
            }
        });
        if let Some(target) = self.confirm {
            let what = match target {
                None => String::from("휴지통 전체"),
                Some(i) => self.bins.get(i).map_or_else(String::new, |b| when(b.stamp)),
            };
            ui.colored_label(
                egui::Color32::from_rgb(240, 170, 90),
                format!("{what} 을(를) 영구 삭제합니다 — 되돌릴 수 없습니다."),
            );
            ui.horizontal(|ui| {
                if ui.button("영구 삭제").clicked() {
                    let bin = target.and_then(|i| self.bins.get(i));
                    self.status = Some(match purge(root, bin) {
                        Ok(()) => (false, format!("{what} 을(를) 비웠습니다")),
                        Err(e) => (true, e),
                    });
                    self.confirm = None;
                    self.bins = scan(root);
                }
                if ui.button("취소").clicked() {
                    self.confirm = None;
                }
            });
        }
        ui.separator();

        let mut action: Option<(usize, Option<Entry>)> = None; // (칸, 되살릴 것 / None = 칸 비우기)
        egui::ScrollArea::vertical()
            .max_height(ui.available_height() - 30.0)
            .show(ui, |ui| {
                if self.bins.is_empty() {
                    ui.weak("휴지통이 비어 있습니다.");
                }
                for (i, bin) in self.bins.iter().enumerate() {
                    egui::CollapsingHeader::new(format!(
                        "{} — {}개",
                        when(bin.stamp),
                        bin.entries.len()
                    ))
                    .id_salt(&bin.dir)
                    .default_open(i == 0)
                    .show(ui, |ui| {
                        for entry in &bin.entries {
                            ui.horizontal(|ui| {
                                let kind = match entry {
                                    Entry::File(_) => "파일",
                                    Entry::Table(_) => "표 항목",
                                };
                                ui.weak(kind);
                                ui.monospace(entry.label());
                                if ui.small_button("되살리기").clicked() {
                                    action = Some((i, Some(entry.clone())));
                                }
                            });
                        }
                        if ui.small_button("이 칸 영구 삭제…").clicked() {
                            action = Some((i, None));
                        }
                    });
                }
            });
        match action {
            Some((i, Some(entry))) => {
                let bin = self.bins[i].clone();
                self.status = Some(match restore(root, &bin, &entry) {
                    Ok(done) => {
                        let mut all = self.changed.unwrap_or_default();
                        all.data |= done.data;
                        all.levels |= done.levels;
                        all.screens |= done.screens;
                        self.changed = Some(all);
                        (false, format!("되살렸습니다 — {}", entry.label()))
                    }
                    Err(e) => (true, format!("되살리지 않았습니다 — {e}")),
                });
                self.bins = scan(root);
            }
            Some((i, None)) => self.confirm = Some(Some(i)),
            None => {}
        }
        if let Some((error, text)) = &self.status {
            let color = if *error {
                egui::Color32::from_rgb(240, 110, 100)
            } else {
                egui::Color32::from_rgb(140, 210, 140)
            };
            ui.colored_label(color, text);
        }
    }
}

/// 유닉스 시각을 사람이 읽는 꼴로 — 지역 시간대를 모르므로 UTC 로, 그렇다고 적는다.
fn when(stamp: u64) -> String {
    let days = stamp / 86_400;
    let secs = stamp % 86_400;
    // 1970-01-01 부터 날짜 (그레고리력, Howard Hinnant 의 civil_from_days).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y}-{m:02}-{d:02} {:02}:{:02} UTC",
        secs / 3600,
        secs % 3600 / 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_read_as_utc_dates() {
        assert_eq!(when(0), "1970-01-01 00:00 UTC");
        assert_eq!(
            when(1_791_763_200 + 3_600 * 9 + 60 * 5),
            "2026-10-12 09:05 UTC"
        );
    }

    #[test]
    fn a_table_note_splits_into_its_halves() {
        let note = "// data/rules.ron 의 actors 에서 뺀 항목\n        // 설명\n        150: (\n            max_hp: 80,\n        ),\n\n// data/display.ron 의 actors 에서 뺀 항목\n        150: (\n            name: \"슬라임 복사본\",\n        ),\n";
        let parts = note_parts(note).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(
            parts[0],
            NotePart {
                file: String::from("data/rules.ron"),
                map: String::from("actors"),
                id: 150,
                entry: String::from("// 설명\n150: (\n    max_hp: 80,\n),"),
            }
        );
        assert_eq!(parts[1].file, "data/display.ron");
        assert!(note_parts("아무 말").is_err());
    }
}
