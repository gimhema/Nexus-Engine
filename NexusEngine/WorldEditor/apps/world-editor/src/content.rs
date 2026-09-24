//! 콘텐츠 브라우저 — 엔진이 다루는 리소스를 한곳에서 보고 여는 곳 (단계 2 P8).
//!
//! ```text
//! ┌ 콘텐츠 브라우저 ──────────────────────────────────────────────────────────┐
//! │ [새로 만들기 ▼] [새로 고침]  보기: (아이콘)(자세히)  종류: [모든 종류 ▼]      │
//! │ [◀][▲]  프로젝트 › levels                                    [검색……]      │
//! │ ▾ 프로젝트      │ ┌──┐ ┌──┐ ┌──┐                             │ 레벨 · 마을     │
//! │   ▸ assets      │ │LV│ │LV│ │LV│                             │ 존: zones/…     │
//! │   ▾ data        │ └──┘ └──┘ └──┘                             │ 쓰는 곳:        │
//! │     scripts     │ 마을 메인 화면 샘플 존                       │  ui/main_menu…  │
//! │   ▸ 게임 데이터 │                                            │ [열기]          │
//! │ 3개 항목 · 선택: 마을                                                         │
//! └──────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! - **파일 탐색기처럼** 왼쪽은 폴더 트리, 가운데는 지금 폴더의 내용(하위 폴더 먼저),
//!   위는 주소 줄(뒤로 · 위로 · 경로 조각)이다. 가운데는 **아이콘 / 자세히** 두 보기가 있다.
//! - **한 번 누르면 상세**(오른쪽), **두 번 누르면 그 종류의 편집기**가 열린다 (언리얼과 같다).
//!   폴더는 두 번 누르면 들어간다.
//! - **검색어나 종류 필터가 있으면 지금 폴더 아래 전체**에서 찾는다 (탐색기의 검색과 같다) —
//!   같은 종류가 여러 폴더에 흩어져 있어서다 (스프라이트 시트는 받아온 팩 폴더마다 그림 옆에 있다).
//! - **액터는 파일이 아니다.** `rules.ron`(수치)·`display.ron`(이름·시트) 표 안의 항목을
//!   하나씩 가상 항목으로 보여 준다 — 서버 테이블과 짝을 맞춘 "반으로 나눈 표" 구조를
//!   건드리지 않기 위해서다 (사용자 결정 2026-09-24). 트리에서는 **"게임 데이터" 가상 폴더**
//!   아래에 종류마다 하위 폴더로 모인다 (디스크에 그런 폴더는 없다).
//! - **디스크의 파일만** 보인다. 실행 파일에 내장된 기본 데이터는 파일이 없으면 목록에 없다.
//! - 목록은 창을 열 때·새로 고침·편집기가 저장한 뒤에만 다시 읽는다 (매 프레임 디스크를 훑지 않는다).
//!
//! "쓰는 곳"(참조)은 **문자열 검색**으로 찾는다 — 레벨 파일의 `zone: Some("…")`,
//! 화면의 `OpenLevel("…")`, 액터의 `script`·`sheet`, 존 파일의 `actor: N` 같은 것을 본다.
//! 빠르고 단순하지만, 경로를 다른 모양으로 적은 곳(`./zones/…`)은 놓칠 수 있다.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use nexus_assets::Image;
use nexus_render_wgpu::egui;

use crate::asset_ops::{AssetRef, FileOp, OpKind, Plan, supports};
use crate::game_data::{ActorForm, TableForms, actor_forms, table_forms};
use crate::table_editor::Tab;

/// 리소스 종류 — 왼쪽 목록의 순서이기도 하다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum AssetKind {
    Level,
    Zone,
    Screen,
    Actor,
    /// 아이템·스킬·드롭 표 — 액터처럼 표 안의 항목 (P10).
    Item,
    Skill,
    Loot,
    Script,
    Sheet,
    Image,
    Data,
}

impl AssetKind {
    pub(crate) const ALL: [Self; 11] = [
        Self::Level,
        Self::Zone,
        Self::Screen,
        Self::Actor,
        Self::Item,
        Self::Skill,
        Self::Loot,
        Self::Script,
        Self::Sheet,
        Self::Image,
        Self::Data,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Level => "레벨",
            Self::Zone => "맵",
            Self::Screen => "UI 화면",
            Self::Actor => "액터",
            Self::Item => "아이템",
            Self::Skill => "스킬",
            Self::Loot => "드롭 표",
            Self::Script => "스크립트",
            Self::Sheet => "스프라이트 시트",
            Self::Image => "그림",
            Self::Data => "데이터",
        }
    }

    /// 목록의 색 표시 (sRGB) — 종류를 한눈에 가르려고.
    fn color(self) -> egui::Color32 {
        match self {
            Self::Level => egui::Color32::from_rgb(230, 190, 80),
            Self::Zone => egui::Color32::from_rgb(110, 200, 110),
            Self::Screen => egui::Color32::from_rgb(170, 130, 230),
            Self::Actor => egui::Color32::from_rgb(235, 140, 80),
            Self::Item => egui::Color32::from_rgb(210, 170, 110),
            Self::Skill => egui::Color32::from_rgb(230, 100, 90),
            Self::Loot => egui::Color32::from_rgb(140, 190, 120),
            Self::Script => egui::Color32::from_rgb(100, 160, 240),
            Self::Sheet => egui::Color32::from_rgb(230, 120, 170),
            Self::Image => egui::Color32::from_rgb(90, 200, 200),
            Self::Data => egui::Color32::from_rgb(160, 160, 170),
        }
    }

    /// 아이콘 안의 짧은 글씨 — 영문이라 어느 폰트에서나 나온다.
    fn badge(self) -> &'static str {
        match self {
            Self::Level => "LV",
            Self::Zone => "MAP",
            Self::Screen => "UI",
            Self::Actor => "ACT",
            Self::Item => "ITM",
            Self::Skill => "SKL",
            Self::Loot => "DRP",
            Self::Script => "RHAI",
            Self::Sheet => "SHT",
            Self::Image => "PNG",
            Self::Data => "RON",
        }
    }

    /// 파일이 아니라 표 안의 항목인가.
    fn is_virtual(self) -> bool {
        matches!(self, Self::Actor | Self::Item | Self::Skill | Self::Loot)
    }
}

/// 표 항목이 모이는 가상 폴더의 이름. 디스크 경로는 소문자 영문이라 겹치지 않는다.
const VIRTUAL_ROOT: &str = "게임 데이터";

/// 폴더의 부모 — `""` 가 프로젝트 맨 위.
fn parent(folder: &str) -> &str {
    folder.rsplit_once('/').map_or("", |(p, _)| p)
}

/// 트리·주소 줄에 보이는 폴더 이름.
fn folder_name(folder: &str) -> &str {
    if folder.is_empty() {
        "프로젝트"
    } else {
        folder.rsplit('/').next().unwrap_or(folder)
    }
}

/// `folder` 가 `ancestor` 아래(또는 그 자신)인가.
fn is_under(folder: &str, ancestor: &str) -> bool {
    ancestor.is_empty()
        || folder == ancestor
        || folder
            .strip_prefix(ancestor)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// 가운데 보기 방식.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum View {
    /// 큰 아이콘 타일 (그림은 썸네일).
    #[default]
    Icons,
    /// 한 줄에 하나 — 이름 · 종류 · 크기 · 경로.
    List,
}

/// 가운데의 한 칸 — 하위 폴더 또는 항목.
#[derive(Clone, Debug)]
enum Entry {
    Folder(String),
    Asset(Asset),
}

/// 목록의 항목 하나.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Asset {
    pub(crate) kind: AssetKind,
    /// 파일이면 작업 디렉터리 기준 경로(`/` 구분, 소문자 그대로). 액터면 `actor:번호`.
    pub(crate) key: String,
    /// 목록에 보이는 이름.
    pub(crate) name: String,
    /// 이름 옆의 작은 글씨 — 경로나 번호.
    pub(crate) note: String,
    /// 파일 크기 (표 항목은 없다).
    pub(crate) bytes: Option<u64>,
}

impl Asset {
    /// 이 항목이 들어 있는 폴더 — 파일은 경로의 폴더, 표 항목은 `게임 데이터/<종류>`.
    pub(crate) fn folder(&self) -> String {
        if self.kind.is_virtual() {
            format!("{VIRTUAL_ROOT}/{}", self.kind.label())
        } else {
            parent(&self.key).to_owned()
        }
    }

    /// 액터 항목이면 그 번호.
    pub(crate) fn actor_id(&self) -> Option<u32> {
        self.key.strip_prefix("actor:")?.parse().ok()
    }

    /// 표 항목(`item:501`·`skill:1`·`loot:1`)이면 그 번호.
    pub(crate) fn entry_id(&self) -> Option<u32> {
        self.key.split_once(':')?.1.parse().ok()
    }

    /// 파일 이름에서 확장자를 뗀 번호 — `levels/village.level.ron` → `village`.
    pub(crate) fn file_id(&self) -> Option<&str> {
        let name = self.key.rsplit('/').next()?;
        let ext = match self.kind {
            AssetKind::Level => ".level.ron",
            AssetKind::Screen => ".ui.ron",
            AssetKind::Zone => ".zone.ron",
            _ => return None,
        };
        name.strip_suffix(ext)
    }
}

/// 두 번 눌렀을 때 할 일 — 편집기를 누가 가졌는지에 따라 UI 가 처리하거나 에디터로 넘긴다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ContentAction {
    /// 존 파일을 뷰포트에 연다.
    OpenZone(PathBuf),
    /// 레벨 편집기.
    EditLevel(String),
    /// 위젯 편집기로 이 화면을.
    EditScreen(String),
    /// 스크립트 편집기 (`data/` 기준 경로).
    EditScript(String),
    /// 지형 팔레트로 이 그림을.
    OpenPalette(String),
    /// 스프라이트 시트 뷰어.
    ViewSheet(String),
    /// 액터 편집기.
    EditActor(u32),
    /// 데이터 표 편집기 — 아이템·스킬·드롭 표 (P10).
    EditTable(Tab, u32),
    NewTable(Tab),
    /// 게임 규칙 설정 — `rules.ron` 의 한 벌짜리 값 (시드·기본 액터·진영·시작 소지품).
    EditRules,
    /// 새 레벨 / 새 액터 — 해당 편집기에서 만든다.
    NewLevel,
    NewActor,
    /// 위젯 편집기·스크립트 편집기는 자기 창에 "새로 만들기" 가 있다 — 창만 연다.
    NewScreen,
    NewScript,
    /// 편집기가 없는 파일 — 알림만.
    Notice(String),
    /// 파일 작업 — 확인 창에서 '실행' 을 눌렀다 (P9). 에디터가 다시 계획을 세워 실행한다.
    FileOp(FileOp),
}

/// 브라우저 상태. 창이 닫혀 있어도 고른 항목·검색어는 남는다.
#[derive(Default)]
pub(crate) struct ContentBrowser {
    open: bool,
    assets: Vec<Asset>,
    /// 액터·데이터 표 폼 — 상세와 참조 검색에 쓴다.
    forms: Forms,
    /// 목록을 읽다 난 오류 (규칙·표시 파일이 깨졌을 때 등).
    error: Option<String>,
    /// 모든 폴더 (조상 포함, 이름순) — 목록을 읽을 때 한 번 만든다.
    folders: Vec<String>,
    /// 지금 보고 있는 폴더. `""` = 프로젝트 맨 위.
    folder: String,
    /// 뒤로 가기 기록.
    back: Vec<String>,
    /// 다음 프레임에 트리에서 지금 폴더까지 펼친다 (이동한 직후 한 번).
    reveal: bool,
    view: View,
    /// 종류 필터. `None` = 모든 종류.
    kind: Option<AssetKind>,
    search: String,
    /// 한 번 누른 항목의 key.
    selected: Option<String>,
    /// 고른 항목의 상세 — 고를 때 한 번 계산한다.
    details: Option<Details>,
    thumbs: Thumbnails,
    /// 처음 열 때 목록을 읽는다.
    loaded: bool,
    /// 파일 작업 확인 창 (P9).
    op_dialog: Option<OpDialog>,
    /// 빈 곳 오른쪽 클릭 메뉴를 코드로 연다 — 자동 검증(`contentmenu`)용.
    menu_request: bool,
    /// 코드로 연 메뉴의 자리 (마우스 자리가 없으므로).
    menu_at: Option<egui::Pos2>,
}

/// 파일 작업 확인 창 — 새 이름을 받고, 할 일을 **미리 계산해** 보여 준다.
#[derive(Clone, Debug)]
struct OpDialog {
    kind: OpKind,
    asset: AssetRef,
    target: String,
    /// 마지막으로 계산한 `(새 이름, 계획)` — 이름이 바뀔 때만 다시 계산한다.
    cached: Option<(String, Result<Plan, String>)>,
}

impl OpDialog {
    fn op(&self) -> FileOp {
        FileOp {
            kind: self.kind,
            asset: self.asset.clone(),
            target: self.target.clone(),
        }
    }

    /// 계획 — 새 이름이 바뀌었을 때만 다시 계산한다 (디스크를 읽으므로 매 프레임 하지 않는다).
    fn plan(&mut self) -> &Result<Plan, String> {
        let stale = self.cached.as_ref().is_none_or(|(t, _)| *t != self.target);
        if stale {
            let plan = crate::asset_ops::plan(Path::new("."), &self.op());
            self.cached = Some((self.target.clone(), plan));
        }
        &self.cached.as_ref().expect("방금 채웠다").1
    }
}

impl std::fmt::Debug for ContentBrowser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContentBrowser")
            .field("open", &self.open)
            .field("assets", &self.assets.len())
            .field("selected", &self.selected)
            .finish()
    }
}

/// 표에서 읽은 것 — 액터와 아이템·스킬·드롭 표.
#[derive(Clone, Debug, Default)]
pub(crate) struct Forms {
    actors: BTreeMap<u32, ActorForm>,
    tables: TableForms,
}

/// 고른 항목의 상세.
#[derive(Clone, Debug, Default)]
struct Details {
    /// `(항목, 값)` 줄.
    rows: Vec<(String, String)>,
    /// 이 리소스를 가리키는 곳.
    used_by: Vec<String>,
    /// 그림 미리보기 (PNG 경로).
    preview: Option<String>,
}

impl ContentBrowser {
    pub(crate) fn toggle(&mut self) {
        self.open = !self.open;
        if self.open && !self.loaded {
            self.refresh();
        }
    }

    /// 디스크를 다시 훑는다 — 편집기가 저장한 뒤에도 부른다.
    pub(crate) fn refresh(&mut self) {
        self.loaded = true;
        let root = Path::new(".");
        let (assets, forms, error) = collect(root);
        self.folders = folders_of(&assets);
        self.assets = assets;
        self.forms = forms;
        self.error = error;
        // 보던 폴더가 사라졌으면(이름 바꾸기 등) 남아 있는 가장 가까운 조상으로.
        while !self.folder.is_empty() && !self.folders.contains(&self.folder) {
            self.folder = parent(&self.folder).to_owned();
        }
        // 고른 항목이 사라졌으면 상세도 비운다. 남아 있으면 상세를 다시 계산한다.
        self.details = self
            .selected
            .as_ref()
            .and_then(|key| self.assets.iter().find(|a| &a.key == key))
            .map(|a| details(root, a, &self.assets, &self.forms));
        if self.details.is_none() {
            self.selected = None;
        }
    }

    /// 이름으로 고른다 — 자동 검증(`NEXUS_SCRIPT`)용. 찾으면 상세를 띄운다.
    pub(crate) fn select_key(&mut self, key: &str) -> bool {
        if !self.loaded {
            self.refresh();
        }
        let Some(asset) = self.assets.iter().find(|a| a.key == key).cloned() else {
            return false;
        };
        self.open = true;
        // 그 항목이 든 폴더로 간다 — 필터가 걸려 있으면 안 보일 수 있으니 푼다.
        self.navigate(&asset.folder());
        self.kind = None;
        self.search.clear();
        self.selected = Some(asset.key.clone());
        self.details = Some(details(Path::new("."), &asset, &self.assets, &self.forms));
        true
    }

    /// 폴더로 간다 (뒤로 가기 기록을 남긴다). 없는 폴더면 `false`.
    pub(crate) fn navigate(&mut self, folder: &str) -> bool {
        if !self.loaded {
            self.refresh();
        }
        let folder = folder.trim_matches('/');
        if !folder.is_empty() && !self.folders.iter().any(|f| f == folder) {
            return false;
        }
        if self.folder != folder {
            self.back
                .push(std::mem::replace(&mut self.folder, folder.to_owned()));
        }
        self.reveal = true;
        true
    }

    /// 보기 방식 바꾸기 — 자동 검증용 (`true` = 자세히).
    pub(crate) fn set_list_view(&mut self, list: bool) {
        self.view = if list { View::List } else { View::Icons };
    }

    fn go_back(&mut self) {
        if let Some(f) = self.back.pop() {
            self.folder = f;
            self.reveal = true;
        }
    }

    /// 바로 아래 폴더들.
    fn subfolders(&self, folder: &str) -> Vec<String> {
        self.folders
            .iter()
            .filter(|f| parent(f) == folder)
            .cloned()
            .collect()
    }

    /// 필터(종류·검색어)에 맞는 항목인가.
    fn matches(&self, asset: &Asset, needle: &str) -> bool {
        self.kind.is_none_or(|k| asset.kind == k)
            && (needle.is_empty()
                || asset.name.to_lowercase().contains(needle)
                || asset.key.to_lowercase().contains(needle))
    }

    /// 가운데에 보일 것 — 필터가 없으면 지금 폴더의 하위 폴더 + 항목,
    /// 있으면 지금 폴더 **아래 전체**에서 맞는 항목만.
    fn entries(&self) -> Vec<Entry> {
        let needle = self.search.trim().to_lowercase();
        let filtering = self.kind.is_some() || !needle.is_empty();
        let mut out = Vec::new();
        if !filtering {
            out.extend(self.subfolders(&self.folder).into_iter().map(Entry::Folder));
        }
        out.extend(
            self.assets
                .iter()
                .filter(|a| {
                    let f = a.folder();
                    if filtering {
                        is_under(&f, &self.folder)
                    } else {
                        f == self.folder
                    }
                })
                .filter(|a| self.matches(a, &needle))
                .cloned()
                .map(Entry::Asset),
        );
        out
    }

    /// 고른 항목을 연다 — 자동 검증의 "두 번 누르기".
    pub(crate) fn open_selected(&self) -> Option<ContentAction> {
        let key = self.selected.as_ref()?;
        let asset = self.assets.iter().find(|a| &a.key == key)?;
        Some(open_action(asset))
    }

    /// 파일 작업 확인 창을 연다 — 새 이름 칸에는 쓸 만한 기본값을 넣어 둔다.
    fn begin(&mut self, kind: OpKind, asset: &Asset) {
        let target = match (kind, asset.kind) {
            (OpKind::Delete, _) => String::new(),
            (OpKind::Duplicate, kind) if kind.is_virtual() => {
                // 이 번호 다음의 빈 번호 — 그 표 안에서.
                let id = asset.entry_id().unwrap_or(100);
                let t = &self.forms.tables;
                let taken = |n: &u32| match kind {
                    AssetKind::Item => t.items.contains_key(n),
                    AssetKind::Skill => t.skills.contains_key(n),
                    AssetKind::Loot => t.loot.contains_key(n),
                    _ => self.forms.actors.contains_key(n),
                };
                let free = (id + 1..).find(|n| !taken(n)).unwrap_or(id + 1);
                free.to_string()
            }
            (OpKind::Duplicate, _) => format!("{}_copy", file_stem(asset)),
            (OpKind::Rename, _) => file_stem(asset),
        };
        self.op_dialog = Some(OpDialog {
            kind,
            asset: AssetRef::from(asset),
            target,
            cached: None,
        });
    }

    /// 고른 항목에 파일 작업을 건다 — 자동 검증(`NEXUS_SCRIPT`)용. 새 이름을 주면 그것으로.
    pub(crate) fn begin_selected(&mut self, kind: OpKind, target: Option<&str>) -> bool {
        let Some(asset) = self
            .selected
            .as_ref()
            .and_then(|k| self.assets.iter().find(|a| &a.key == k))
            .cloned()
        else {
            return false;
        };
        self.begin(kind, &asset);
        if let (Some(t), Some(d)) = (target, self.op_dialog.as_mut()) {
            d.target = t.to_owned();
        }
        true
    }

    /// 확인 창의 "실행" — 자동 검증용. 계획이 서지 않으면 이유를 돌려준다.
    pub(crate) fn confirm_op(&mut self) -> Result<ContentAction, String> {
        let dialog = self
            .op_dialog
            .as_mut()
            .ok_or_else(|| String::from("열린 파일 작업이 없습니다"))?;
        dialog.plan().clone()?;
        let op = dialog.op();
        self.op_dialog = None;
        Ok(ContentAction::FileOp(op))
    }

    /// 파일 작업이 끝났다 — 목록을 다시 읽는다. 옮겨진 항목이면 새 자리를 고른다.
    pub(crate) fn after_op(&mut self, op: &FileOp, moved_to: Option<&str>) {
        if op.kind == OpKind::Delete && self.selected.as_deref() == Some(op.asset.key.as_str()) {
            self.selected = None;
        }
        if let Some(to) = moved_to {
            self.selected = Some(to.to_owned());
        }
        self.refresh();
    }

    /// 확인 창. "실행" 을 누르면 할 일을 돌려준다.
    fn op_window(&mut self, ui: &mut egui::Ui) -> Option<ContentAction> {
        let dialog = self.op_dialog.as_mut()?;
        let mut open = true;
        let mut run = false;
        let mut cancel = false;
        egui::Window::new(format!("{} — {}", dialog.kind.label(), dialog.asset.name))
            .id(egui::Id::new("content_op"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(420.0)
            .show(ui.ctx(), |ui| {
                ui.weak(&dialog.asset.key);
                if dialog.kind != OpKind::Delete {
                    ui.horizontal(|ui| {
                        ui.label(if dialog.asset.kind.is_virtual() {
                            "새 번호"
                        } else {
                            "새 이름"
                        });
                        ui.add(egui::TextEdit::singleline(&mut dialog.target).desired_width(200.0));
                    });
                }
                ui.separator();
                match dialog.plan() {
                    Ok(plan) => {
                        ui.label("할 일");
                        for step in &plan.steps {
                            ui.label(format!("  • {step}"));
                        }
                    }
                    Err(e) => {
                        ui.colored_label(egui::Color32::from_rgb(240, 110, 100), e);
                    }
                }
                ui.add_space(4.0);
                if dialog.kind == OpKind::Delete {
                    ui.weak("지우지 않고 휴지통(trash/)으로 옮깁니다 — 되살리려면 원래 자리로 옮기면 됩니다.");
                }
                ui.horizontal(|ui| {
                    let ok = dialog.cached.as_ref().is_some_and(|(_, p)| p.is_ok());
                    if ui.add_enabled(ok, egui::Button::new("실행")).clicked() {
                        run = true;
                    }
                    if ui.button("취소").clicked() {
                        cancel = true;
                    }
                });
            });
        if run {
            return self.confirm_op().ok();
        }
        if cancel || !open {
            self.op_dialog = None;
        }
        None
    }

    /// 하단 패널을 그린다. 두 번 누른 항목이 있으면 할 일을 돌려준다.
    pub(crate) fn show(&mut self, ui: &mut egui::Ui) -> Option<ContentAction> {
        if !self.open {
            return None;
        }
        let mut action = self.op_window(ui);
        let entries = self.entries();
        egui::Panel::bottom("content_browser")
            .resizable(true)
            .default_size(270.0)
            .min_size(140.0)
            .show(ui, |ui| {
                // 패널 안에 패널 — 도구 줄·주소 줄(위) · 상태 줄(아래) · 폴더 트리(왼쪽) ·
                // 상세(오른쪽) · 내용(가운데). 가운데를 마지막에 두어야 남은 폭을 다 쓴다.
                egui::Panel::top("content_toolbar").show(ui, |ui| {
                    action = self.toolbar(ui);
                    self.address_bar(ui);
                });
                egui::Panel::bottom("content_status").show(ui, |ui| {
                    self.status_line(ui, entries.len());
                });
                egui::Panel::left("content_tree")
                    .resizable(true)
                    .default_size(190.0)
                    .show(ui, |ui| {
                        let mut nav = None;
                        egui::ScrollArea::vertical()
                            .id_salt("content_tree")
                            .auto_shrink([false, false])
                            .show(ui, |ui| self.tree(ui, "", &mut nav, &mut action));
                        self.reveal = false;
                        if let Some(f) = nav {
                            self.navigate(&f);
                            // 트리에서 고른 폴더는 펼침 상태를 사용자가 정한다.
                            self.reveal = false;
                        }
                    });
                egui::Panel::right("content_details")
                    .resizable(true)
                    .default_size(320.0)
                    .show(ui, |ui| {
                        if let Some(a) = self.details_panel(ui) {
                            action = Some(a);
                        }
                    });
                egui::CentralPanel::default().show(ui, |ui| {
                    if let Some(a) = self.contents(ui, &entries) {
                        action = Some(a);
                    }
                });
            });
        action
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) -> Option<ContentAction> {
        let mut action = None;
        ui.horizontal(|ui| {
            ui.strong("콘텐츠 브라우저");
            // 오른쪽 클릭 메뉴와 같은 목록.
            ui.menu_button("새로 만들기", |ui| {
                action = create_menu(ui, &self.folder);
            });
            if ui.button("새로 고침").clicked() {
                self.refresh();
            }
            ui.separator();
            ui.label("보기");
            ui.selectable_value(&mut self.view, View::Icons, "아이콘");
            ui.selectable_value(&mut self.view, View::List, "자세히");
            ui.separator();
            ui.label("종류");
            egui::ComboBox::from_id_salt("content_kind")
                .selected_text(self.kind.map_or("모든 종류", AssetKind::label))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.kind, None, "모든 종류");
                    for kind in AssetKind::ALL {
                        let n = self.assets.iter().filter(|a| a.kind == kind).count();
                        ui.selectable_value(
                            &mut self.kind,
                            Some(kind),
                            format!("{} ({n})", kind.label()),
                        );
                    }
                });
            ui.weak("한 번 = 상세 · 두 번 = 편집기(폴더는 들어가기) · Ctrl+Space = 닫기");
        });
        if let Some(e) = &self.error {
            ui.colored_label(egui::Color32::from_rgb(240, 110, 100), e);
        }
        action
    }

    /// 주소 줄 — 뒤로 · 위로 · 경로 조각, 오른쪽에 검색.
    fn address_bar(&mut self, ui: &mut egui::Ui) {
        let mut nav: Option<String> = None;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.back.is_empty(), egui::Button::new("◀"))
                .on_hover_text("뒤로")
                .clicked()
            {
                self.go_back();
            }
            if ui
                .add_enabled(!self.folder.is_empty(), egui::Button::new("▲"))
                .on_hover_text("위 폴더로")
                .clicked()
            {
                nav = Some(parent(&self.folder).to_owned());
            }
            // 경로 조각 — 누르면 그 폴더로.
            let mut crumbs = vec![String::new()];
            let mut acc = String::new();
            for part in self.folder.split('/').filter(|p| !p.is_empty()) {
                if !acc.is_empty() {
                    acc.push('/');
                }
                acc.push_str(part);
                crumbs.push(acc.clone());
            }
            for (i, crumb) in crumbs.iter().enumerate() {
                if i > 0 {
                    ui.weak("›");
                }
                let last = i + 1 == crumbs.len();
                if ui.selectable_label(last, folder_name(crumb)).clicked() && !last {
                    nav = Some(crumb.clone());
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text("이 폴더 아래에서 검색")
                        .desired_width(200.0),
                );
            });
        });
        if let Some(f) = nav {
            self.navigate(&f);
        }
    }

    fn status_line(&self, ui: &mut egui::Ui, count: usize) {
        ui.horizontal(|ui| {
            ui.weak(format!("{count}개 항목"));
            if self.kind.is_some() || !self.search.trim().is_empty() {
                ui.weak(format!(
                    "· '{}' 아래 전체에서 찾음",
                    folder_name(&self.folder)
                ));
            }
            if let Some(a) = self
                .selected
                .as_ref()
                .and_then(|k| self.assets.iter().find(|a| &a.key == k))
            {
                ui.weak(format!("· 선택: {}", a.name));
            }
        });
    }

    /// 왼쪽 — 폴더 트리. 누른 폴더는 `nav` 로, 오른쪽 클릭 메뉴에서 고른 것은 `act` 로 돌려준다.
    fn tree(
        &self,
        ui: &mut egui::Ui,
        folder: &str,
        nav: &mut Option<String>,
        act: &mut Option<ContentAction>,
    ) {
        // 폴더 이름 오른쪽 클릭 — 그 폴더로 가면서 그 폴더에 맞는 새로 만들기 메뉴.
        let menu =
            |r: egui::Response, nav: &mut Option<String>, act: &mut Option<ContentAction>| {
                if r.clicked() {
                    *nav = Some(folder.to_owned());
                }
                let _ = r.context_menu(|ui| {
                    if ui.button("이 폴더 열기").clicked() {
                        *nav = Some(folder.to_owned());
                        ui.close();
                    }
                    ui.separator();
                    if let Some(a) = create_menu(ui, folder) {
                        *act = Some(a);
                    }
                });
            };
        let kids = self.subfolders(folder);
        let picked = self.folder == folder;
        let count = self
            .assets
            .iter()
            .filter(|a| is_under(&a.folder(), folder))
            .count();
        let text = format!("{} ({count})", folder_name(folder));
        let text = if folder == VIRTUAL_ROOT || parent(folder) == VIRTUAL_ROOT {
            // 가상 폴더 — 디스크에 없다는 표시로 색을 다르게.
            egui::RichText::new(text).color(VIRTUAL_COLOR)
        } else {
            egui::RichText::new(text)
        };
        if kids.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(ui.spacing().indent);
                menu(ui.selectable_label(picked, text), nav, act);
            });
            return;
        }
        let id = ui.make_persistent_id(("content_tree", folder));
        let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            id,
            folder.is_empty(),
        );
        // 이동한 직후 — 지금 폴더까지 펼친다.
        if self.reveal && self.folder != folder && is_under(&self.folder, folder) {
            state.set_open(true);
        }
        state
            .show_header(ui, |ui| {
                menu(ui.selectable_label(picked, text), nav, act);
            })
            .body(|ui| {
                for kid in &kids {
                    self.tree(ui, kid, nav, act);
                }
            });
    }

    /// 빈 곳 오른쪽 클릭 — 이 폴더에 맞는 새로 만들기 · 편집기 열기 · 보기 · 새로 고침.
    fn background_menu(&mut self, bg: &egui::Response, action: &mut Option<ContentAction>) {
        let mut popup = egui::Popup::context_menu(bg);
        if std::mem::take(&mut self.menu_request) {
            self.menu_at = Some(bg.rect.left_top() + egui::vec2(40.0, 24.0));
            popup = popup.open_memory(Some(egui::containers::SetOpenCommand::Bool(true)));
        }
        if let Some(at) = self.menu_at {
            popup = popup.at_position(at);
        }
        let folder = self.folder.clone();
        let (mut view, mut refresh) = (None, false);
        let shown = popup.show(|ui| {
            if let Some(a) = create_menu(ui, &folder) {
                *action = Some(a);
            }
            ui.separator();
            for (label, v) in [("아이콘 보기", View::Icons), ("자세히 보기", View::List)]
            {
                if ui.button(label).clicked() {
                    view = Some(v);
                    ui.close();
                }
            }
            if ui.button("새로 고침").clicked() {
                refresh = true;
                ui.close();
            }
        });
        if shown.is_none() {
            self.menu_at = None;
        }
        if let Some(v) = view {
            self.view = v;
        }
        if refresh {
            self.refresh();
        }
    }

    /// 빈 곳 오른쪽 클릭 메뉴를 연다 — 자동 검증용.
    pub(crate) fn open_menu(&mut self) {
        self.open = true;
        if !self.loaded {
            self.refresh();
        }
        self.menu_request = true;
    }

    /// 가운데 — 지금 폴더의 내용 (아이콘 / 자세히).
    fn contents(&mut self, ui: &mut egui::Ui, entries: &[Entry]) -> Option<ContentAction> {
        let mut action = None;
        let mut nav = None;
        // 빈 곳 — 항목보다 **먼저** 등록해야 항목이 위에서 클릭을 받는다 (egui 는 나중 것이 위).
        let bg = ui.interact(
            ui.available_rect_before_wrap(),
            ui.id().with("content_bg"),
            egui::Sense::click(),
        );
        if bg.clicked() {
            // 탐색기처럼 빈 곳을 누르면 선택이 풀린다.
            self.selected = None;
            self.details = None;
        }
        self.background_menu(&bg, &mut action);
        egui::ScrollArea::vertical()
            .id_salt("content_list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if entries.is_empty() {
                    ui.weak("항목이 없습니다.");
                    return;
                }
                match self.view {
                    View::Icons => {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                            for entry in entries {
                                self.tile(ui, entry, &mut action, &mut nav);
                            }
                        });
                    }
                    View::List => {
                        let filtering = self.kind.is_some() || !self.search.trim().is_empty();
                        let cols = Columns::new(ui.available_width());
                        list_header(ui, &cols, filtering);
                        for entry in entries {
                            self.row(ui, entry, &cols, &mut action, &mut nav);
                        }
                    }
                }
            });
        if let Some(f) = nav {
            self.navigate(&f);
            self.search.clear();
        }
        action
    }

    /// 아이콘 보기의 칸 하나.
    fn tile(
        &mut self,
        ui: &mut egui::Ui,
        entry: &Entry,
        action: &mut Option<ContentAction>,
        nav: &mut Option<String>,
    ) {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(88.0, 100.0), egui::Sense::click());
        if ui.is_rect_visible(rect) {
            self.paint_back(ui, rect, entry, &resp);
            let icon = egui::Rect::from_center_size(
                egui::pos2(rect.center().x, rect.min.y + 34.0),
                egui::vec2(56.0, 56.0),
            );
            self.paint_icon(ui, icon, entry);
            let mut job = egui::text::LayoutJob::simple(
                entry_name(entry).to_owned(),
                egui::FontId::proportional(12.0),
                ui.visuals().text_color(),
                rect.width() - 6.0,
            );
            job.wrap.max_rows = 2;
            job.wrap.break_anywhere = true;
            job.halign = egui::Align::Center;
            let galley = ui.painter().layout_job(job);
            ui.painter().galley(
                egui::pos2(rect.center().x, rect.min.y + 66.0),
                galley,
                ui.visuals().text_color(),
            );
        }
        self.interact(resp, entry, action, nav);
    }

    /// 자세히 보기의 줄 하나.
    fn row(
        &mut self,
        ui: &mut egui::Ui,
        entry: &Entry,
        cols: &Columns,
        action: &mut Option<ContentAction>,
        nav: &mut Option<String>,
    ) {
        let (rect, resp) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 20.0), egui::Sense::click());
        if ui.is_rect_visible(rect) {
            self.paint_back(ui, rect, entry, &resp);
            let icon =
                egui::Rect::from_min_size(rect.min + egui::vec2(4.0, 2.0), egui::vec2(16.0, 16.0));
            self.paint_icon(ui, icon, entry);
            let (kind, size, note) = match entry {
                Entry::Folder(f) => {
                    let n = self
                        .assets
                        .iter()
                        .filter(|a| is_under(&a.folder(), f))
                        .count();
                    (String::from("폴더"), format!("{n}개"), String::new())
                }
                Entry::Asset(a) => (
                    a.kind.label().to_owned(),
                    a.bytes.map(size_text).unwrap_or_default(),
                    a.note.clone(),
                ),
            };
            let text = ui.visuals().text_color();
            let weak = ui.visuals().weak_text_color();
            cols.paint(
                ui,
                rect,
                [entry_name(entry), &kind, &size, &note],
                [text, weak, weak, weak],
            );
        }
        self.interact(resp, entry, action, nav);
    }

    /// 고른 칸·마우스가 올라간 칸의 바탕.
    fn paint_back(&self, ui: &egui::Ui, rect: egui::Rect, entry: &Entry, resp: &egui::Response) {
        let picked =
            matches!(entry, Entry::Asset(a) if self.selected.as_deref() == Some(a.key.as_str()));
        let fill = if picked {
            Some(ui.visuals().selection.bg_fill)
        } else if resp.hovered() {
            Some(ui.visuals().widgets.hovered.weak_bg_fill)
        } else {
            None
        };
        if let Some(fill) = fill {
            ui.painter().rect_filled(rect, 3.0, fill);
        }
    }

    /// 아이콘 — 폴더 모양, 그림 썸네일, 또는 종류 색 네모 + 짧은 글씨.
    fn paint_icon(&mut self, ui: &egui::Ui, rect: egui::Rect, entry: &Entry) {
        let painter = ui.painter();
        let (w, h) = (rect.width(), rect.height());
        match entry {
            Entry::Folder(f) => {
                let color = if f == VIRTUAL_ROOT || parent(f) == VIRTUAL_ROOT {
                    VIRTUAL_COLOR
                } else {
                    FOLDER_COLOR
                };
                let tab = egui::Rect::from_min_size(
                    rect.min + egui::vec2(0.0, h * 0.12),
                    egui::vec2(w * 0.42, h * 0.2),
                );
                let body = egui::Rect::from_min_max(
                    egui::pos2(rect.min.x, rect.min.y + h * 0.24),
                    egui::pos2(rect.max.x, rect.max.y - h * 0.08),
                );
                painter.rect_filled(tab, 2.0, color.gamma_multiply(0.8));
                painter.rect_filled(body, 3.0, color);
            }
            Entry::Asset(a) => {
                let thumb = (a.kind == AssetKind::Image)
                    .then(|| self.thumbs.get(ui.ctx(), &a.key))
                    .flatten();
                if let Some((tex, size)) = thumb {
                    let fitted = egui::Rect::from_center_size(rect.center(), fit(size, w));
                    egui::Image::new((tex, fitted.size())).paint_at(ui, fitted);
                    return;
                }
                let color = a.kind.color();
                let body = rect.shrink2(egui::vec2(w * 0.1, 0.0));
                painter.rect_filled(body, 4.0, color.gamma_multiply(0.22));
                painter.rect_stroke(
                    body,
                    4.0,
                    egui::Stroke::new(if h >= 32.0 { 1.5 } else { 1.0 }, color),
                    egui::StrokeKind::Inside,
                );
                if h >= 32.0 {
                    painter.text(
                        body.center(),
                        egui::Align2::CENTER_CENTER,
                        a.kind.badge(),
                        egui::FontId::proportional(h * 0.24),
                        color,
                    );
                }
            }
        }
    }

    /// 누르기 — 폴더는 두 번 눌러 들어가고, 항목은 한 번 = 상세 · 두 번 = 편집기 · 오른쪽 = 파일 작업.
    fn interact(
        &mut self,
        resp: egui::Response,
        entry: &Entry,
        action: &mut Option<ContentAction>,
        nav: &mut Option<String>,
    ) {
        match entry {
            Entry::Folder(f) => {
                let resp = resp.on_hover_text(if f.is_empty() { "프로젝트" } else { f });
                if resp.double_clicked() {
                    *nav = Some(f.clone());
                }
                let _ = resp.context_menu(|ui| {
                    if ui.button("열기").clicked() {
                        *nav = Some(f.clone());
                        ui.close();
                    }
                });
            }
            Entry::Asset(asset) => {
                let resp = resp.on_hover_text(&asset.key);
                if resp.clicked() || resp.secondary_clicked() {
                    self.selected = Some(asset.key.clone());
                    self.details = Some(details(Path::new("."), asset, &self.assets, &self.forms));
                }
                if resp.double_clicked() {
                    *action = Some(open_action(asset));
                }
                // 오른쪽 클릭 — 열기 + 파일 작업 (P9).
                let _ = resp.context_menu(|ui| {
                    if ui.button("열기").clicked() {
                        *action = Some(open_action(asset));
                        ui.close();
                    }
                    ui.separator();
                    for kind in [OpKind::Duplicate, OpKind::Rename, OpKind::Delete] {
                        if ui
                            .add_enabled(
                                supports(asset.kind, kind),
                                egui::Button::new(format!("{}…", kind.label())),
                            )
                            .clicked()
                        {
                            self.begin(kind, asset);
                            ui.close();
                        }
                    }
                });
            }
        }
    }

    /// 오른쪽 — 고른 항목의 상세.
    fn details_panel(&mut self, ui: &mut egui::Ui) -> Option<ContentAction> {
        let Some(key) = self.selected.clone() else {
            ui.weak("항목을 고르세요.");
            return None;
        };
        let asset = self.assets.iter().find(|a| a.key == key)?.clone();
        let details = self.details.clone().unwrap_or_default();
        let mut action = None;
        egui::ScrollArea::vertical()
            .id_salt("content_details")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(asset.kind.color(), asset.kind.label());
                    ui.strong(&asset.name);
                });
                if let Some(path) = &details.preview
                    && let Some((tex, size)) = self.thumbs.get(ui.ctx(), path)
                {
                    ui.image((tex, fit(size, 96.0)));
                }
                egui::Grid::new("content_rows")
                    .num_columns(2)
                    .spacing([8.0, 2.0])
                    .show(ui, |ui| {
                        for (k, v) in &details.rows {
                            ui.weak(k);
                            ui.label(v);
                            ui.end_row();
                        }
                    });
                ui.add_space(4.0);
                ui.label("쓰는 곳");
                if details.used_by.is_empty() {
                    ui.weak("  (찾지 못함)");
                }
                for who in &details.used_by {
                    ui.weak(format!("  {who}"));
                }
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("열기").clicked() {
                        action = Some(open_action(&asset));
                    }
                    for kind in [OpKind::Duplicate, OpKind::Rename, OpKind::Delete] {
                        if ui
                            .add_enabled(
                                supports(asset.kind, kind),
                                egui::Button::new(kind.label()),
                            )
                            .clicked()
                        {
                            self.begin(kind, &asset);
                        }
                    }
                });
            });
        action
    }
}

/// 오른쪽 클릭 메뉴의 "새로 만들기" 항목 — 도구 줄의 *새로 만들기* 와 같은 목록이다.
/// 파일 이름·번호는 각 편집기에서 정한다 (여기서는 편집기를 새 항목 상태로 열 뿐이다).
fn create_items() -> [(&'static str, ContentAction); 5] {
    [
        ("레벨…", ContentAction::NewLevel),
        ("액터…", ContentAction::NewActor),
        ("아이템…", ContentAction::NewTable(Tab::Items)),
        ("스킬…", ContentAction::NewTable(Tab::Skills)),
        ("드롭 표…", ContentAction::NewTable(Tab::Loot)),
    ]
}

/// 창 안에 "새로 만들기" 가 있는 편집기들 — 창을 열어 그 안에서 만든다.
fn editor_items() -> [(&'static str, ContentAction); 4] {
    [
        (
            "게임 규칙 설정 (진영·기본 액터·시작 소지품)",
            ContentAction::EditRules,
        ),
        (
            "스크립트 편집기 (새 스크립트·편집)",
            ContentAction::NewScript,
        ),
        ("위젯 편집기 (UI 화면 디자인)", ContentAction::NewScreen),
        (
            "지형 팔레트 (타일·건물 조각)",
            ContentAction::OpenPalette(String::new()),
        ),
    ]
}

/// 이 폴더에서 가장 그럴듯한 새 리소스 — 메뉴 맨 위에 굵게 보인다.
fn suggestion(folder: &str) -> Option<(&'static str, ContentAction)> {
    let actor = format!("{VIRTUAL_ROOT}/{}", AssetKind::Actor.label());
    let item = format!("{VIRTUAL_ROOT}/{}", AssetKind::Item.label());
    let skill = format!("{VIRTUAL_ROOT}/{}", AssetKind::Skill.label());
    let loot = format!("{VIRTUAL_ROOT}/{}", AssetKind::Loot.label());
    Some(match folder {
        f if is_under(f, "levels") && !f.is_empty() => ("새 레벨…", ContentAction::NewLevel),
        f if is_under(f, "ui") && !f.is_empty() => {
            ("새 UI 화면… (위젯 편집기)", ContentAction::NewScreen)
        }
        f if is_under(f, "data/scripts") && !f.is_empty() => {
            ("새 스크립트… (스크립트 편집기)", ContentAction::NewScript)
        }
        f if f == actor => ("새 액터…", ContentAction::NewActor),
        f if f == item => ("새 아이템…", ContentAction::NewTable(Tab::Items)),
        f if f == skill => ("새 스킬…", ContentAction::NewTable(Tab::Skills)),
        f if f == loot => ("새 드롭 표…", ContentAction::NewTable(Tab::Loot)),
        _ => return None,
    })
}

/// 오른쪽 클릭 메뉴 — 이 폴더에 맞는 것 먼저, 그다음 새로 만들기 · 편집기 열기.
/// 고른 것을 돌려준다.
fn create_menu(ui: &mut egui::Ui, folder: &str) -> Option<ContentAction> {
    let mut picked = None;
    let mut item = |ui: &mut egui::Ui, text: egui::RichText, a: ContentAction| {
        if ui.button(text).clicked() {
            picked = Some(a);
            ui.close();
        }
    };
    if let Some((label, a)) = suggestion(folder) {
        item(ui, egui::RichText::new(label).strong(), a);
        ui.separator();
    }
    ui.weak("새로 만들기");
    for (label, a) in create_items() {
        item(ui, egui::RichText::new(label), a);
    }
    ui.separator();
    ui.weak("편집기 열기");
    for (label, a) in editor_items() {
        item(ui, egui::RichText::new(label), a);
    }
    picked
}

/// 폴더 아이콘 색 (sRGB).
const FOLDER_COLOR: egui::Color32 = egui::Color32::from_rgb(225, 185, 85);
/// 가상 폴더(게임 데이터) 색 — 디스크에 없는 폴더라는 표시.
const VIRTUAL_COLOR: egui::Color32 = egui::Color32::from_rgb(140, 170, 235);

/// 항목들이 든 폴더 전부 — 조상까지 채워 넣는다 (`assets/third_party/x` 면 `assets`·`assets/third_party` 도).
/// 맨 위(`""`)는 넣지 않는다.
fn folders_of(assets: &[Asset]) -> Vec<String> {
    let mut set = std::collections::BTreeSet::new();
    for a in assets {
        let mut f = a.folder();
        while !f.is_empty() {
            let up = parent(&f).to_owned();
            set.insert(f);
            f = up;
        }
    }
    set.into_iter().collect()
}

fn entry_name(entry: &Entry) -> &str {
    match entry {
        Entry::Folder(f) => folder_name(f),
        Entry::Asset(a) => &a.name,
    }
}

/// 자세히 보기의 칸 경계 — 이름 · 종류 · 크기 · 경로.
struct Columns {
    x: [f32; 4],
    width: f32,
}

impl Columns {
    fn new(width: f32) -> Self {
        let name = 24.0;
        let kind = (width * 0.38).max(180.0);
        let size = kind + 110.0;
        let note = size + 80.0;
        Self {
            x: [name, kind, size, note],
            width,
        }
    }

    /// 칸마다 글씨를 그린다 — 칸을 넘는 글씨는 잘린다.
    fn paint(&self, ui: &egui::Ui, rect: egui::Rect, texts: [&str; 4], colors: [egui::Color32; 4]) {
        for i in 0..4 {
            let left = rect.min.x + self.x[i];
            let right = if i + 1 < 4 {
                rect.min.x + self.x[i + 1] - 8.0
            } else {
                rect.min.x + self.width
            };
            if right <= left {
                continue;
            }
            let clip = egui::Rect::from_x_y_ranges(left..=right, rect.y_range());
            ui.painter().with_clip_rect(clip).text(
                egui::pos2(left, rect.center().y),
                egui::Align2::LEFT_CENTER,
                texts[i],
                egui::FontId::proportional(13.0),
                colors[i],
            );
        }
    }
}

fn list_header(ui: &mut egui::Ui, cols: &Columns, filtering: bool) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 18.0), egui::Sense::hover());
    let weak = ui.visuals().weak_text_color();
    let last = if filtering { "경로" } else { "메모" };
    cols.paint(ui, rect, ["이름", "종류", "크기", last], [weak; 4]);
    ui.painter().hline(
        rect.x_range(),
        rect.max.y,
        egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
    );
}

/// 표 항목 → 데이터 표 편집기.
fn table_action(asset: &Asset, tab: Tab) -> ContentAction {
    asset.entry_id().map_or(ContentAction::NewTable(tab), |id| {
        ContentAction::EditTable(tab, id)
    })
}

/// 파일 이름에서 확장자를 뗀 것 — 새 이름 칸의 기본값. 스크립트는 `goblin`, 액터는 번호.
fn file_stem(asset: &Asset) -> String {
    if let Some(id) = asset.file_id() {
        return id.to_owned();
    }
    let name = asset.key.rsplit('/').next().unwrap_or(&asset.key);
    name.split('.').next().unwrap_or(name).to_owned()
}

/// 이미지 크기를 `max` 픽셀 안에 맞춘다 (비율 유지, 픽셀아트라 정수배 우선은 하지 않는다).
fn fit(size: [u32; 2], max: f32) -> egui::Vec2 {
    let (w, h) = (size[0].max(1) as f32, size[1].max(1) as f32);
    let s = (max / w.max(h)).min(4.0);
    egui::vec2(w * s, h * s)
}

/// 두 번 눌렀을 때 — 종류마다 맞는 편집기.
pub(crate) fn open_action(asset: &Asset) -> ContentAction {
    match asset.kind {
        AssetKind::Zone => ContentAction::OpenZone(PathBuf::from(&asset.key)),
        AssetKind::Level => {
            ContentAction::EditLevel(asset.file_id().unwrap_or_default().to_owned())
        }
        AssetKind::Screen => {
            ContentAction::EditScreen(asset.file_id().unwrap_or_default().to_owned())
        }
        AssetKind::Script => ContentAction::EditScript(
            asset
                .key
                .strip_prefix("data/")
                .unwrap_or(&asset.key)
                .to_owned(),
        ),
        AssetKind::Image => ContentAction::OpenPalette(asset.key.clone()),
        AssetKind::Sheet => ContentAction::ViewSheet(asset.key.clone()),
        AssetKind::Actor => asset
            .actor_id()
            .map_or(ContentAction::NewActor, ContentAction::EditActor),
        AssetKind::Item => table_action(asset, Tab::Items),
        AssetKind::Skill => table_action(asset, Tab::Skills),
        AssetKind::Loot => table_action(asset, Tab::Loot),
        AssetKind::Data => match asset.key.as_str() {
            // 표를 통째로 여는 편집기는 없다 — 그 표의 항목을 고치는 편집기로 간다.
            // 표의 항목(액터·아이템·스킬·드롭 표)은 "게임 데이터" 폴더에서 하나씩 연다.
            // 파일을 두 번 누르면 항목이 아닌 한 벌짜리 값(시드·기본 액터·진영·시작 소지품)을 고친다.
            "data/rules.ron" => ContentAction::EditRules,
            "data/display.ron" => ContentAction::Notice(String::from(
                "이름·그림·색은 '게임 데이터' 폴더의 각 항목 편집기에서 고칩니다",
            )),
            "data/terrain.ron" => ContentAction::OpenPalette(String::new()),
            "data/ui.ron" => ContentAction::EditScreen(String::from("hud")),
            "data/project.ron" => ContentAction::Notice(String::from(
                "시작 레벨은 레벨 편집기의 '시작 레벨로 지정' 으로 바꿉니다",
            )),
            other => ContentAction::Notice(format!("{other} 는 직접 고치는 파일입니다")),
        },
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 목록 모으기 — 디스크를 훑는다 (root 기준이라 테스트가 저장소 루트로 부른다)
// ─────────────────────────────────────────────────────────────────────────────

/// 리소스 목록과 액터 폼을 모은다. 액터 표를 못 읽으면 오류로 알리고 나머지는 그대로 보인다.
fn collect(root: &Path) -> (Vec<Asset>, Forms, Option<String>) {
    let mut out = Vec::new();

    for path in scan_files(root, "levels", ".level.ron") {
        let name = read(root, &path)
            .and_then(|t| crate::level::read_level(&t).ok())
            .map_or_else(|| stem(&path, ".level.ron"), |l| l.name);
        out.push(file_asset(root, AssetKind::Level, path, name));
    }
    for path in scan_files(root, "zones", ".zone.ron") {
        let name = stem(&path, ".zone.ron");
        out.push(file_asset(root, AssetKind::Zone, path, name));
    }
    for path in scan_files(root, "ui", ".ui.ron") {
        let name = read(root, &path)
            .and_then(|t| crate::screen::Screens::read_screen(&t).ok())
            .map_or_else(|| stem(&path, ".ui.ron"), |s| s.name);
        out.push(file_asset(root, AssetKind::Screen, path, name));
    }
    for path in scan_files(root, "data/scripts", ".rhai") {
        let name = stem(&path, ".rhai");
        out.push(file_asset(root, AssetKind::Script, path, name));
    }
    for path in scan_files(root, "assets", ".sheet.ron") {
        let name = stem(&path, ".sheet.ron");
        out.push(file_asset(root, AssetKind::Sheet, path, name));
    }
    for path in scan_files(root, "assets", ".png") {
        let name = stem(&path, ".png");
        out.push(file_asset(root, AssetKind::Image, path, name));
    }
    // 데이터는 data/ 바로 아래의 .ron 만 (스크립트 폴더는 따로 보인다).
    for path in scan_files(root, "data", ".ron")
        .into_iter()
        .filter(|p| p.matches('/').count() == 1)
    {
        let name = stem(&path, ".ron");
        out.push(file_asset(root, AssetKind::Data, path, name));
    }

    // 액터 — 두 표의 항목을 가상 항목으로.
    let (forms, error) = match (
        read(root, crate::game_data::RULES_PATH),
        read(root, crate::game_data::DISPLAY_PATH),
    ) {
        (Some(rules), Some(display)) => {
            match (actor_forms(&rules, &display), table_forms(&rules, &display)) {
                (Ok(actors), Ok(tables)) => (Forms { actors, tables }, None),
                (Err(e), _) | (_, Err(e)) => (
                    Forms::default(),
                    Some(format!("데이터 표를 읽지 못함 — {e}")),
                ),
            }
        }
        _ => (Forms::default(), None),
    };
    for (id, form) in &forms.actors {
        let name = if form.name.is_empty() {
            format!("(이름 없음 #{id})")
        } else {
            form.name.clone()
        };
        out.push(Asset {
            kind: AssetKind::Actor,
            key: format!("actor:{id}"),
            name,
            note: format!("#{id}"),
            bytes: None,
        });
    }
    // 아이템·스킬·드롭 표 — 같은 두 표의 다른 칸들.
    let named = |name: &str, id: u32, what: &str| {
        if name.is_empty() {
            format!("({what} #{id})")
        } else {
            name.to_owned()
        }
    };
    for (id, f) in &forms.tables.items {
        out.push(Asset {
            kind: AssetKind::Item,
            key: format!("item:{id}"),
            name: named(&f.name, *id, "아이템"),
            note: format!("#{id}"),
            bytes: None,
        });
    }
    for (id, f) in &forms.tables.skills {
        out.push(Asset {
            kind: AssetKind::Skill,
            key: format!("skill:{id}"),
            name: named(&f.name, *id, "스킬"),
            note: format!("#{id}"),
            bytes: None,
        });
    }
    for (id, rows) in &forms.tables.loot {
        out.push(Asset {
            kind: AssetKind::Loot,
            key: format!("loot:{id}"),
            name: format!("드롭 표 #{id}"),
            note: format!("{}줄", rows.len()),
            bytes: None,
        });
    }
    (out, forms, error)
}

fn file_asset(root: &Path, kind: AssetKind, path: String, name: String) -> Asset {
    Asset {
        kind,
        bytes: std::fs::metadata(root.join(&path)).ok().map(|m| m.len()),
        note: path.clone(),
        key: path,
        name,
    }
}

/// `root/dir` 아래에서 `suffix` 로 끝나는 파일을 모두 — `dir/…` 모양 경로, `/` 구분, 이름순.
///
/// 경로를 `/` 로 맞추는 이유: 데이터 파일에 그대로 적히고, 그 파일은 두 OS 가 같이 쓴다.
pub(crate) fn scan_files(root: &Path, dir: &str, suffix: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![PathBuf::from(dir)];
    while let Some(rel) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(root.join(&rel)) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let child = rel.join(&name);
            if entry.path().is_dir() {
                stack.push(child);
            } else if name.to_ascii_lowercase().ends_with(suffix) {
                found.push(slash(&child));
            }
        }
    }
    found.sort();
    found
}

fn slash(path: &Path) -> String {
    path.components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn stem(path: &str, suffix: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.strip_suffix(suffix).unwrap_or(name).to_owned()
}

fn read(root: &Path, path: &str) -> Option<String> {
    std::fs::read_to_string(root.join(path)).ok()
}

// ─────────────────────────────────────────────────────────────────────────────
// 상세와 참조
// ─────────────────────────────────────────────────────────────────────────────

fn details(root: &Path, asset: &Asset, assets: &[Asset], forms: &Forms) -> Details {
    let mut rows: Vec<(String, String)> = Vec::new();
    let mut preview = None;
    let mut row = |k: &str, v: String| rows.push((k.to_owned(), v));

    // 표 항목(액터·아이템·스킬·드롭 표)은 파일이 아니라 경로가 없다.
    let virtual_entry = matches!(
        asset.kind,
        AssetKind::Actor | AssetKind::Item | AssetKind::Skill | AssetKind::Loot
    );
    if !virtual_entry {
        row("경로", asset.key.clone());
        if let Ok(meta) = std::fs::metadata(root.join(&asset.key)) {
            row("크기", size_text(meta.len()));
        }
    }
    match asset.kind {
        AssetKind::Level => {
            if let Some(level) =
                read(root, &asset.key).and_then(|t| crate::level::read_level(&t).ok())
            {
                row(
                    "존",
                    level
                        .zone
                        .clone()
                        .unwrap_or_else(|| String::from("없음 (UI 레벨)")),
                );
                row(
                    "HUD",
                    level.hud.clone().unwrap_or_else(|| String::from("—")),
                );
                row(
                    "들어갈 때",
                    level.on_enter.clone().unwrap_or_else(|| String::from("—")),
                );
                row(
                    "일시정지",
                    level.pause.clone().unwrap_or_else(|| String::from("—")),
                );
            }
        }
        AssetKind::Screen => {
            if let Some(screen) =
                read(root, &asset.key).and_then(|t| crate::screen::Screens::read_screen(&t).ok())
            {
                row("위젯", format!("{}개", count_widgets(&screen.widgets)));
            }
        }
        AssetKind::Script => {
            if let Some(text) = read(root, &asset.key) {
                row("줄", format!("{}", text.lines().count()));
                let hooks = match nexus_script::check(&text) {
                    Ok(c) => c.hooks.join(", "),
                    Err(e) => format!("컴파일 오류 — {e}"),
                };
                row("훅", hooks);
            }
        }
        AssetKind::Image => {
            if let Some(size) = image_size(root, &asset.key) {
                row("크기(픽셀)", format!("{} × {}", size[0], size[1]));
            }
            preview = Some(asset.key.clone());
        }
        AssetKind::Sheet => {
            if let Ok(info) = crate::sprites::inspect_sheet(root, &asset.key) {
                row("그림", info.image_label.clone());
                row("칸", format!("{} × {}", info.cell.0, info.cell.1));
                row("방향", format!("{}", info.directions));
                row(
                    "클립",
                    info.clips
                        .iter()
                        .map(|c| c.state.clone())
                        .collect::<Vec<_>>()
                        .join(", "),
                );
                preview = info.image_path.clone();
            }
        }
        AssetKind::Actor => {
            if let Some(id) = asset.actor_id()
                && let Some(f) = forms.actors.get(&id)
            {
                row("번호", format!("#{id}"));
                row(
                    "HP · 공격 · 방어",
                    format!("{} · {} · {}", f.max_hp, f.attack, f.defense),
                );
                row("AI", f.ai.label().to_owned());
                row(
                    "시트",
                    if f.sheet.is_empty() {
                        String::from("(내장 플레이스홀더)")
                    } else {
                        f.sheet.clone()
                    },
                );
                row(
                    "스크립트",
                    f.script.clone().unwrap_or_else(|| String::from("—")),
                );
            }
        }
        AssetKind::Item => {
            if let Some(f) = asset.entry_id().and_then(|id| forms.tables.items.get(&id)) {
                use crate::game_data::ItemKindForm;
                row("번호", format!("#{}", asset.entry_id().unwrap_or_default()));
                row(
                    "종류",
                    match f.kind {
                        ItemKindForm::Consumable { heal, max_stack } => {
                            format!("소모품 · 회복 {heal} · 최대 {max_stack}개")
                        }
                        ItemKindForm::Equipment {
                            slot,
                            attack,
                            defense,
                        } => format!("장비({}) · 공격 {attack} · 방어 {defense}", slot.label()),
                    },
                );
            }
        }
        AssetKind::Skill => {
            if let Some(f) = asset.entry_id().and_then(|id| forms.tables.skills.get(&id)) {
                row("번호", format!("#{}", asset.entry_id().unwrap_or_default()));
                row(
                    "사거리 · 쿨타임 · 배율",
                    format!("{}m · {}ms · ×{}", f.range, f.cooldown_ms, f.damage_mult),
                );
            }
        }
        AssetKind::Loot => {
            if let Some(lines) = asset.entry_id().and_then(|id| forms.tables.loot.get(&id)) {
                for r in lines {
                    let name = forms
                        .tables
                        .items
                        .get(&r.item)
                        .map_or_else(|| format!("#{}", r.item), |f| f.name.clone());
                    row(
                        &name,
                        format!("{}개 · {:.1}%", r.count, r.chance_per_mille as f32 / 10.0),
                    );
                }
            }
        }
        AssetKind::Zone | AssetKind::Data => {}
    }
    Details {
        rows,
        used_by: references(root, asset, assets, forms),
        preview,
    }
}

fn count_widgets(widgets: &[crate::screen::Widget]) -> usize {
    widgets.iter().map(|w| 1 + count_widgets(&w.children)).sum()
}

fn size_text(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    }
}

/// PNG 머리만 읽어 크기를 안다 — 전체 디코딩 없이 (IHDR 는 16~24 바이트에 있다).
fn image_size(root: &Path, path: &str) -> Option<[u32; 2]> {
    let bytes = std::fs::read(root.join(path)).ok()?;
    if bytes.len() < 24 || &bytes[1..4] != b"PNG" {
        return None;
    }
    let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    Some([w, h])
}

/// 이 리소스를 가리키는 곳 — 문자열 검색 (모듈 머리 주석의 한계 참고).
fn references(root: &Path, asset: &Asset, assets: &[Asset], forms: &Forms) -> Vec<String> {
    // 어떤 종류의 파일에서 어떤 문자열을 찾을지.
    let mut needles: Vec<(AssetKind, String)> = Vec::new();
    let mut out: Vec<String> = Vec::new();
    match asset.kind {
        AssetKind::Zone => needles.push((AssetKind::Level, format!("\"{}\"", asset.key))),
        AssetKind::Level => {
            let id = asset.file_id().unwrap_or_default();
            needles.push((AssetKind::Screen, format!("OpenLevel(\"{id}\")")));
            needles.push((AssetKind::Data, format!("startup_level: \"{id}\"")));
        }
        AssetKind::Screen => {
            let id = asset.file_id().unwrap_or_default();
            needles.push((AssetKind::Level, format!("Some(\"{id}\")")));
            needles.push((AssetKind::Screen, format!("OpenScreen(\"{id}\")")));
        }
        AssetKind::Script => {
            let rel = asset.key.strip_prefix("data/").unwrap_or(&asset.key);
            for (id, f) in &forms.actors {
                if f.script.as_deref() == Some(rel) {
                    out.push(format!("액터 #{id} {}", f.name));
                }
            }
        }
        AssetKind::Sheet => {
            for (id, f) in &forms.actors {
                if f.sheet == asset.key {
                    out.push(format!("액터 #{id} {}", f.name));
                }
            }
        }
        AssetKind::Image => {
            // 지형 타일셋·UI 테마는 경로 그대로, 시트 정의는 자기 폴더 기준 상대 경로일 수 있다.
            needles.push((AssetKind::Data, format!("\"{}\"", asset.key)));
            for sheet in assets.iter().filter(|a| a.kind == AssetKind::Sheet) {
                if let Ok(info) = crate::sprites::inspect_sheet(root, &sheet.key)
                    && info.image_path.as_deref() == Some(asset.key.as_str())
                {
                    out.push(sheet.key.clone());
                }
            }
        }
        AssetKind::Actor => {
            let id = asset.actor_id().unwrap_or_default();
            needles.push((AssetKind::Zone, format!("actor: {id},")));
        }
        AssetKind::Item => {
            let id = asset.entry_id().unwrap_or_default();
            for (loot, rows) in &forms.tables.loot {
                if rows.iter().any(|r| r.item == id) {
                    out.push(format!("드롭 표 #{loot}"));
                }
            }
            if forms.tables.starting_kit.contains(&id) {
                out.push(String::from("시작 소지품 (rules.ron)"));
            }
        }
        AssetKind::Skill => {
            let id = asset.entry_id().unwrap_or_default();
            for (a, f) in &forms.actors {
                if f.basic_attack == Some(id) {
                    out.push(format!("액터 #{a} {}", f.name));
                }
            }
            if forms.tables.player_attack == id {
                out.push(String::from("플레이어 공격 (rules.ron)"));
            }
        }
        AssetKind::Loot => {
            let id = asset.entry_id().unwrap_or_default();
            for (a, f) in &forms.actors {
                if f.loot == Some(id) {
                    out.push(format!("액터 #{a} {}", f.name));
                }
            }
        }
        AssetKind::Data => {}
    }
    for (kind, needle) in needles {
        for other in assets
            .iter()
            .filter(|a| a.kind == kind && a.key != asset.key)
        {
            if read(root, &other.key).is_some_and(|t| t.contains(&needle)) {
                out.push(other.key.clone());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// 썸네일 — egui 텍스처, 경로당 한 번
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct Thumbnails {
    textures: HashMap<String, Option<(egui::TextureHandle, [u32; 2])>>,
}

impl Thumbnails {
    fn get(&mut self, ctx: &egui::Context, path: &str) -> Option<(egui::TextureId, [u32; 2])> {
        let entry = self
            .textures
            .entry(path.to_owned())
            .or_insert_with(|| load_egui_image(ctx, path, "content"));
        entry.as_ref().map(|(h, size)| (h.id(), *size))
    }
}

/// PNG 를 egui 텍스처로 — 콘텐츠 브라우저와 시트 뷰어가 같이 쓴다. 못 읽으면 `None`.
pub(crate) fn load_egui_image(
    ctx: &egui::Context,
    path: &str,
    label: &str,
) -> Option<(egui::TextureHandle, [u32; 2])> {
    let bytes = std::fs::read(path).ok()?;
    let image = Image::decode_png(&bytes).ok()?;
    let (w, h) = (image.width(), image.height());
    let max = ctx.input(|i| i.max_texture_side);
    if w as usize > max || h as usize > max {
        return None;
    }
    let pixels =
        egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], image.desc(label).rgba);
    let handle = ctx.load_texture(
        format!("{label}:{path}"),
        pixels,
        egui::TextureOptions::NEAREST,
    );
    Some((handle, [w, h]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn the_repository_resources_are_found_by_kind() {
        let (assets, forms, error) = collect(&repo());
        assert_eq!(error, None);
        let has =
            |kind: AssetKind, key: &str| assets.iter().any(|a| a.kind == kind && a.key == key);
        assert!(has(AssetKind::Level, "levels/village.level.ron"));
        assert!(has(AssetKind::Zone, "zones/village.zone.ron"));
        assert!(has(AssetKind::Screen, "ui/main_menu.ui.ron"));
        assert!(has(AssetKind::Script, "data/scripts/goblin.rhai"));
        assert!(has(AssetKind::Data, "data/rules.ron"));
        assert!(has(AssetKind::Actor, "actor:102"));
        assert!(
            assets.iter().any(|a| a.kind == AssetKind::Sheet),
            "받아온 팩의 시트 정의"
        );
        assert!(
            !assets
                .iter()
                .any(|a| a.kind == AssetKind::Data && a.key.contains("scripts/")),
            "스크립트는 데이터가 아니다"
        );
        assert_eq!(forms.actors[&102].name, "고블린");
        // 경로는 `/` 구분이고 폴더부터 시작한다 — 두 OS 에서 같은 모양.
        for a in assets.iter().filter(|a| a.kind != AssetKind::Actor) {
            assert!(!a.key.contains('\\'), "{}", a.key);
            assert!(!a.key.starts_with("./"), "{}", a.key);
        }
    }

    #[test]
    fn level_and_screen_names_come_from_inside_the_file() {
        let (assets, _, _) = collect(&repo());
        let name = |key: &str| assets.iter().find(|a| a.key == key).unwrap().name.clone();
        assert_eq!(name("levels/village.level.ron"), "마을");
        assert_eq!(name("ui/main_menu.ui.ron"), "메인 화면");
    }

    #[test]
    fn double_click_goes_to_the_right_editor() {
        let (assets, _, _) = collect(&repo());
        let open = |key: &str| open_action(assets.iter().find(|a| a.key == key).unwrap());
        assert_eq!(
            open("zones/village.zone.ron"),
            ContentAction::OpenZone(PathBuf::from("zones/village.zone.ron"))
        );
        assert_eq!(
            open("levels/main.level.ron"),
            ContentAction::EditLevel(String::from("main"))
        );
        assert_eq!(
            open("ui/pause.ui.ron"),
            ContentAction::EditScreen(String::from("pause"))
        );
        assert_eq!(
            open("data/scripts/goblin.rhai"),
            ContentAction::EditScript(String::from("scripts/goblin.rhai"))
        );
        assert_eq!(open("actor:102"), ContentAction::EditActor(102));
    }

    #[test]
    fn references_find_who_points_at_a_resource() {
        let root = repo();
        let (assets, forms, _) = collect(&root);
        let refs = |key: &str| {
            let a = assets.iter().find(|a| a.key == key).unwrap();
            references(&root, a, &assets, &forms)
        };
        assert_eq!(
            refs("zones/village.zone.ron"),
            vec!["levels/village.level.ron"]
        );
        assert!(
            refs("levels/village.level.ron").contains(&String::from("ui/main_menu.ui.ron")),
            "메인 메뉴의 시작 버튼이 마을 레벨을 연다"
        );
        assert!(
            refs("levels/main.level.ron").contains(&String::from("data/project.ron")),
            "시작 레벨"
        );
        assert!(refs("ui/pause.ui.ron").contains(&String::from("levels/village.level.ron")));
        assert_eq!(refs("data/scripts/goblin.rhai"), vec!["액터 #102 고블린"]);
        assert!(
            refs("actor:102").contains(&String::from("zones/village.zone.ron")),
            "마을에 고블린 마커가 있다"
        );
    }

    #[test]
    fn png_size_is_read_from_the_header() {
        let root = repo();
        let size = image_size(&root, "assets/third_party/zelda-like-armm1998/gfx/font.png");
        assert!(size.is_some_and(|[w, h]| w > 0 && h > 0));
        assert_eq!(
            image_size(&root, "data/rules.ron"),
            None,
            "PNG 가 아니면 None"
        );
    }

    /// 저장소를 읽은 브라우저 — `refresh` 는 작업 디렉터리(`.`)를 읽으므로 직접 채운다.
    fn browser() -> ContentBrowser {
        let (assets, forms, error) = collect(&repo());
        assert!(error.is_none(), "{error:?}");
        ContentBrowser {
            folders: folders_of(&assets),
            assets,
            forms,
            loaded: true,
            ..ContentBrowser::default()
        }
    }

    fn names(entries: &[Entry]) -> Vec<String> {
        entries.iter().map(|e| entry_name(e).to_owned()).collect()
    }

    #[test]
    fn folders_include_ancestors_and_the_virtual_data_folder() {
        let b = browser();
        for f in [
            "assets",
            "assets/third_party",
            "assets/third_party/slime-garakh",
            "data",
            "data/scripts",
            "levels",
            "게임 데이터",
            "게임 데이터/액터",
            "게임 데이터/드롭 표",
        ] {
            assert!(b.folders.iter().any(|x| x == f), "{f} 가 없다");
        }
        assert!(
            !b.folders.iter().any(String::is_empty),
            "맨 위는 목록에 넣지 않는다"
        );
        assert_eq!(parent("assets/third_party"), "assets");
        assert_eq!(parent("assets"), "");
        assert!(is_under("assets/third_party", "assets"));
        assert!(
            !is_under("assets2", "assets"),
            "이름 앞부분만 같은 폴더는 아래가 아니다"
        );
    }

    #[test]
    fn a_folder_shows_its_subfolders_first_then_its_own_items() {
        let mut b = browser();
        let top = names(&b.entries());
        assert_eq!(top.last().map(String::as_str), Some("게임 데이터"));
        assert!(top.contains(&String::from("levels")));

        assert!(b.navigate("data"));
        let data = b.entries();
        assert!(matches!(&data[0], Entry::Folder(f) if f == "data/scripts"));
        assert!(
            data.iter()
                .any(|e| matches!(e, Entry::Asset(a) if a.key == "data/rules.ron"))
        );
        assert!(
            !data
                .iter()
                .any(|e| matches!(e, Entry::Asset(a) if a.kind == AssetKind::Script)),
            "하위 폴더의 항목은 그 폴더에서 보인다"
        );

        assert!(b.navigate("게임 데이터/액터"));
        let actors = b.entries();
        assert!(
            actors
                .iter()
                .all(|e| matches!(e, Entry::Asset(a) if a.kind == AssetKind::Actor))
        );
    }

    #[test]
    fn a_filter_searches_everything_below_the_current_folder() {
        let mut b = browser();
        b.kind = Some(AssetKind::Sheet);
        let sheets = b.entries();
        assert!(sheets.len() >= 3, "팩 폴더마다 흩어진 시트가 다 모인다");
        assert!(
            sheets
                .iter()
                .all(|e| matches!(e, Entry::Asset(a) if a.kind == AssetKind::Sheet))
        );

        b.kind = None;
        b.search = String::from("goblin");
        assert!(b.navigate("levels"));
        assert!(b.entries().is_empty(), "지금 폴더 밖은 찾지 않는다");
        assert!(b.navigate(""));
        assert!(
            b.entries()
                .iter()
                .any(|e| matches!(e, Entry::Asset(a) if a.key == "data/scripts/goblin.rhai"))
        );
    }

    #[test]
    fn navigation_keeps_a_back_history_and_refuses_unknown_folders() {
        let mut b = browser();
        assert!(!b.navigate("no/such/folder"));
        assert_eq!(b.folder, "");
        assert!(b.navigate("assets"));
        assert!(b.navigate("assets/sprites"));
        b.go_back();
        assert_eq!(b.folder, "assets");
        b.go_back();
        assert_eq!(b.folder, "");
        // 항목을 고르면 그 폴더로 간다.
        assert!(b.select_key("actor:102"));
        assert_eq!(b.folder, "게임 데이터/액터");
    }

    #[test]
    fn the_right_click_menu_suggests_what_belongs_in_the_folder() {
        let pick = |f: &str| suggestion(f).map(|(_, a)| a);
        assert_eq!(pick("levels"), Some(ContentAction::NewLevel));
        assert_eq!(pick("ui"), Some(ContentAction::NewScreen));
        assert_eq!(pick("data/scripts"), Some(ContentAction::NewScript));
        assert_eq!(pick("게임 데이터/액터"), Some(ContentAction::NewActor));
        assert_eq!(
            pick("게임 데이터/드롭 표"),
            Some(ContentAction::NewTable(Tab::Loot))
        );
        assert_eq!(pick(""), None, "맨 위는 고르지 않는다");
        assert_eq!(pick("data"), None);
        assert_eq!(pick("assets/sprites"), None);
        assert_eq!(pick("levels2"), None, "이름 앞부분만 같은 폴더");
        // 새로 만들기 목록은 가상 항목 종류를 모두 덮는다.
        let all: Vec<ContentAction> = create_items().into_iter().map(|(_, a)| a).collect();
        assert!(all.contains(&ContentAction::NewActor));
        assert!(all.contains(&ContentAction::NewTable(Tab::Skills)));
    }
}
