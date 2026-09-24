//! 파일 작업 — 콘텐츠 브라우저의 삭제·복제·이름 바꾸기 (단계 2 P9).
//!
//! 작업은 두 단계다. **[`plan`] 이 할 일을 전부 계산하고 검증**한 뒤, 확인 창에서 사람이 보고
//! 누르면 **[`apply`] 가 그대로 실행**한다. 계산 단계는 디스크를 읽기만 한다.
//!
//! | 작업 | 규칙 |
//! |---|---|
//! | 삭제 | **휴지통**(`trash/<시각>/원래경로`)으로 옮긴다 — 지우지 않는다. 다른 곳이 쓰고 있으면 거부 |
//! | 복제 | 새 번호로 복사. 같은 이름이 있으면 거부 |
//! | 이름 바꾸기 | 파일을 옮기고 **이 리소스를 가리키는 곳을 전부 고쳐 쓴다** |
//!
//! - **고쳐 쓴 파일은 전부 게임과 같은 검사를 다시 통과해야** 계획이 선다 — 하나라도 깨지면
//!   아무것도 쓰지 않는다.
//! - **내장 리소스**(hud·main_menu·main 레벨·goblin 스크립트 …)는 지우거나 이름을 바꾸지 않는다.
//!   디스크 파일을 지워도 실행 파일 안의 내장본으로 되살아나서, 사용자가 본 것과 게임이 어긋난다.
//! - 액터는 파일이 아니라 **표의 항목**이다 — 삭제하면 두 표에서 빼고, 뺀 텍스트를 휴지통에
//!   기록 파일로 남긴다. 번호 바꾸기는 하지 않는다 (이름은 액터 편집기에서 바꾼다).
//! - 참조를 찾고 고치는 것은 **문자열 치환**이다 (`OpenLevel("old")` → `OpenLevel("new")` 처럼
//!   따옴표까지 포함해 바꾼다 — 이름의 일부가 겹쳐도 잘못 바뀌지 않게).

use std::path::{Path, PathBuf};

use crate::content::{Asset, AssetKind, scan_files};
use crate::game_data::{
    DISPLAY_PATH, GameData, RULES_PATH, actor_forms, default_actor_ids, is_builtin_script,
};

/// 휴지통 폴더 (작업 디렉터리 기준, git 무시).
pub(crate) const TRASH_DIR: &str = "trash";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OpKind {
    Delete,
    Duplicate,
    Rename,
}

impl OpKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Delete => "삭제",
            Self::Duplicate => "복제",
            Self::Rename => "이름 바꾸기",
        }
    }
}

/// 사람이 고른 작업 — 확인 창이 이것으로 계획을 세운다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileOp {
    pub(crate) kind: OpKind,
    pub(crate) asset: AssetRef,
    /// 새 번호 — 파일이면 확장자 없는 이름(`village2`), 액터면 번호(`150`). 삭제면 비어 있다.
    pub(crate) target: String,
}

/// 작업 대상 — [`Asset`] 에서 필요한 것만 (확인 창·에디터 사이를 오간다).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AssetRef {
    pub(crate) kind: AssetKind,
    pub(crate) key: String,
    pub(crate) name: String,
}

impl From<&Asset> for AssetRef {
    fn from(a: &Asset) -> Self {
        Self {
            kind: a.kind,
            key: a.key.clone(),
            name: a.name.clone(),
        }
    }
}

/// 이 종류에 이 작업을 할 수 있는가 — 메뉴를 켜고 끄는 데 쓴다 (내장·참조 검사는 [`plan`] 이 한다).
pub(crate) fn supports(kind: AssetKind, op: OpKind) -> bool {
    match kind {
        AssetKind::Level | AssetKind::Zone | AssetKind::Screen | AssetKind::Script => true,
        AssetKind::Actor => op != OpKind::Rename,
        AssetKind::Item
        | AssetKind::Skill
        | AssetKind::Loot
        | AssetKind::Sheet
        | AssetKind::Image
        | AssetKind::Data => false,
    }
}

/// 실행할 일 전부 — 계산을 마친 결과.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Plan {
    /// 사람이 읽는 할 일 목록 (확인 창).
    pub(crate) steps: Vec<String>,
    /// 새로 만들거나 고쳐 쓸 파일 — `(경로, 내용)`.
    writes: Vec<(String, String)>,
    /// 옮길 파일 — `(원래, 새)`. 이름 바꾸기.
    moves: Vec<(String, String)>,
    /// 휴지통으로 옮길 파일.
    trash: Vec<String>,
    /// 휴지통에 남길 기록 — 파일이 아닌 것(액터 항목)을 되살릴 수 있게. `(이름, 내용)`.
    notes: Vec<(String, String)>,
    /// 에디터가 다시 읽어야 할 것.
    pub(crate) touched: Touched,
}

/// 작업 뒤 에디터가 다시 읽어야 할 것.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Touched {
    pub(crate) levels: bool,
    pub(crate) screens: bool,
    pub(crate) data: bool,
    /// 이름 바꾸기로 옮겨진 파일 `(원래, 새)` — 에디터가 열어 둔 존이면 경로를 따라가고,
    /// 브라우저는 새 자리를 고른다.
    pub(crate) moved: Option<(String, String)>,
}

/// 할 일을 계산하고 검증한다. 할 수 없으면 이유를 돌려준다 (아무것도 쓰지 않는다).
pub(crate) fn plan(root: &Path, op: &FileOp) -> Result<Plan, String> {
    if !supports(op.asset.kind, op.kind) {
        return Err(match op.asset.kind {
            AssetKind::Actor => {
                String::from("액터의 이름은 액터 편집기에서 바꿉니다 (번호는 바꾸지 않습니다)")
            }
            _ => format!(
                "{} 은(는) 아직 파일 작업을 하지 않습니다 (받아온 애셋 팩·데이터 표)",
                op.asset.kind.label()
            ),
        });
    }
    match op.asset.kind {
        AssetKind::Actor => plan_actor(root, op),
        _ => plan_file(root, op),
    }
}

/// 파일 리소스 (레벨·맵·화면·스크립트).
fn plan_file(root: &Path, op: &FileOp) -> Result<Plan, String> {
    let spec = Spec::of(op.asset.kind, &op.asset.key)?;
    let mut p = Plan::default();
    p.touched.levels = matches!(
        op.asset.kind,
        AssetKind::Level | AssetKind::Zone | AssetKind::Screen
    );
    p.touched.screens = matches!(op.asset.kind, AssetKind::Screen | AssetKind::Level);
    p.touched.data = op.asset.kind == AssetKind::Script;

    if op.kind != OpKind::Duplicate && spec.builtin {
        return Err(format!(
            "'{}' 은(는) 실행 파일에 내장된 리소스입니다 — 파일을 지우거나 옮겨도 내장본으로 되살아나므로 \
             {}하지 않습니다 (복제는 됩니다)",
            spec.id,
            op.kind.label()
        ));
    }

    // 이 리소스를 가리키는 곳 — (파일, 원래 문자열, 바꿀 문자열).
    let target = spec.target_for(&op.target);
    let refs = references(root, &spec, target.as_ref())?;

    match op.kind {
        OpKind::Delete => {
            if !refs.is_empty() {
                let who: Vec<String> = refs.iter().map(|r| r.file.clone()).collect();
                return Err(format!(
                    "다른 곳이 쓰고 있어 지우지 않습니다 — {}",
                    dedup(who).join(", ")
                ));
            }
            p.trash.push(op.asset.key.clone());
            p.steps.push(format!("휴지통으로: {}", op.asset.key));
        }
        OpKind::Duplicate | OpKind::Rename => {
            let t = target.ok_or_else(|| {
                if op.target.trim().is_empty() {
                    String::from("새 이름을 적으세요")
                } else {
                    format!(
                        "'{}' — 이름은 영문 소문자·숫자·_·- 만 (파일 이름이 됩니다)",
                        op.target.trim()
                    )
                }
            })?;
            let new_path = spec.path_for(&t.id);
            if root.join(&new_path).exists() {
                return Err(format!("{new_path} 이 이미 있습니다"));
            }
            if op.kind == OpKind::Duplicate {
                let text = std::fs::read_to_string(root.join(&op.asset.key))
                    .map_err(|e| format!("{}: {e}", op.asset.key))?;
                // 이름표도 바꿔 둔다 — 목록에서 두 개가 같은 이름으로 보이지 않게.
                let text = spec.rename_inside(&text, &t.id);
                spec.validate(&text)?;
                p.steps
                    .push(format!("새 파일: {new_path} ({} 복사)", op.asset.key));
                p.writes.push((new_path, text));
            } else {
                p.moves.push((op.asset.key.clone(), new_path.clone()));
                p.steps.push(format!("옮김: {} → {new_path}", op.asset.key));
                // 참조 고쳐 쓰기 — 파일마다 모아서 한 번에.
                let mut files: Vec<String> = refs.iter().map(|r| r.file.clone()).collect();
                files = dedup(files);
                for file in files {
                    let mut text = std::fs::read_to_string(root.join(&file))
                        .map_err(|e| format!("{file}: {e}"))?;
                    for r in refs.iter().filter(|r| r.file == file) {
                        text = text.replace(&r.from, &r.to);
                    }
                    validate_any(root, &file, &text)?;
                    p.steps.push(format!("고쳐 씀: {file}"));
                    p.writes.push((file, text));
                }
                p.touched.moved = Some((op.asset.key.clone(), new_path));
            }
        }
    }
    Ok(p)
}

/// 액터 — 표의 항목.
fn plan_actor(root: &Path, op: &FileOp) -> Result<Plan, String> {
    let id: u32 = op
        .asset
        .key
        .strip_prefix("actor:")
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| format!("'{}' 는 액터가 아닙니다", op.asset.key))?;
    let rules = read(root, RULES_PATH)?;
    let display = read(root, DISPLAY_PATH)?;
    let mut p = Plan {
        touched: Touched {
            data: true,
            ..Touched::default()
        },
        ..Plan::default()
    };
    match op.kind {
        OpKind::Delete => {
            if default_actor_ids(&rules)?.contains(&id) {
                return Err(format!(
                    "#{id} 은(는) 마커 종류의 기본 액터입니다 (rules.ron 의 default_actors) — 지우지 않습니다"
                ));
            }
            let needle = format!("actor: {id},");
            let users: Vec<String> = scan_files(root, "zones", ".zone.ron")
                .into_iter()
                .filter(|z| read(root, z).is_ok_and(|t| t.contains(&needle)))
                .collect();
            if !users.is_empty() {
                return Err(format!(
                    "이 액터를 쓰는 마커가 있어 지우지 않습니다 — {}",
                    users.join(", ")
                ));
            }
            let (rules2, gone_rules) = crate::ron_patch::remove_entry(&rules, "actors", id)?;
            let (display2, gone_display) = crate::ron_patch::remove_entry(&display, "actors", id)?;
            GameData::parse(&rules2, &display2)?;
            p.steps
                .push(format!("#{id} {} 를 두 표에서 뺌", op.asset.name));
            p.steps
                .push(String::from("뺀 항목은 휴지통에 기록으로 남김"));
            p.writes.push((RULES_PATH.to_owned(), rules2));
            p.writes.push((DISPLAY_PATH.to_owned(), display2));
            p.notes.push((
                format!("actor-{id}.ron"),
                format!(
                    "// {RULES_PATH} 의 actors 에서 뺀 항목\n{gone_rules}\n\n// {DISPLAY_PATH} 의 actors 에서 뺀 항목\n{gone_display}\n"
                ),
            ));
        }
        OpKind::Duplicate => {
            let new_id: u32 = op
                .target
                .trim()
                .parse()
                .map_err(|_| String::from("새 번호를 숫자로 적으세요"))?;
            let forms = actor_forms(&rules, &display)?;
            if forms.contains_key(&new_id) {
                return Err(format!("#{new_id} 은(는) 이미 있습니다"));
            }
            let mut form = forms
                .get(&id)
                .cloned()
                .ok_or_else(|| format!("#{id} 액터가 없습니다"))?;
            form.name = format!("{} 복사본", form.name);
            let rules2 = crate::ron_patch::upsert_entry(
                &rules,
                "actors",
                new_id,
                &form.rules_entry(new_id),
            )?;
            let display2 = crate::ron_patch::upsert_entry(
                &display,
                "actors",
                new_id,
                &form.display_entry(new_id),
            )?;
            GameData::parse(&rules2, &display2)?;
            p.steps.push(format!("새 액터: #{new_id} {}", form.name));
            p.writes.push((RULES_PATH.to_owned(), rules2));
            p.writes.push((DISPLAY_PATH.to_owned(), display2));
        }
        OpKind::Rename => unreachable!("supports() 가 막는다"),
    }
    Ok(p)
}

/// 계획을 실행한다. 휴지통을 썼으면 그 폴더를 돌려준다.
///
/// 쓸 파일은 **전부 임시 파일에 먼저 쓴 뒤** 이름을 바꾼다 — 도중에 멈춰도 반쯤 덮인 파일이
/// 남지 않게. 그다음 옮기기·휴지통 순서다.
pub(crate) fn apply(root: &Path, plan: &Plan) -> Result<Option<String>, String> {
    let io = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    let mut temps: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (path, text) in &plan.writes {
        let dest = root.join(path);
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
        }
        let tmp = dest.with_extension("op.tmp");
        if let Err(e) = std::fs::write(&tmp, text) {
            for (t, _) in &temps {
                let _ = std::fs::remove_file(t);
            }
            return Err(io(&tmp, e));
        }
        temps.push((tmp, dest));
    }
    for (tmp, dest) in &temps {
        std::fs::rename(tmp, dest).map_err(|e| io(dest, e))?;
    }
    for (from, to) in &plan.moves {
        let dest = root.join(to);
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
        }
        std::fs::rename(root.join(from), &dest).map_err(|e| io(&dest, e))?;
    }
    if plan.trash.is_empty() && plan.notes.is_empty() {
        return Ok(None);
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let bin = format!("{TRASH_DIR}/{stamp}");
    for path in &plan.trash {
        let dest = root.join(&bin).join(path);
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
        }
        std::fs::rename(root.join(path), &dest).map_err(|e| io(&dest, e))?;
    }
    for (name, text) in &plan.notes {
        let dest = root.join(&bin).join(name);
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
        }
        std::fs::write(&dest, text).map_err(|e| io(&dest, e))?;
    }
    Ok(Some(bin))
}

// ─────────────────────────────────────────────────────────────────────────────
// 종류별 규칙
// ─────────────────────────────────────────────────────────────────────────────

/// 파일 리소스 한 종류의 규칙 — 번호·경로·내장 여부·참조 모양.
struct Spec {
    kind: AssetKind,
    /// 번호 — 레벨·화면·맵은 파일 이름(확장자 없이), 스크립트는 `data/` 기준 경로.
    id: String,
    builtin: bool,
}

/// 새 이름으로 계산한 것.
struct Target {
    id: String,
}

/// 참조 한 곳.
struct Ref {
    file: String,
    from: String,
    to: String,
}

impl Spec {
    fn of(kind: AssetKind, key: &str) -> Result<Self, String> {
        let file = key.rsplit('/').next().unwrap_or(key);
        let (id, builtin) = match kind {
            AssetKind::Level => {
                let id = file.strip_suffix(".level.ron").unwrap_or(file).to_owned();
                let b = crate::level::is_builtin(&id);
                (id, b)
            }
            AssetKind::Screen => {
                let id = file.strip_suffix(".ui.ron").unwrap_or(file).to_owned();
                let b = crate::screen::is_builtin(&id);
                (id, b)
            }
            AssetKind::Zone => (
                file.strip_suffix(".zone.ron").unwrap_or(file).to_owned(),
                false,
            ),
            AssetKind::Script => {
                let rel = key.strip_prefix("data/").unwrap_or(key).to_owned();
                let b = is_builtin_script(&rel);
                (rel, b)
            }
            _ => return Err(format!("{} 은(는) 파일 작업 대상이 아닙니다", kind.label())),
        };
        Ok(Self { kind, id, builtin })
    }

    /// 사람이 적은 새 이름 → 번호. 규칙에 맞지 않으면 `None` (확인 창이 이유를 보여 준다).
    fn target_for(&self, input: &str) -> Option<Target> {
        let input = input.trim().to_ascii_lowercase();
        if input.is_empty() {
            return None;
        }
        match self.kind {
            AssetKind::Script => crate::script_editor::new_path(&input)
                .ok()
                .map(|id| Target { id }),
            _ => crate::screen::valid_screen_id(&input).then_some(Target { id: input }),
        }
    }

    /// 번호 → 파일 경로 (작업 디렉터리 기준).
    fn path_for(&self, id: &str) -> String {
        match self.kind {
            AssetKind::Level => format!("levels/{id}.level.ron"),
            AssetKind::Screen => format!("ui/{id}.ui.ron"),
            AssetKind::Zone => format!("zones/{id}.zone.ron"),
            AssetKind::Script => format!("data/{id}"),
            _ => String::new(),
        }
    }

    /// 이 리소스를 가리키는 문자열 모양 — `(찾을 파일들의 폴더·확장자, 원래, 새)`.
    fn needles(&self, new: Option<&str>) -> Vec<(&'static str, &'static str, String, String)> {
        let new = new.unwrap_or("");
        let id = &self.id;
        match self.kind {
            AssetKind::Level => vec![
                (
                    "ui",
                    ".ui.ron",
                    format!("OpenLevel(\"{id}\")"),
                    format!("OpenLevel(\"{new}\")"),
                ),
                (
                    "data",
                    "project.ron",
                    format!("startup_level: \"{id}\""),
                    format!("startup_level: \"{new}\""),
                ),
            ],
            AssetKind::Zone => vec![(
                "levels",
                ".level.ron",
                format!("\"zones/{id}.zone.ron\""),
                format!("\"zones/{new}.zone.ron\""),
            )],
            AssetKind::Screen => vec![
                (
                    "levels",
                    ".level.ron",
                    format!("Some(\"{id}\")"),
                    format!("Some(\"{new}\")"),
                ),
                (
                    "ui",
                    ".ui.ron",
                    format!("OpenScreen(\"{id}\")"),
                    format!("OpenScreen(\"{new}\")"),
                ),
            ],
            AssetKind::Script => vec![(
                "data",
                "rules.ron",
                format!("Some(\"{id}\")"),
                format!("Some(\"{new}\")"),
            )],
            _ => Vec::new(),
        }
    }

    /// 복제본 안의 이름표 — 레벨·화면은 `name` 을 새 번호로 (목록에서 구별되게).
    fn rename_inside(&self, text: &str, new_id: &str) -> String {
        match self.kind {
            AssetKind::Level | AssetKind::Screen => {
                crate::ron_patch::replace_field(text, "name", &crate::ron_patch::quote(new_id))
                    .unwrap_or_else(|_| text.to_owned())
            }
            _ => text.to_owned(),
        }
    }

    /// 이 종류의 파일로 읽히는가.
    fn validate(&self, text: &str) -> Result<(), String> {
        match self.kind {
            AssetKind::Level => crate::level::read_level(text).map(|_| ()),
            AssetKind::Screen => crate::screen::Screens::read_screen(text).map(|_| ()),
            AssetKind::Zone => crate::zone_file::from_ron(text).map(|_| ()),
            AssetKind::Script => nexus_script::check(text)
                .map(|_| ())
                .map_err(|e| e.to_string()),
            _ => Ok(()),
        }
    }
}

/// 이 리소스를 가리키는 곳을 모두 찾는다.
fn references(root: &Path, spec: &Spec, target: Option<&Target>) -> Result<Vec<Ref>, String> {
    let mut out = Vec::new();
    for (dir, suffix, from, to) in spec.needles(target.map(|t| t.id.as_str())) {
        let files: Vec<String> = if suffix.starts_with('.') {
            scan_files(root, dir, suffix)
        } else {
            // 한 파일짜리 (data/project.ron 등).
            let path = format!("{dir}/{suffix}");
            if root.join(&path).exists() {
                vec![path]
            } else {
                Vec::new()
            }
        };
        for file in files {
            if read(root, &file)?.contains(&from) {
                out.push(Ref {
                    file,
                    from: from.clone(),
                    to: to.clone(),
                });
            }
        }
    }
    Ok(out)
}

/// 고쳐 쓴 파일이 여전히 그 종류로 읽히는지 — 경로로 종류를 안다.
fn validate_any(root: &Path, path: &str, text: &str) -> Result<(), String> {
    let r = if path.ends_with(".level.ron") {
        crate::level::read_level(text).map(|_| ())
    } else if path.ends_with(".ui.ron") {
        crate::screen::Screens::read_screen(text).map(|_| ())
    } else if path == crate::level::PROJECT_PATH {
        crate::level::read_project(text).map(|_| ())
    } else if path == RULES_PATH {
        GameData::parse(text, &read(root, DISPLAY_PATH)?).map(|_| ())
    } else {
        Ok(())
    };
    r.map_err(|e| format!("{path} 을 고치면 읽을 수 없게 됨 — {e}"))
}

fn read(root: &Path, path: &str) -> Result<String, String> {
    std::fs::read_to_string(root.join(path)).map_err(|e| format!("{path}: {e}"))
}

fn dedup(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 저장소의 데이터를 임시 폴더에 복사한 작업장 — 테스트가 진짜 파일을 건드리지 않게.
    struct Sandbox(PathBuf);

    impl Sandbox {
        fn new(name: &str) -> Self {
            let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            let dir = std::env::temp_dir().join(format!("nexus-ops-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            for (folder, suffix) in [
                ("levels", ".level.ron"),
                ("ui", ".ui.ron"),
                ("zones", ".zone.ron"),
                ("data", ".ron"),
                ("data/scripts", ".rhai"),
            ] {
                for path in scan_files(&repo, folder, suffix) {
                    let dest = dir.join(&path);
                    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
                    std::fs::copy(repo.join(&path), dest).unwrap();
                }
            }
            Self(dir)
        }

        fn read(&self, path: &str) -> String {
            std::fs::read_to_string(self.0.join(path)).unwrap()
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn op(kind: OpKind, asset: AssetKind, key: &str, target: &str) -> FileOp {
        FileOp {
            kind,
            asset: AssetRef {
                kind: asset,
                key: key.to_owned(),
                name: key.to_owned(),
            },
            target: target.to_owned(),
        }
    }

    #[test]
    fn renaming_a_zone_rewrites_the_levels_that_use_it() {
        let sb = Sandbox::new("zone");
        let o = op(
            OpKind::Rename,
            AssetKind::Zone,
            "zones/village.zone.ron",
            "town",
        );
        let plan = plan(&sb.0, &o).unwrap();
        assert!(
            plan.steps
                .iter()
                .any(|s| s.contains("levels/village.level.ron"))
        );
        apply(&sb.0, &plan).unwrap();
        assert!(sb.0.join("zones/town.zone.ron").exists());
        assert!(!sb.0.join("zones/village.zone.ron").exists());
        let level = crate::level::read_level(&sb.read("levels/village.level.ron")).unwrap();
        assert_eq!(level.zone.as_deref(), Some("zones/town.zone.ron"));
        assert_eq!(
            plan.touched.moved,
            Some((
                String::from("zones/village.zone.ron"),
                String::from("zones/town.zone.ron")
            ))
        );
    }

    #[test]
    fn renaming_a_level_rewrites_buttons_and_the_startup_level() {
        let sb = Sandbox::new("level");
        // 내장 레벨은 옮기지 않는다 — 복제로 사용자 레벨을 만든 뒤 그것을 옮긴다.
        let dup = op(
            OpKind::Duplicate,
            AssetKind::Level,
            "levels/village.level.ron",
            "town",
        );
        apply(&sb.0, &plan(&sb.0, &dup).unwrap()).unwrap();
        let copy = crate::level::read_level(&sb.read("levels/town.level.ron")).unwrap();
        assert_eq!(copy.name, "town", "복제본의 이름표도 바뀐다");

        // 메인 메뉴 버튼과 시작 레벨이 town 을 가리키게 해 둔다.
        let menu = sb
            .read("ui/main_menu.ui.ron")
            .replace("OpenLevel(\"village\")", "OpenLevel(\"town\")");
        std::fs::write(sb.0.join("ui/main_menu.ui.ron"), menu).unwrap();
        let project = crate::level::patch_startup(&sb.read("data/project.ron"), "town").unwrap();
        std::fs::write(sb.0.join("data/project.ron"), project).unwrap();

        let ren = op(
            OpKind::Rename,
            AssetKind::Level,
            "levels/town.level.ron",
            "harbor",
        );
        apply(&sb.0, &plan(&sb.0, &ren).unwrap()).unwrap();
        assert!(
            sb.read("ui/main_menu.ui.ron")
                .contains("OpenLevel(\"harbor\")")
        );
        assert_eq!(
            crate::level::read_project(&sb.read("data/project.ron"))
                .unwrap()
                .startup_level,
            "harbor"
        );
        assert!(sb.0.join("levels/harbor.level.ron").exists());
    }

    #[test]
    fn builtin_resources_are_not_deleted_or_renamed_but_can_be_duplicated() {
        let sb = Sandbox::new("builtin");
        for (kind, key) in [
            (AssetKind::Screen, "ui/hud.ui.ron"),
            (AssetKind::Level, "levels/main.level.ron"),
            (AssetKind::Script, "data/scripts/goblin.rhai"),
        ] {
            for k in [OpKind::Delete, OpKind::Rename] {
                let err = plan(&sb.0, &op(k, kind, key, "x2")).unwrap_err();
                assert!(err.contains("내장"), "{key} {k:?}: {err}");
            }
            assert!(
                plan(&sb.0, &op(OpKind::Duplicate, kind, key, "x2")).is_ok(),
                "{key}"
            );
        }
    }

    #[test]
    fn a_used_resource_is_not_deleted_and_says_who_uses_it() {
        let sb = Sandbox::new("used");
        let err = plan(
            &sb.0,
            &op(
                OpKind::Delete,
                AssetKind::Zone,
                "zones/village.zone.ron",
                "",
            ),
        )
        .unwrap_err();
        assert!(err.contains("levels/village.level.ron"), "{err}");
        // 고블린(#102)은 마을 존의 마커가 쓴다.
        let err = plan(
            &sb.0,
            &op(OpKind::Delete, AssetKind::Actor, "actor:102", ""),
        )
        .unwrap_err();
        assert!(err.contains("zones/village.zone.ron"), "{err}");
        // 기본 액터(#1 플레이어)는 지우지 않는다.
        let err = plan(&sb.0, &op(OpKind::Delete, AssetKind::Actor, "actor:1", "")).unwrap_err();
        assert!(err.contains("기본 액터"), "{err}");
    }

    #[test]
    fn deleting_moves_to_the_trash_instead_of_erasing() {
        let sb = Sandbox::new("trash");
        let dup = op(
            OpKind::Duplicate,
            AssetKind::Screen,
            "ui/settings.ui.ron",
            "options2",
        );
        apply(&sb.0, &plan(&sb.0, &dup).unwrap()).unwrap();
        let del = op(OpKind::Delete, AssetKind::Screen, "ui/options2.ui.ron", "");
        let bin = apply(&sb.0, &plan(&sb.0, &del).unwrap())
            .unwrap()
            .expect("휴지통 폴더");
        assert!(!sb.0.join("ui/options2.ui.ron").exists());
        assert!(
            sb.0.join(&bin).join("ui/options2.ui.ron").exists(),
            "휴지통에 원래 경로로"
        );
    }

    #[test]
    fn an_actor_can_be_duplicated_and_the_copy_deleted_with_a_trash_note() {
        let sb = Sandbox::new("actor");
        let dup = op(OpKind::Duplicate, AssetKind::Actor, "actor:100", "150");
        apply(&sb.0, &plan(&sb.0, &dup).unwrap()).unwrap();
        let forms = actor_forms(&sb.read(RULES_PATH), &sb.read(DISPLAY_PATH)).unwrap();
        assert_eq!(forms[&150].name, "슬라임 복사본");
        assert_eq!(forms[&150].max_hp, forms[&100].max_hp);

        let del = op(OpKind::Delete, AssetKind::Actor, "actor:150", "");
        let bin = apply(&sb.0, &plan(&sb.0, &del).unwrap()).unwrap().unwrap();
        let forms = actor_forms(&sb.read(RULES_PATH), &sb.read(DISPLAY_PATH)).unwrap();
        assert!(!forms.contains_key(&150));
        let note = std::fs::read_to_string(sb.0.join(&bin).join("actor-150.ron")).unwrap();
        assert!(
            note.contains("150: (") && note.contains("슬라임 복사본"),
            "되살릴 수 있는 기록"
        );
    }

    #[test]
    fn bad_names_and_existing_files_are_refused() {
        let sb = Sandbox::new("names");
        let bad = plan(
            &sb.0,
            &op(
                OpKind::Duplicate,
                AssetKind::Zone,
                "zones/village.zone.ron",
                "마을",
            ),
        );
        assert!(bad.is_err(), "파일 이름이 될 수 없는 이름");
        let taken = plan(
            &sb.0,
            &op(
                OpKind::Duplicate,
                AssetKind::Zone,
                "zones/village.zone.ron",
                "sample",
            ),
        )
        .unwrap_err();
        assert!(taken.contains("이미 있습니다"), "{taken}");
        let unsupported = plan(
            &sb.0,
            &op(OpKind::Delete, AssetKind::Data, "data/rules.ron", ""),
        );
        assert!(unsupported.is_err());
    }

    #[test]
    fn nothing_is_written_when_planning() {
        let sb = Sandbox::new("dry");
        let before = sb.read("levels/village.level.ron");
        let _ = plan(
            &sb.0,
            &op(
                OpKind::Rename,
                AssetKind::Zone,
                "zones/village.zone.ron",
                "town",
            ),
        )
        .unwrap();
        assert_eq!(sb.read("levels/village.level.ron"), before);
        assert!(sb.0.join("zones/village.zone.ron").exists());
    }
}
