//! 캐릭터 외형 (C3) — 종이 인형 카탈로그, 저장되는 외형, 커스터마이징 화면의 상태.
//!
//! # 어디서 오나
//!
//! 템플릿과 파츠는 `WorldCanvas/tools/paperdoll.py` 가 만든다. 공용 카탈로그(`catalog.ron`)가
//! 템플릿 목록과 색 묶음을, 템플릿 폴더의 `catalog.ron` 이 그 체형에 맞는 파츠(분류·색 묶음·장비 자리)를
//! 적는다. 이 모듈은 둘을 읽기만 한다.
//!
//! # 무엇을 저장하나
//!
//! [`Appearance`] 는 **템플릿 키 + 피부색 + (파츠 키, 색) 목록**이다. 색은 색 묶음의 번호가 아니라
//! 값으로 저장한다 — 생성기가 색 묶음을 고쳐도 저장한 캐릭터의 색이 바뀌지 않게.
//! 그리는 쪽은 [`Appearance::resolve`] 로 몸 시트 경로와 파츠([`PartRef`])를 얻는다. 장비 외형과 합치는
//! 규칙은 템플릿 기본 파츠와 같다(`sprites::compose`) — 상의·신발·모자는 그 자리에 외형 장비를 입으면 빠진다.
//!
//! # 종족 특징
//!
//! 귀·뿔·꼬리·엄니(`feature`)는 고르지 않는다 — 그 템플릿에 있으면 늘 붙는다. 색은 색 묶음을 따른다:
//! 머리색 묶음이면 머리색, 피부 묶음이면 피부색, 그 밖(뿔)이면 「특징 색」 줄에서 고른다.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nexus_assets::{AnimState, SpriteAnimator};
use nexus_sim::EquipSlot;
use serde::{Deserialize, Serialize};

use crate::sprites::PartRef;

/// 종이 인형 애셋 폴더 (작업 디렉터리 기준).
pub(crate) const CATALOG_DIR: &str = "assets/worldcanvas/paperdoll";

/// 커스터마이징 화면에서 고르는 분류 — `(분류, 화면 이름, 비울 수 있나)`. 화면의 줄 순서다.
pub(crate) const CATEGORIES: [(&str, &str, bool); 10] = [
    ("eyes", "눈", false),
    ("face", "입", false),
    ("hair", "머리", true),
    ("hair_back", "뒷머리", true),
    ("facial", "얼굴 꾸미기", true),
    ("top", "상의", true),
    ("bottom", "하의", true),
    ("shoes", "신발", true),
    ("hat", "모자", true),
    ("cape", "망토", true),
];

/// 화면 버튼 `Look(part: "…")` 이 가리킬 수 있는 이름 — 읽을 때 검사한다.
pub(crate) fn look_target_ok(name: &str) -> bool {
    matches!(name, "template" | "skin" | "random" | "feature_color")
        || CATEGORIES
            .iter()
            .any(|(c, ..)| name == *c || name.strip_suffix("_color").is_some_and(|base| base == *c))
}

// ─────────────────────────────────────────────────────────────────────────────
// 카탈로그
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogFile {
    version: u32,
    templates: Vec<String>,
    palettes: BTreeMap<String, PaletteFile>,
}

/// 색 묶음 하나 — `(이름, sRGB)`.
type PaletteFile = Vec<(String, (f32, f32, f32))>;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplateFile {
    version: u32,
    key: String,
    name: String,
    reference: String,
    body: String,
    parts: Vec<PartEntryFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PartEntryFile {
    key: String,
    name: String,
    category: String,
    palette: String,
    /// 색을 곱하는 파츠인가 — 입·주근깨·엄니는 그림 색 그대로다.
    tinted: bool,
    slot: Option<SlotName>,
}

/// 파츠가 빠지는 장비 자리 — 생성기는 옷·신발·모자에만 적는다.
#[derive(Clone, Copy, Debug, Deserialize)]
enum SlotName {
    Weapon,
    Head,
    Body,
    Hand,
    Shoes,
}

impl From<SlotName> for EquipSlot {
    fn from(s: SlotName) -> Self {
        match s {
            SlotName::Weapon => Self::Weapon,
            SlotName::Head => Self::Head,
            SlotName::Body => Self::Body,
            SlotName::Hand => Self::Hand,
            SlotName::Shoes => Self::Shoes,
        }
    }
}

/// 파츠 하나 — 카탈로그의 한 줄.
#[derive(Clone, Debug)]
pub(crate) struct PartEntry {
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) category: String,
    pub(crate) palette: String,
    pub(crate) tinted: bool,
    pub(crate) slot: Option<EquipSlot>,
}

/// 템플릿 하나 — 체형과 그 체형에 맞는 파츠.
#[derive(Clone, Debug)]
pub(crate) struct TemplateEntry {
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) reference: String,
    /// 시트 정의가 있는 폴더 (작업 디렉터리 기준, `/` 구분).
    dir: String,
    body: String,
    pub(crate) parts: Vec<PartEntry>,
}

impl TemplateEntry {
    fn sheet(&self, file: &str) -> String {
        format!("{}/{file}", self.dir)
    }

    fn of(&self, category: &str) -> impl Iterator<Item = &PartEntry> {
        self.parts.iter().filter(move |p| p.category == category)
    }

    fn part(&self, key: &str) -> Option<&PartEntry> {
        self.parts.iter().find(|p| p.key == key)
    }
}

/// 읽은 카탈로그.
#[derive(Clone, Debug)]
pub(crate) struct Catalog {
    pub(crate) templates: Vec<TemplateEntry>,
    /// 색 묶음 — `(이름, sRGB)`.
    palettes: BTreeMap<String, Vec<(String, [f32; 4])>>,
}

impl Catalog {
    /// `root/CATALOG_DIR/catalog.ron` 과 템플릿 카탈로그들을 읽는다.
    ///
    /// # Errors
    /// 공용 카탈로그가 없거나 형식이 틀리면 오류 — 생성기를 돌리지 않은 작업 폴더다.
    /// 템플릿 하나가 깨졌으면 그 템플릿만 빼지 않고 **전체를 거절한다** — 고친 것이 조용히 빠지지 않게.
    pub(crate) fn load(root: &Path) -> Result<Self, String> {
        let dir = root.join(CATALOG_DIR);
        let read =
            |p: &PathBuf| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
        let path = dir.join("catalog.ron");
        let file: CatalogFile =
            ron::from_str(&read(&path)?).map_err(|e| format!("{}: {e}", path.display()))?;
        if file.version != 1 {
            return Err(format!(
                "{}: 형식 {} 은(는) 읽을 수 없음",
                path.display(),
                file.version
            ));
        }
        let mut templates = Vec::new();
        for rel in &file.templates {
            let path = dir.join(rel);
            let t: TemplateFile =
                ron::from_str(&read(&path)?).map_err(|e| format!("{}: {e}", path.display()))?;
            if t.version != 1 {
                return Err(format!(
                    "{}: 형식 {} 은(는) 읽을 수 없음",
                    path.display(),
                    t.version
                ));
            }
            let folder = rel.rsplit_once('/').map_or("", |(d, _)| d);
            templates.push(TemplateEntry {
                key: t.key,
                name: t.name,
                reference: t.reference,
                dir: format!("{CATALOG_DIR}/{folder}"),
                body: t.body,
                parts: t
                    .parts
                    .into_iter()
                    .map(|p| PartEntry {
                        key: p.key,
                        name: p.name,
                        category: p.category,
                        palette: p.palette,
                        tinted: p.tinted,
                        slot: p.slot.map(EquipSlot::from),
                    })
                    .collect(),
            });
        }
        if templates.is_empty() {
            return Err(format!("{}: 템플릿이 없음", path.display()));
        }
        let palettes = file
            .palettes
            .into_iter()
            .map(|(k, v)| {
                let colours = v
                    .into_iter()
                    .map(|(name, (r, g, b))| (name, [r, g, b, 1.0]))
                    .collect();
                (k, colours)
            })
            .collect();
        Ok(Self {
            templates,
            palettes,
        })
    }

    fn template(&self, key: &str) -> Option<&TemplateEntry> {
        self.templates.iter().find(|t| t.key == key)
    }

    fn palette(&self, name: &str) -> &[(String, [f32; 4])] {
        self.palettes.get(name).map_or(&[][..], Vec::as_slice)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 저장되는 외형
// ─────────────────────────────────────────────────────────────────────────────

/// 캐릭터 하나의 외형 — 저장 파일의 `appearance`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Appearance {
    /// 템플릿 키 (`sd_elf`).
    pub(crate) template: String,
    /// 몸에 곱할 색 (sRGB).
    pub(crate) skin: (f32, f32, f32),
    /// 겹칠 파츠 `(파츠 키, sRGB)` — 종족 특징까지 들어 있다. 순서는 의미가 없다(앞뒤는 시트가 정한다).
    pub(crate) parts: Vec<(String, (f32, f32, f32))>,
}

/// 그릴 준비가 된 외형 — 시트 경로와 색.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ResolvedLook {
    pub(crate) body: String,
    pub(crate) skin: [f32; 4],
    pub(crate) parts: Vec<PartRef>,
}

impl ResolvedLook {
    /// 올려야 할 시트 경로 전부 — 몸과 파츠.
    pub(crate) fn sheet_paths(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.body.as_str()).chain(self.parts.iter().map(|p| p.sheet.as_str()))
    }
}

fn rgba((r, g, b): (f32, f32, f32)) -> [f32; 4] {
    [r, g, b, 1.0]
}

fn rgb(c: [f32; 4]) -> (f32, f32, f32) {
    (c[0], c[1], c[2])
}

impl Appearance {
    /// 카탈로그로 시트 경로를 푼다. 템플릿이 없으면 `None`(기본 액터 그림으로 그린다),
    /// 카탈로그에 없는 파츠는 건너뛴다 — 생성기에서 파츠를 지워도 이어 하기가 막히지 않게.
    pub(crate) fn resolve(&self, catalog: &Catalog) -> Option<ResolvedLook> {
        let t = catalog.template(&self.template)?;
        let parts = self
            .parts
            .iter()
            .filter_map(|(key, colour)| {
                let p = t.part(key)?;
                Some(PartRef {
                    sheet: t.sheet(&format!("{key}.sheet.ron")),
                    // 컬러 그림에 색을 곱하면 물든다 — 색 그대로인 파츠는 곱하지 않는다.
                    tint: if p.tinted {
                        rgba(*colour)
                    } else {
                        crate::sprites::NO_TINT
                    },
                    slot: p.slot,
                })
            })
            .collect();
        Some(ResolvedLook {
            body: t.sheet(&t.body),
            skin: rgba(self.skin),
            parts,
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 커스터마이징 화면의 상태
// ─────────────────────────────────────────────────────────────────────────────

/// 고르는 중인 외형. 화면 버튼(`Look`)이 한 칸씩 넘기고, 시작하면 [`Appearance`] 가 된다.
#[derive(Debug)]
pub(crate) struct Creator {
    catalog: Catalog,
    template: usize,
    skin: usize,
    /// 분류 → 고른 파츠 키 (`None` = 없음). 키로 들고 있어야 템플릿을 바꿔도 같은 머리 모양이 남는다.
    pick: BTreeMap<&'static str, Option<String>>,
    /// 분류 → 색 묶음 안의 번호. `feature` 는 종족 특징(뿔) 색.
    colour: BTreeMap<&'static str, usize>,
    /// 미리보기 재생기 — 제자리 걷기.
    pub(crate) animator: SpriteAnimator,
    /// 무작위 고르기 상태 (SplitMix64).
    seed: u64,
}

/// 처음 고른 차림 — 무엇이든 보이도록 흔한 것으로.
const DEFAULTS: [(&str, &str); 6] = [
    ("eyes", "eyes_round"),
    ("face", "face_smile"),
    ("hair", "hair_short"),
    ("top", "top_tunic"),
    ("bottom", "bottom_pants"),
    ("shoes", "shoes_boots"),
];

/// 처음 고른 색 — 색 묶음의 이름. 없는 이름이면 첫 색.
const DEFAULT_COLOURS: [(&str, &str); 6] = [
    ("eyes", "갈색"),
    ("hair", "갈색"),
    ("top", "파랑"),
    ("bottom", "갈색"),
    ("shoes", "검정"),
    ("cape", "빨강"),
];

impl Creator {
    pub(crate) fn new(catalog: Catalog, seed: u64) -> Self {
        let mut pick = BTreeMap::new();
        for (cat, ..) in CATEGORIES {
            let default = DEFAULTS
                .iter()
                .find(|(c, _)| *c == cat)
                .map(|(_, k)| (*k).to_owned());
            pick.insert(cat, default);
        }
        // 처음 색 — 모험가 차림(갈색 머리·파란 튜닉·갈색 바지·짙은 부츠). 색 묶음의 번호다.
        let mut colour: BTreeMap<&'static str, usize> = BTreeMap::new();
        for (cat, ..) in CATEGORIES {
            colour.insert(cat, 0);
        }
        for (cat, name) in DEFAULT_COLOURS {
            if let Some(i) = catalog
                .templates
                .first()
                .and_then(|t| t.of(cat).next())
                .and_then(|p| {
                    catalog
                        .palette(&p.palette)
                        .iter()
                        .position(|(n, _)| n == name)
                })
            {
                colour.insert(cat, i);
            }
        }
        colour.insert("feature", 0);
        let mut animator = SpriteAnimator::default();
        animator.set_state(AnimState::Walk);
        Self {
            catalog,
            template: 0,
            skin: 1,
            pick,
            colour,
            animator,
            seed,
        }
    }

    fn template(&self) -> &TemplateEntry {
        &self.catalog.templates[self.template]
    }

    /// 버튼 하나 — `what` 을 `step` 칸 넘긴다 ([`look_target_ok`] 의 이름).
    pub(crate) fn step(&mut self, what: &str, step: i32) {
        match what {
            "template" => {
                self.template = wrap(self.template, step, self.catalog.templates.len());
            }
            "skin" => {
                let n = self.catalog.palette("skin").len();
                self.skin = wrap(self.skin, step, n);
            }
            "random" => self.randomize(),
            "feature_color" => {
                let n = self.catalog.palette("horn").len();
                let c = self.colour.entry("feature").or_default();
                *c = wrap(*c, step, n);
            }
            _ => {
                if let Some(cat) = what.strip_suffix("_color").and_then(category) {
                    let n = self.palette_of(cat).len();
                    let c = self.colour.entry(cat).or_default();
                    *c = wrap(*c, step, n);
                } else if let Some(cat) = category(what) {
                    self.step_part(cat, step);
                }
            }
        }
    }

    /// 분류의 파츠를 넘긴다 — 비울 수 있는 분류는 "없음" 이 한 칸이다.
    fn step_part(&mut self, cat: &'static str, step: i32) {
        let optional = CATEGORIES.iter().any(|(c, _, o)| *c == cat && *o);
        let keys: Vec<Option<String>> = optional
            .then_some(None)
            .into_iter()
            .chain(self.template().of(cat).map(|p| Some(p.key.clone())))
            .collect();
        if keys.is_empty() {
            return;
        }
        let now = self.pick.get(cat).cloned().flatten();
        let at = keys.iter().position(|k| *k == now).unwrap_or(0);
        self.pick
            .insert(cat, keys[wrap(at, step, keys.len())].clone());
    }

    /// 그 분류가 쓰는 색 묶음 — 템플릿 카탈로그의 첫 파츠를 따른다.
    fn palette_of(&self, cat: &str) -> &[(String, [f32; 4])] {
        let name = self
            .template()
            .of(cat)
            .next()
            .map_or("cloth", |p| p.palette.as_str());
        self.catalog.palette(name)
    }

    fn colour_of(&self, cat: &str) -> Option<&(String, [f32; 4])> {
        // 뒷머리는 머리색을 따른다 — 따로 고르면 앞뒤가 어긋나기 쉽다.
        let cat = if cat == "hair_back" { "hair" } else { cat };
        let pal = self.palette_of(cat);
        let i = self.colour.get(cat).copied().unwrap_or(0);
        pal.get(i % pal.len().max(1))
    }

    fn next_random(&mut self, n: usize) -> usize {
        // SplitMix64 — 시뮬레이션과 같은 생성기. 결정적일 필요는 없지만 의존성을 늘리지 않는다.
        self.seed = self.seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.seed;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        if n == 0 { 0 } else { (z % n as u64) as usize }
    }

    /// 템플릿은 그대로 두고 피부·파츠·색을 무작위로.
    fn randomize(&mut self) {
        let skins = self.catalog.palette("skin").len();
        self.skin = self.next_random(skins);
        for (cat, ..) in CATEGORIES {
            let n = self.template().of(cat).count() + 1;
            let roll = self.next_random(n);
            // 눈·입은 늘 있고, 상의·하의는 대체로 입힌다 — 맨몸이 너무 자주 나오지 않게.
            let pick = match (cat, roll) {
                ("eyes" | "face", r) => r.max(1),
                ("top" | "bottom" | "shoes", 0) => 1,
                (_, r) => r,
            };
            let key = pick
                .checked_sub(1)
                .and_then(|i| self.template().of(cat).nth(i).map(|p| p.key.clone()));
            self.pick.insert(cat, key);
            let colours = self.palette_of(cat).len();
            let c = self.next_random(colours);
            self.colour.insert(cat, c);
        }
        let horns = self.catalog.palette("horn").len();
        let c = self.next_random(horns);
        self.colour.insert("feature", c);
        // 뒷머리는 앞머리와 짝일 때 자연스럽다 — 긴 머리·포니테일·트윈테일이면 맞춰 준다.
        if let Some(Some(hair)) = self.pick.get("hair").cloned() {
            let back = hair.replace("hair_", "hairback_");
            if self.template().part(&back).is_some() {
                self.pick.insert("hair_back", Some(back));
            }
        }
    }

    /// 지금 고른 외형.
    pub(crate) fn appearance(&self) -> Appearance {
        let t = self.template();
        let skin = self
            .catalog
            .palette("skin")
            .get(self.skin)
            .map_or([1.0; 4], |(_, c)| *c);
        let mut parts = Vec::new();
        for (cat, ..) in CATEGORIES {
            let Some(Some(key)) = self.pick.get(cat) else {
                continue;
            };
            if t.part(key).is_none() {
                continue;
            }
            let colour = self.colour_of(cat).map_or([1.0; 4], |(_, c)| *c);
            parts.push((key.clone(), rgb(colour)));
        }
        // 종족 특징 — 늘 붙는다. 색은 색 묶음을 따른다.
        let hair = self.colour_of("hair").map_or([1.0; 4], |(_, c)| *c);
        let horn = self
            .catalog
            .palette("horn")
            .get(self.colour.get("feature").copied().unwrap_or(0))
            .map_or([1.0; 4], |(_, c)| *c);
        for p in t.of("feature") {
            let colour = match p.palette.as_str() {
                "hair" => hair,
                "skin" => skin,
                _ => horn,
            };
            parts.push((p.key.clone(), rgb(colour)));
        }
        Appearance {
            template: t.key.clone(),
            skin: rgb(skin),
            parts,
        }
    }

    pub(crate) fn resolved(&self) -> Option<ResolvedLook> {
        self.appearance().resolve(&self.catalog)
    }

    /// 화면 문구 `{look_*}` 의 값 — `(이름, 글)`.
    pub(crate) fn texts(&self) -> Vec<(String, String)> {
        let t = self.template();
        let mut out = vec![
            (String::from("look_template"), t.name.clone()),
            (String::from("look_reference"), t.reference.clone()),
            (
                String::from("look_skin"),
                self.catalog
                    .palette("skin")
                    .get(self.skin)
                    .map_or_else(String::new, |(n, _)| n.clone()),
            ),
        ];
        for (cat, ..) in CATEGORIES {
            let name = match self.pick.get(cat) {
                Some(Some(key)) => t
                    .part(key)
                    .map_or_else(|| String::from("—"), |p| short(&p.name)),
                _ => String::from("없음"),
            };
            out.push((format!("look_{cat}"), name));
            let colour = self
                .colour_of(cat)
                .map_or_else(String::new, |(n, _)| n.clone());
            out.push((format!("look_{cat}_color"), colour));
        }
        let horn = self
            .catalog
            .palette("horn")
            .get(self.colour.get("feature").copied().unwrap_or(0))
            .map_or_else(String::new, |(n, _)| n.clone());
        let has_feature = t.of("feature").next().is_some();
        out.push((
            String::from("look_feature"),
            if has_feature {
                t.of("feature")
                    .map(|p| p.name.clone())
                    .collect::<Vec<_>>()
                    .join(" · ")
            } else {
                String::from("없음")
            },
        ));
        out.push((String::from("look_feature_color"), horn));
        out
    }
}

/// `눈 — 동그란` → `동그란`. 화면 칸이 좁다.
fn short(name: &str) -> String {
    name.split_once(" — ").map_or(name, |(_, s)| s).to_owned()
}

fn category(name: &str) -> Option<&'static str> {
    CATEGORIES.iter().map(|(c, ..)| *c).find(|c| *c == name)
}

fn wrap(at: usize, step: i32, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let n = n as i64;
    ((at as i64 + i64::from(step)).rem_euclid(n)) as usize
}

/// 화면 문구 이름 전부 — `screen::PLACEHOLDERS` 와 맞춘다.
pub(crate) fn placeholder_names() -> Vec<String> {
    let mut out = vec![
        String::from("look_template"),
        String::from("look_reference"),
        String::from("look_skin"),
        String::from("look_feature"),
        String::from("look_feature_color"),
    ];
    for (cat, ..) in CATEGORIES {
        out.push(format!("look_{cat}"));
        out.push(format!("look_{cat}_color"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Option<Catalog> {
        Catalog::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).ok()
    }

    #[test]
    fn stepping_wraps_and_optional_categories_have_a_none() {
        assert_eq!(wrap(0, -1, 3), 2);
        assert_eq!(wrap(2, 1, 3), 0);
        // 카탈로그는 생성기 결과(추적하지 않는 애셋)다 — 없으면 규칙만 본다.
        let Some(cat) = catalog() else { return };
        let mut c = Creator::new(cat, 1);
        assert_eq!(c.appearance().template, "sd_human");
        // 모자는 처음에 없다 → 한 칸 넘기면 첫 모자 → 거꾸로 두 칸이면 다시 없음 앞의 마지막 모자.
        c.step("hat", 1);
        assert!(
            c.appearance()
                .parts
                .iter()
                .any(|(k, _)| k.starts_with("hat_"))
        );
        c.step("hat", -1);
        assert!(
            !c.appearance()
                .parts
                .iter()
                .any(|(k, _)| k.starts_with("hat_"))
        );
        // 눈은 비울 수 없다.
        for _ in 0..12 {
            c.step("eyes", 1);
            assert!(
                c.appearance()
                    .parts
                    .iter()
                    .any(|(k, _)| k.starts_with("eyes_"))
            );
        }
    }

    #[test]
    fn templates_keep_the_same_picks_and_add_their_features() {
        let Some(cat) = catalog() else { return };
        let mut c = Creator::new(cat.clone(), 1);
        c.step("hair", 3);
        let hair = c.appearance().parts[2].0.clone();
        // 수인 템플릿 — 같은 머리 + 고양이 귀·꼬리가 저절로 붙는다.
        while c.appearance().template != "sd_beast" {
            c.step("template", 1);
        }
        let look = c.appearance();
        assert!(
            look.parts.iter().any(|(k, _)| *k == hair),
            "머리 모양이 남는다"
        );
        assert!(look.parts.iter().any(|(k, _)| k == "ears_cat"));
        assert!(look.parts.iter().any(|(k, _)| k == "tail_cat"));

        // 저장 모양을 지나도 같다. 풀면 모든 파츠가 그 템플릿 폴더의 시트다.
        let text = ron::to_string(&look).unwrap();
        let back: Appearance = ron::from_str(&text).unwrap();
        assert_eq!(back, look);
        let resolved = back.resolve(&cat).unwrap();
        assert!(resolved.body.ends_with("sd_beast/body.sheet.ron"));
        assert_eq!(resolved.parts.len(), look.parts.len());
        assert!(
            resolved
                .parts
                .iter()
                .all(|p| p.sheet.contains("/sd_beast/"))
        );
        // 상의는 몸 자리 — 갑옷을 입으면 빠진다.
        let top = resolved
            .parts
            .iter()
            .find(|p| p.sheet.contains("top_"))
            .unwrap();
        assert_eq!(top.slot, Some(EquipSlot::Body));
    }

    #[test]
    fn random_looks_always_have_a_face_and_clothes() {
        let Some(cat) = catalog() else { return };
        let mut c = Creator::new(cat, 7);
        for _ in 0..40 {
            c.step("random", 0);
            let look = c.appearance();
            let has = |p: &str| look.parts.iter().any(|(k, _)| k.starts_with(p));
            assert!(
                has("eyes_") && has("face_") && has("top_") && has("bottom_"),
                "{look:?}"
            );
        }
    }

    #[test]
    fn look_targets_cover_every_row() {
        for (cat, ..) in CATEGORIES {
            assert!(look_target_ok(cat));
            assert!(look_target_ok(&format!("{cat}_color")));
        }
        assert!(look_target_ok("template") && look_target_ok("random"));
        assert!(!look_target_ok("hat_colour") && !look_target_ok("weapon"));
        assert_eq!(placeholder_names().len(), 5 + CATEGORIES.len() * 2);
    }
}
