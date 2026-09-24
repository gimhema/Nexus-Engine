//! 레벨 — 무엇을 열고 어떤 화면을 띄울지 (단계 2 P7).
//!
//! **언리얼의 레벨 관리에 맞춘 구조다.**
//!
//! | 언리얼 | 여기 |
//! |---|---|
//! | Level (`.umap`) | `levels/<번호>.level.ron` — 쓸 존 파일 + 띄울 화면 |
//! | World Settings | 위 레벨 파일 안 (존 파일은 지형·마커만 담는다) |
//! | UI 만 있는 맵 | `zone: None` 인 레벨 — 메인 화면·설정 화면이 여기 붙는다 |
//! | Project Settings 의 시작 맵 | `data/project.ron` 의 `startup_level` |
//! | `Open Level` | 버튼 동작 [`crate::screen::Action::OpenLevel`] |
//! | Add to Viewport / 화면 겹치기 | [`Shell`] 의 화면 스택 |
//!
//! 레벨 파일이 **존 파일과 따로인 이유**: 존 파일(`zones/*.zone.ron`)은 지형과 마커만 담는
//! 저작 데이터이고 테스트가 바이트 단위로 대조한다. 또 메인 화면처럼 **존이 없는 레벨**도
//! 있어야 한다 — 존 파일에 UI 설정을 넣으면 그 경우를 표현할 수 없다.
//!
//! 다른 데이터 파일과 같은 규칙: 실행 파일에 **내장**되어 있고, 같은 경로에 디스크 파일이
//! 있으면 그것이 우선한다. 잘못된 파일은 경고로 남기고 그 레벨만 빠진다.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 프로젝트 설정 (작업 디렉터리 기준).
pub(crate) const PROJECT_PATH: &str = "data/project.ron";
const EMBEDDED_PROJECT: &str = include_str!("../../../data/project.ron");

/// 레벨 파일이 있는 폴더. 파일 이름이 곧 레벨 번호다 (`levels/village.level.ron` → `"village"`).
pub(crate) const LEVEL_DIR: &str = "levels";
const LEVEL_EXT: &str = ".level.ron";

const EMBEDDED_LEVELS: &[(&str, &str)] = &[
    ("main", include_str!("../../../levels/main.level.ron")),
    ("village", include_str!("../../../levels/village.level.ron")),
    ("sample", include_str!("../../../levels/sample.level.ron")),
];

const FORMAT_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectFile {
    pub(crate) version: u32,
    /// 게임을 시작할 때 여는 레벨 — 보통 메인 화면.
    pub(crate) startup_level: String,
}

/// 레벨 한 개.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LevelFile {
    pub(crate) version: u32,
    /// 사람이 읽는 이름.
    pub(crate) name: String,
    /// 쓸 존 파일. **없으면 UI 만 있는 레벨**이다 (시뮬레이션을 돌리지 않는다).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) zone: Option<String>,
    /// 플레이 중 바탕에 띄울 화면 (보통 `"hud"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) hud: Option<String>,
    /// 레벨에 들어갈 때 띄울 화면 — 메인 화면·설정 화면 레벨이 쓴다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) on_enter: Option<String>,
    /// Esc 로 띄울 화면 (일시정지).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) pause: Option<String>,
}

impl LevelFile {
    /// 바탕 화면 — 존이 있으면 HUD, 없으면 들어갈 때 띄우는 화면.
    pub(crate) fn base_screen(&self) -> Option<&str> {
        if self.zone.is_some() {
            self.hud.as_deref().or(self.on_enter.as_deref())
        } else {
            self.on_enter.as_deref()
        }
    }
}

/// 프로젝트 설정과 레벨 목록.
#[derive(Debug)]
pub(crate) struct Levels {
    project: ProjectFile,
    levels: BTreeMap<String, LevelFile>,
}

impl Default for Levels {
    fn default() -> Self {
        Self {
            project: ProjectFile {
                version: FORMAT_VERSION,
                startup_level: String::new(),
            },
            levels: BTreeMap::new(),
        }
    }
}

impl Levels {
    /// 설정과 레벨을 읽는다. 못 읽은 것은 경고로 돌려준다 (레벨이 없다고 에디터를 막지 않는다).
    pub(crate) fn load() -> (Self, Vec<String>) {
        let mut warnings = Vec::new();
        let text = match std::fs::read_to_string(PROJECT_PATH) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => EMBEDDED_PROJECT.to_owned(),
            Err(e) => {
                warnings.push(format!("{PROJECT_PATH}: {e}"));
                EMBEDDED_PROJECT.to_owned()
            }
        };
        let project = match read_project(&text) {
            Ok(project) => project,
            Err(e) => {
                warnings.push(format!("{PROJECT_PATH}: {e}"));
                match read_project(EMBEDDED_PROJECT) {
                    Ok(project) => project,
                    Err(e) => {
                        warnings.push(format!("내장 프로젝트 설정: {e}"));
                        return (Self::default(), warnings);
                    }
                }
            }
        };
        let levels = load_levels(&mut warnings);
        if !levels.contains_key(&project.startup_level) {
            warnings.push(format!(
                "{PROJECT_PATH}: 시작 레벨 '{}' 이 없습니다",
                project.startup_level
            ));
        }
        (Self { project, levels }, warnings)
    }

    pub(crate) fn level(&self, id: &str) -> Option<&LevelFile> {
        self.levels.get(id)
    }

    /// 시작 레벨 번호 — 언리얼의 Game Default Map.
    pub(crate) fn startup(&self) -> &str {
        &self.project.startup_level
    }

    /// `(번호, 이름)` 목록 (사전 순) — 메뉴에 쓴다.
    pub(crate) fn list(&self) -> Vec<(String, String)> {
        self.levels
            .iter()
            .map(|(id, level)| (id.clone(), level.name.clone()))
            .collect()
    }

    /// 이 존 파일을 쓰는 레벨 — F5(지금 레벨 플레이)가 어느 레벨인지 알아내는 데 쓴다.
    pub(crate) fn find_by_zone(&self, zone: Option<&Path>) -> Option<&str> {
        let zone = zone?;
        let zone = normalize(zone);
        self.levels
            .iter()
            .find(|(_, level)| {
                level.zone.as_deref().map(str::to_ascii_lowercase) == Some(zone.clone())
            })
            .map(|(id, _)| id.as_str())
    }
}

/// 경로 비교용 — 소문자 + `/`. Windows 와 리눅스에서 같은 결과가 나오게.
fn normalize(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase()
}

pub(crate) fn read_project(text: &str) -> Result<ProjectFile, String> {
    let file: ProjectFile = ron::from_str(text).map_err(|e| format!("읽을 수 없음 — {e}"))?;
    if file.version != FORMAT_VERSION {
        return Err(format!(
            "형식 {} 은(는) 읽을 수 없음 (이 에디터는 {FORMAT_VERSION})",
            file.version
        ));
    }
    if file.startup_level.trim().is_empty() {
        return Err(String::from("startup_level 이 비어 있음"));
    }
    Ok(file)
}

pub(crate) fn read_level(text: &str) -> Result<LevelFile, String> {
    let file: LevelFile = ron::from_str(text).map_err(|e| format!("읽을 수 없음 — {e}"))?;
    if file.version != FORMAT_VERSION {
        return Err(format!(
            "형식 {} 은(는) 읽을 수 없음 (이 에디터는 {FORMAT_VERSION})",
            file.version
        ));
    }
    if file.name.trim().is_empty() {
        return Err(String::from("레벨 이름이 비어 있음"));
    }
    if let Some(zone) = &file.zone {
        // 경로 규칙은 에셋·스크립트와 같다 — 리눅스에서 "파일 없음" 이 되는 것을 미리 막는다.
        if zone != &zone.to_ascii_lowercase() || zone.contains('\\') || !zone.ends_with(".zone.ron")
        {
            return Err(format!(
                "zone '{zone}' — 소문자·'/'·'.zone.ron' 으로 끝나는 상대 경로여야 함"
            ));
        }
    }
    if file.zone.is_none() && file.on_enter.is_none() {
        return Err(String::from(
            "존도 화면도 없는 레벨입니다 — zone 이나 on_enter 중 하나는 있어야 합니다",
        ));
    }
    Ok(file)
}

/// `levels/` 를 읽는다. 디스크가 우선이고, 없는 것은 내장본으로 채운다.
fn load_levels(warnings: &mut Vec<String>) -> BTreeMap<String, LevelFile> {
    let mut out = BTreeMap::new();
    for (id, text) in EMBEDDED_LEVELS {
        match read_level(text) {
            Ok(level) => {
                out.insert((*id).to_owned(), level);
            }
            Err(e) => warnings.push(format!("내장 레벨 '{id}': {e}")),
        }
    }
    let Ok(dir) = std::fs::read_dir(LEVEL_DIR) else {
        return out; // 폴더가 없으면 내장본만
    };
    let mut paths: Vec<PathBuf> = dir
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(LEVEL_EXT))
        })
        .collect();
    paths.sort(); // 실행마다 같은 순서
    for path in paths {
        let Some(id) = level_id(&path) else { continue };
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| read_level(&t))
        {
            Ok(level) => {
                out.insert(id, level);
            }
            Err(e) => warnings.push(format!("{}: {e}", path.display())),
        }
    }
    out
}

fn level_id(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    Some(name.strip_suffix(LEVEL_EXT)?.to_ascii_lowercase())
}

/// 실행 파일에 내장된 레벨인가 — 디스크 파일을 지워도 내장본으로 되살아난다 (P9).
pub(crate) fn is_builtin(id: &str) -> bool {
    EMBEDDED_LEVELS.iter().any(|(i, _)| *i == id)
}

/// 레벨 번호 → 파일 경로. 이름은 **소문자**로 고정한다 (ext4 는 대소문자를 구분한다).
pub(crate) fn level_path(id: &str) -> PathBuf {
    Path::new(LEVEL_DIR).join(format!("{}{LEVEL_EXT}", id.to_ascii_lowercase()))
}

/// 레벨을 RON 으로 — 레벨 편집기가 저장할 때 쓴다 (P8). 줄바꿈은 LF 고정.
///
/// 화면 파일처럼 **편집기가 파일을 소유한다** — 통째로 다시 쓰므로 손으로 쓴 주석은 남지 않는다
/// (머리 주석만 늘 새로 붙는다). 레벨 파일은 몇 줄뿐이라 설명은 이 머리 주석으로 충분하다.
pub(crate) fn to_ron(level: &LevelFile) -> String {
    let config = ron::ser::PrettyConfig::new()
        .new_line("\n")
        .indentor("    ")
        .struct_names(false);
    let body = ron::ser::to_string_pretty(level, config).expect("레벨 직렬화 실패");
    let header = "\
// 레벨 — 언리얼의 레벨(.umap)에 대응한다. 쓸 존 파일과 띄울 화면을 적는다.
// **에디터의 레벨 편집기가 이 파일을 다시 써낸다** (콘텐츠 브라우저 → 레벨 두 번 누르기).
//
// zone 이 없으면 UI 만 있는 레벨이다 (메인 화면). hud = 플레이 중 바탕 화면,
// on_enter = 들어갈 때 띄울 화면, pause = Esc 로 띄울 화면 (전부 ui/<번호>.ui.ron).
";
    format!("{header}{body}\n")
}

/// 프로젝트 설정의 시작 레벨만 바꾼다 — 주석은 그대로 둔다 (`startup_level:` 줄만).
pub(crate) fn patch_startup(project: &str, id: &str) -> Result<String, String> {
    let text =
        crate::ron_patch::replace_field(project, "startup_level", &crate::ron_patch::quote(id))?;
    read_project(&text)?;
    Ok(text)
}

// ─────────────────────────────────────────────────────────────────────────────
// 실행 중 상태
// ─────────────────────────────────────────────────────────────────────────────

/// 지금 열려 있는 레벨과 그 위에 뜬 화면들 — **게임 셸**.
///
/// 화면은 **스택**이다: `[0]` 이 바탕(HUD 또는 메뉴)이고 뒤로 갈수록 위에 뜬다
/// (일시정지 → 설정처럼 겹친다). 바탕은 닫히지 않는다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Shell {
    pub(crate) level: String,
    pub(crate) screens: Vec<String>,
    /// 일시정지 — 시뮬레이션 tick 을 멈춘다.
    pub(crate) paused: bool,
}

impl Shell {
    pub(crate) fn new(level: &str, base: Option<&str>) -> Self {
        Self {
            level: level.to_owned(),
            screens: base.map(|b| vec![b.to_owned()]).unwrap_or_default(),
            paused: false,
        }
    }

    /// 맨 위 화면 — 버튼을 찾는 곳이다 (아래 화면의 버튼은 가려진 것으로 본다).
    pub(crate) fn top(&self) -> Option<&str> {
        self.screens.last().map(String::as_str)
    }

    /// 바탕 위에 화면을 겹친다. 이미 맨 위면 아무것도 하지 않는다.
    pub(crate) fn push(&mut self, id: &str) {
        if self.top() != Some(id) {
            self.screens.push(id.to_owned());
        }
    }

    /// 맨 위 화면을 닫는다. **바탕은 닫히지 않는다.**
    pub(crate) fn pop(&mut self) -> bool {
        if self.screens.len() > 1 {
            self.screens.pop();
            true
        } else {
            false
        }
    }

    /// 바탕만 남았는가 — 게임 클릭을 받아도 되는 상태다.
    pub(crate) fn is_base(&self) -> bool {
        self.screens.len() <= 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_levels_and_project_are_valid() {
        let project = read_project(EMBEDDED_PROJECT).expect("프로젝트 설정");
        let mut ids = Vec::new();
        for (id, text) in EMBEDDED_LEVELS {
            read_level(text).unwrap_or_else(|e| panic!("내장 레벨 '{id}': {e}"));
            ids.push((*id).to_owned());
        }
        assert!(
            ids.contains(&project.startup_level),
            "시작 레벨 '{}' 이 레벨 목록에 없다",
            project.startup_level
        );
    }

    /// 레벨이 가리키는 존 파일과 화면이 실제로 있는지 — 저장소에 들어가는 조합을 지킨다.
    #[test]
    fn levels_point_at_files_that_exist() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for (id, text) in EMBEDDED_LEVELS {
            let level = read_level(text).unwrap();
            if let Some(zone) = &level.zone {
                assert!(
                    root.join(zone).exists(),
                    "레벨 '{id}' 의 존 파일 {zone} 이 없다"
                );
            }
            for screen in [&level.hud, &level.on_enter, &level.pause]
                .into_iter()
                .flatten()
            {
                assert!(
                    root.join(crate::screen::screen_path(screen)).exists(),
                    "레벨 '{id}' 의 화면 {screen} 이 없다"
                );
            }
        }
    }

    /// 저장소의 레벨 파일은 **레벨 편집기가 저장하는 모양 그대로**다 — 열어서 그냥 저장만 해도
    /// 바이트가 달라지면 diff 가 지저분해진다 (화면 파일·존 샘플과 같은 규칙).
    #[test]
    fn shipped_levels_are_already_in_editor_format() {
        for (id, text) in EMBEDDED_LEVELS {
            let level = read_level(text).unwrap();
            assert_eq!(
                to_ron(&level),
                *text,
                "levels/{id}.level.ron 을 편집기로 저장하면 바이트가 바뀐다 — \
                 `cargo test -p world-editor -- --ignored regenerate_shipped_levels` 로 갱신하라"
            );
            assert_eq!(read_level(&to_ron(&level)).unwrap(), level, "왕복");
        }
    }

    #[test]
    #[ignore = "저장소의 레벨 파일을 덮어쓴다 — 필요할 때만 직접 실행"]
    fn regenerate_shipped_levels() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for (id, text) in EMBEDDED_LEVELS {
            let level = read_level(text).unwrap();
            std::fs::write(root.join(level_path(id)), to_ron(&level)).unwrap();
        }
    }

    #[test]
    fn the_startup_level_line_is_patched_in_place() {
        let out = patch_startup(EMBEDDED_PROJECT, "village").unwrap();
        assert_eq!(read_project(&out).unwrap().startup_level, "village");
        assert!(out.contains("// 프로젝트 설정"), "머리 주석이 남는다");
        assert!(
            patch_startup(EMBEDDED_PROJECT, " ").is_err(),
            "빈 이름은 거부"
        );
    }

    #[test]
    fn a_bad_level_says_why() {
        let cases = [
            ("(version: 9, name: \"x\", on_enter: Some(\"m\"))", "형식"),
            (
                "(version: 1, name: \" \", on_enter: Some(\"m\"))",
                "이름이 비어",
            ),
            ("(version: 1, name: \"x\")", "존도 화면도 없는"),
            (
                "(version: 1, name: \"x\", zone: Some(\"zones/Village.zone.ron\"))",
                "소문자",
            ),
            (
                "(version: 1, name: \"x\", zone: Some(\"zones/village.ron\"))",
                "소문자",
            ),
            (
                "(version: 1, name: \"x\", zne: Some(\"a\"))",
                "읽을 수 없음",
            ),
        ];
        for (text, want) in cases {
            let err = read_level(text).expect_err(text);
            assert!(err.contains(want), "'{err}' 에 '{want}' 가 없다");
        }
    }

    #[test]
    fn a_level_without_a_zone_is_a_ui_level() {
        let level =
            read_level("(version: 1, name: \"메뉴\", on_enter: Some(\"main_menu\"))").unwrap();
        assert_eq!(level.zone, None);
        assert_eq!(level.base_screen(), Some("main_menu"));

        let level = read_level(
            "(version: 1, name: \"마을\", zone: Some(\"zones/village.zone.ron\"), hud: Some(\"hud\"))",
        )
        .unwrap();
        assert_eq!(level.base_screen(), Some("hud"));
    }

    #[test]
    fn level_ids_come_from_file_names() {
        assert_eq!(
            level_id(Path::new("levels/village.level.ron")).as_deref(),
            Some("village")
        );
        assert_eq!(level_id(Path::new("levels/village.ron")), None);
    }

    #[test]
    fn the_screen_stack_keeps_its_base() {
        let mut shell = Shell::new("village", Some("hud"));
        assert_eq!(shell.top(), Some("hud"));
        assert!(shell.is_base());
        assert!(!shell.pop(), "바탕은 닫히지 않는다");

        shell.push("pause");
        shell.push("pause"); // 같은 화면을 두 번 겹치지 않는다
        assert_eq!(shell.screens, vec!["hud", "pause"]);
        assert!(!shell.is_base(), "겹친 창이 있으면 게임 클릭을 받지 않는다");

        shell.push("settings");
        assert_eq!(shell.top(), Some("settings"));
        assert!(shell.pop());
        assert_eq!(shell.top(), Some("pause"));
        assert!(shell.pop());
        assert!(shell.is_base());
    }

    #[test]
    fn a_zone_path_finds_its_level_on_both_systems() {
        let (levels, _) = Levels::load();
        // 윈도우 구분자·대문자여도 같은 레벨을 찾는다.
        assert_eq!(
            levels.find_by_zone(Some(Path::new("zones\\Village.zone.ron"))),
            Some("village")
        );
        assert_eq!(
            levels.find_by_zone(Some(Path::new("zones/none.zone.ron"))),
            None
        );
        assert_eq!(levels.find_by_zone(None), None);
    }
}
