//! 콘텐츠 브라우저 — 엔진이 다루는 리소스를 한곳에서 보고 여는 곳 (단계 2 P8).
//!
//! ```text
//! ┌ 콘텐츠 브라우저 ──────────────────────────────────────────────────────────┐
//! │ [검색……]  [새로 고침] [새로 만들기 ▼]                                        │
//! │ ▸ 전체 (52)     │ ● 마을           levels/village.level.ron  │ 레벨 · 마을     │
//! │ ▸ 레벨 (3)      │ ● 메인 화면      levels/main.level.ron     │ 존: zones/…     │
//! │ ▸ 맵 (2)        │ ● 샘플 존        levels/sample.level.ron   │ 쓰는 곳:        │
//! │ ▸ UI 화면 (4)   │                                           │  ui/main_menu…  │
//! │ ▸ 액터 (5) …    │                                           │ [열기]          │
//! └──────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! - **한 번 누르면 상세**(오른쪽), **두 번 누르면 그 종류의 편집기**가 열린다 (언리얼과 같다).
//! - 왼쪽은 폴더가 아니라 **종류별** 목록이다 — 같은 종류가 여러 폴더에 흩어져 있기 때문이다
//!   (스프라이트 시트는 받아온 팩 폴더마다 그림 옆에 있다).
//! - **액터는 파일이 아니다.** `rules.ron`(수치)·`display.ron`(이름·시트) 표 안의 항목을
//!   하나씩 가상 항목으로 보여 준다 — 서버 테이블과 짝을 맞춘 "반으로 나눈 표" 구조를
//!   건드리지 않기 위해서다 (사용자 결정 2026-09-24).
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
}

impl Asset {
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
    /// 왼쪽에서 고른 종류. `None` = 전체.
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
        self.assets = assets;
        self.forms = forms;
        self.error = error;
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
        self.kind = Some(asset.kind);
        self.selected = Some(asset.key.clone());
        self.details = Some(details(Path::new("."), &asset, &self.assets, &self.forms));
        true
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
            (OpKind::Duplicate, AssetKind::Actor) => {
                // 이 번호 다음의 빈 번호.
                let id = asset.actor_id().unwrap_or(100);
                let free = (id + 1..)
                    .find(|n| !self.forms.actors.contains_key(n))
                    .unwrap_or(id + 1);
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
                        ui.label(if dialog.asset.kind == AssetKind::Actor {
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
        egui::Panel::bottom("content_browser")
            .resizable(true)
            .default_size(250.0)
            .min_size(120.0)
            .show(ui, |ui| {
                // 패널 안에 패널 — 도구 줄(위) · 종류(왼쪽) · 상세(오른쪽) · 목록(가운데).
                // 가운데를 마지막에 두어야 남은 폭을 목록이 다 쓴다.
                egui::Panel::top("content_toolbar").show(ui, |ui| {
                    action = self.toolbar(ui);
                });
                egui::Panel::left("content_kinds")
                    .resizable(false)
                    .exact_size(170.0)
                    .show(ui, |ui| self.kinds(ui));
                egui::Panel::right("content_details")
                    .resizable(true)
                    .default_size(320.0)
                    .show(ui, |ui| {
                        if let Some(a) = self.details_panel(ui) {
                            action = Some(a);
                        }
                    });
                egui::CentralPanel::default().show(ui, |ui| {
                    if let Some(a) = self.list(ui) {
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
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("검색 (이름·경로)")
                    .desired_width(200.0),
            );
            if ui.button("새로 고침").clicked() {
                self.refresh();
            }
            ui.menu_button("새로 만들기", |ui| {
                for (label, a) in [
                    ("레벨…", ContentAction::NewLevel),
                    ("액터…", ContentAction::NewActor),
                    ("아이템…", ContentAction::NewTable(Tab::Items)),
                    ("스킬…", ContentAction::NewTable(Tab::Skills)),
                    ("드롭 표…", ContentAction::NewTable(Tab::Loot)),
                    ("UI 화면… (위젯 편집기)", ContentAction::NewScreen),
                    ("스크립트… (스크립트 편집기)", ContentAction::NewScript),
                ] {
                    if ui.button(label).clicked() {
                        action = Some(a);
                        ui.close();
                    }
                }
            });
            ui.weak("한 번 = 상세 · 두 번 = 편집기 · Ctrl+Space = 닫기");
        });
        if let Some(e) = &self.error {
            ui.colored_label(egui::Color32::from_rgb(240, 110, 100), e);
        }
        action
    }

    /// 왼쪽 — 종류별 목록과 개수.
    fn kinds(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("content_kinds")
            .show(ui, |ui| {
                let total = self.assets.len();
                if ui
                    .selectable_label(self.kind.is_none(), format!("전체 ({total})"))
                    .clicked()
                {
                    self.kind = None;
                }
                for kind in AssetKind::ALL {
                    let n = self.assets.iter().filter(|a| a.kind == kind).count();
                    let text = egui::RichText::new(format!("● {} ({n})", kind.label()));
                    let picked = self.kind == Some(kind);
                    let label = if picked {
                        text
                    } else {
                        text.color(kind.color())
                    };
                    if ui.selectable_label(picked, label).clicked() {
                        self.kind = Some(kind);
                    }
                }
            });
    }

    /// 가운데 — 항목 목록.
    fn list(&mut self, ui: &mut egui::Ui) -> Option<ContentAction> {
        let needle = self.search.trim().to_lowercase();
        let shown: Vec<Asset> = self
            .assets
            .iter()
            .filter(|a| self.kind.is_none_or(|k| a.kind == k))
            .filter(|a| {
                needle.is_empty()
                    || a.name.to_lowercase().contains(&needle)
                    || a.key.to_lowercase().contains(&needle)
            })
            .cloned()
            .collect();
        let mut action = None;
        egui::ScrollArea::vertical()
            .id_salt("content_list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if shown.is_empty() {
                    ui.weak("항목이 없습니다.");
                }
                for asset in &shown {
                    let picked = self.selected.as_deref() == Some(asset.key.as_str());
                    let row = ui.horizontal(|ui| {
                        // 그림은 작은 썸네일, 나머지는 종류 색 점.
                        let thumb = (asset.kind == AssetKind::Image)
                            .then(|| self.thumbs.get(ui.ctx(), &asset.key))
                            .flatten();
                        match thumb {
                            Some((tex, size)) => {
                                let s = fit(size, 20.0);
                                ui.image((tex, s));
                            }
                            None => {
                                ui.colored_label(asset.kind.color(), "●");
                            }
                        }
                        let r = ui.selectable_label(picked, &asset.name);
                        ui.weak(&asset.note);
                        r
                    });
                    let r = row.inner;
                    if r.clicked() {
                        self.selected = Some(asset.key.clone());
                        self.details =
                            Some(details(Path::new("."), asset, &self.assets, &self.forms));
                    }
                    if r.double_clicked() {
                        action = Some(open_action(asset));
                    }
                    // 오른쪽 클릭 — 파일 작업 (P9).
                    let _ = r.context_menu(|ui| {
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
                    r.on_hover_text(&asset.key);
                }
            });
        action
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
            "data/rules.ron" | "data/display.ron" => ContentAction::Notice(String::from(
                "이 표의 항목은 '액터' 목록에서 하나씩 엽니다 (아이템·스킬 편집기는 아직 없습니다)",
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
        out.push(file_asset(AssetKind::Level, path, name));
    }
    for path in scan_files(root, "zones", ".zone.ron") {
        let name = stem(&path, ".zone.ron");
        out.push(file_asset(AssetKind::Zone, path, name));
    }
    for path in scan_files(root, "ui", ".ui.ron") {
        let name = read(root, &path)
            .and_then(|t| crate::screen::Screens::read_screen(&t).ok())
            .map_or_else(|| stem(&path, ".ui.ron"), |s| s.name);
        out.push(file_asset(AssetKind::Screen, path, name));
    }
    for path in scan_files(root, "data/scripts", ".rhai") {
        let name = stem(&path, ".rhai");
        out.push(file_asset(AssetKind::Script, path, name));
    }
    for path in scan_files(root, "assets", ".sheet.ron") {
        let name = stem(&path, ".sheet.ron");
        out.push(file_asset(AssetKind::Sheet, path, name));
    }
    for path in scan_files(root, "assets", ".png") {
        let name = stem(&path, ".png");
        out.push(file_asset(AssetKind::Image, path, name));
    }
    // 데이터는 data/ 바로 아래의 .ron 만 (스크립트 폴더는 따로 보인다).
    for path in scan_files(root, "data", ".ron")
        .into_iter()
        .filter(|p| p.matches('/').count() == 1)
    {
        let name = stem(&path, ".ron");
        out.push(file_asset(AssetKind::Data, path, name));
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
        });
    }
    for (id, f) in &forms.tables.skills {
        out.push(Asset {
            kind: AssetKind::Skill,
            key: format!("skill:{id}"),
            name: named(&f.name, *id, "스킬"),
            note: format!("#{id}"),
        });
    }
    for (id, rows) in &forms.tables.loot {
        out.push(Asset {
            kind: AssetKind::Loot,
            key: format!("loot:{id}"),
            name: format!("드롭 표 #{id}"),
            note: format!("{}줄", rows.len()),
        });
    }
    (out, forms, error)
}

fn file_asset(kind: AssetKind, path: String, name: String) -> Asset {
    Asset {
        kind,
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
}
