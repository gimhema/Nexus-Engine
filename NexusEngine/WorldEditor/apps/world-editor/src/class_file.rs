//! `rules.ron` 의 `stats`·`classes` 블록 — 파일 형식·검증·시뮬레이션 타입으로 옮기기 (R2).
//!
//! ```ron
//! stats: (
//!     base: ["str", "vit", "int", "agi"],
//!     derived: (
//!         max_hp: "vit * 20 + level * 20",
//!         attack: "str * 2 + equip_attack",
//!         defense: "agi + equip_defense",
//!         // max_mp · mp_regen 은 적지 않으면 0
//!     ),
//! ),
//! classes: {
//!     1: (base: {"str": 10, "vit": 9}, growth: {"str": 2, "vit": 1}, skills: [5, 6]),
//!     2: (base: {"int": 12}, formulas: (max_mp: "int * 12 + 10")),   // 이 직업만 다른 수식
//! },
//! ```
//!
//! `stats` 블록이 없으면 **스탯 체계가 없는 게임**이다 — 직업도 둘 수 없고 모든 유닛이 액터 수치를 쓴다.
//! 파생 수식은 이름이 정해진 다섯 칸이라 맵이 아니라 구조체 `( … )` 로 적는다(오타는 읽을 때 걸린다).
//!
//! ```text
//! ```
//!
//! 규칙(수식 문법·계산)은 `nexus_sim::stats` 에 있다 — 여기는 파일 모양과 검증뿐이다.
//! 직업 이름·설명과 스탯의 표시 이름은 `display.ron` 의 `classes`·`stats` (규칙/표시 분리).

use std::collections::BTreeMap;

use nexus_sim::{
    ClassDef, ClassId, DerivedStat, EquipRules, Formula, MAX_STATS, RESERVED, StatRules,
};
use serde::Deserialize;

/// 함수 이름 — 스탯 이름으로 쓰면 수식에서 함수와 구분할 수 없다.
const FUNCTIONS: [&str; 3] = ["min", "max", "clamp"];

/// 파생 스탯 중 **적어야 하는 것**. 나머지(MP·MP 회복)는 없으면 0 — MP 를 안 쓰는 게임도 있다.
const REQUIRED: [DerivedStat; 3] = [
    DerivedStat::MaxHp,
    DerivedStat::Attack,
    DerivedStat::Defense,
];

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StatsFile {
    /// 기본 스탯 이름 — 순서가 곧 화면에 보이는 순서다.
    #[serde(default)]
    base: Vec<String>,
    #[serde(default)]
    derived: DerivedFile,
}

/// 파생 스탯 수식 다섯 칸. **빈 문자열 = 적지 않음** (RON 은 `Option` 에 `Some(…)` 을 요구해 번거롭다).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DerivedFile {
    #[serde(default)]
    max_hp: String,
    #[serde(default)]
    max_mp: String,
    #[serde(default)]
    mp_regen: String,
    #[serde(default)]
    attack: String,
    #[serde(default)]
    defense: String,
}

impl DerivedFile {
    /// 적은 칸만 `(파생 스탯, 수식)`.
    fn entries(&self) -> impl Iterator<Item = (DerivedStat, &str)> {
        [
            (DerivedStat::MaxHp, &self.max_hp),
            (DerivedStat::MaxMp, &self.max_mp),
            (DerivedStat::MpRegen, &self.mp_regen),
            (DerivedStat::Attack, &self.attack),
            (DerivedStat::Defense, &self.defense),
        ]
        .into_iter()
        .filter(|(_, s)| !s.trim().is_empty())
        .map(|(d, s)| (d, s.as_str()))
    }

    fn get(&self, stat: DerivedStat) -> Option<&str> {
        self.entries().find(|(d, _)| *d == stat).map(|(_, s)| s)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClassFile {
    /// 레벨 1 의 기본 스탯. 적지 않은 스탯은 0.
    #[serde(default)]
    base: BTreeMap<String, i64>,
    /// 레벨당 성장. 적지 않은 스탯은 0.
    #[serde(default)]
    growth: BTreeMap<String, i64>,
    /// 단축키 스킬 (맨 앞이 1번 키). 비우면 액터 타입의 단축키를 쓴다.
    #[serde(default)]
    pub(crate) skills: Vec<u32>,
    /// 이 직업만 다른 파생 수식.
    #[serde(default)]
    formulas: DerivedFile,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClassLookFile {
    pub(crate) name: String,
    /// 직업 고르기 화면에 나오는 한 줄 설명.
    #[serde(default)]
    pub(crate) description: String,
}

impl StatsFile {
    pub(crate) fn base_has(&self, name: &str) -> bool {
        self.base.iter().any(|b| b == name)
    }

    /// 블록을 적지 않았다 — 스탯 체계가 없는 게임.
    pub(crate) fn is_absent(&self) -> bool {
        self.base.is_empty() && self.derived.entries().next().is_none()
    }
}

/// 장비 한 개의 스탯 보너스·직업 제한 — 검증용으로 빌려 온 것.
pub(crate) struct EquipRef<'a> {
    pub(crate) item: u32,
    pub(crate) stats: &'a BTreeMap<String, i64>,
    pub(crate) classes: &'a [u32],
}

/// 검증. 오류는 `stats: …` / `직업 N: …` / `아이템 N: …` 꼴로 모아 돌려준다.
pub(crate) fn validate(
    stats: Option<&StatsFile>,
    classes: &BTreeMap<u32, ClassFile>,
    equipment: &[EquipRef<'_>],
    skill_exists: impl Fn(u32) -> bool,
) -> Vec<String> {
    let mut errors = Vec::new();
    let Some(stats) = stats else {
        if !classes.is_empty() {
            errors.push(String::from(
                "classes 가 있으면 stats(기본 스탯과 수식)도 있어야 함",
            ));
        }
        for e in equipment {
            if !e.stats.is_empty() || !e.classes.is_empty() {
                errors.push(format!(
                    "아이템 {}: 스탯 보너스·직업 제한은 stats·classes 가 있어야 쓸 수 있음",
                    e.item
                ));
            }
        }
        return errors;
    };

    let mut err = |msg: String| errors.push(msg);
    if stats.base.len() > MAX_STATS {
        err(format!("stats: 기본 스탯은 {MAX_STATS}개까지"));
    }
    for (i, name) in stats.base.iter().enumerate() {
        let ident = name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !ident {
            err(format!(
                "stats: '{name}' — 스탯 이름은 영문 소문자·숫자·_ (첫 글자는 숫자 말고). 화면 이름은 display.ron 에"
            ));
        }
        if RESERVED.contains(&name.as_str()) || FUNCTIONS.contains(&name.as_str()) {
            err(format!("stats: '{name}' 은(는) 예약된 이름"));
        }
        if stats.base[..i].contains(name) {
            err(format!("stats: '{name}' 이(가) 두 번 있음"));
        }
    }
    let vars = StatRules::variables(&stats.base);
    let check_formulas = |owner: &str, f: &DerivedFile, err: &mut dyn FnMut(String)| {
        for (stat, src) in f.entries() {
            if let Err(e) = Formula::parse(src, &vars) {
                err(format!("{owner}: {} 수식 \"{src}\" — {e}", stat.key()));
            }
        }
    };
    check_formulas("stats.derived", &stats.derived, &mut err);
    for need in REQUIRED {
        if stats.derived.get(need).is_none() {
            err(format!("stats.derived: {} 수식이 없음", need.key()));
        }
    }

    let known = |name: &str| stats.base.iter().any(|b| b == name);
    for (id, c) in classes {
        let owner = format!("직업 {id}");
        if *id == 0 {
            err(String::from("직업 0: 0 은 '직업 없음' 예약 번호"));
        }
        for (what, map) in [("base", &c.base), ("growth", &c.growth)] {
            for name in map.keys().filter(|n| !known(n)) {
                err(format!(
                    "{owner}: {what} 의 '{name}' 은(는) stats.base 에 없는 스탯"
                ));
            }
        }
        for skill in c.skills.iter().filter(|s| !skill_exists(**s)) {
            err(format!("{owner}: 없는 스킬 {skill}"));
        }
        check_formulas(&owner, &c.formulas, &mut err);
    }
    for e in equipment {
        for name in e.stats.keys().filter(|n| !known(n)) {
            err(format!(
                "아이템 {}: stats 의 '{name}' 은(는) 없는 스탯",
                e.item
            ));
        }
        for class in e.classes.iter().filter(|c| !classes.contains_key(c)) {
            err(format!(
                "아이템 {}: classes 의 {class} 은(는) 없는 직업",
                e.item
            ));
        }
    }
    errors
}

/// 검증을 마친 블록을 시뮬레이션 타입으로. 스탯 체계가 없으면 `None`.
pub(crate) fn build(
    stats: Option<&StatsFile>,
    classes: &BTreeMap<u32, ClassFile>,
) -> Option<(StatRules, Vec<(ClassId, ClassDef)>)> {
    let stats = stats?;
    let vars = StatRules::variables(&stats.base);
    let parse = |src: &str| Formula::parse(src, &vars).unwrap_or_else(|_| Formula::constant(0));
    let formulas = DerivedStat::ALL.map(|d| {
        stats
            .derived
            .get(d)
            .map_or_else(|| Formula::constant(0), parse)
    });
    let per_stat = |map: &BTreeMap<String, i64>| -> Vec<i64> {
        stats
            .base
            .iter()
            .map(|n| map.get(n).copied().unwrap_or(0))
            .collect()
    };
    let classes = classes
        .iter()
        .map(|(id, c)| {
            (
                ClassId(*id),
                ClassDef {
                    base: per_stat(&c.base),
                    growth: per_stat(&c.growth),
                    formulas: c
                        .formulas
                        .entries()
                        .map(|(d, src)| (d, parse(src)))
                        .collect(),
                },
            )
        })
        .collect();
    Some((StatRules::new(stats.base.clone(), formulas), classes))
}

/// 장비 하나의 규칙. 스탯 이름을 번호로 바꾼다 (모르는 이름은 검증이 이미 막았다).
pub(crate) fn equip_rules(stats: Option<&StatsFile>, e: &EquipRef<'_>) -> EquipRules {
    let index = |name: &str| stats.and_then(|s| s.base.iter().position(|b| b == name));
    EquipRules {
        stats: e
            .stats
            .iter()
            .filter_map(|(n, v)| Some((index(n)?, *v)))
            .collect(),
        classes: e.classes.iter().map(|c| ClassId(*c)).collect(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 스탯 디자이너용 — 블록 전체를 폼으로 (R2)
// ─────────────────────────────────────────────────────────────────────────────

/// 기본 스탯 한 줄 — 규칙 이름과 화면 이름.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct StatRow {
    pub(crate) key: String,
    /// `display.ron` 의 이름. 비우면 규칙 이름 그대로 보인다.
    pub(crate) label: String,
}

/// 직업 하나 — 규칙(`rules.ron`)과 표시(`display.ron`) 반쪽을 함께.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ClassForm {
    pub(crate) id: u32,
    pub(crate) name: String,
    pub(crate) description: String,
    /// [`StatForm::stats`] 와 같은 순서.
    pub(crate) base: Vec<i64>,
    pub(crate) growth: Vec<i64>,
    pub(crate) skills: Vec<u32>,
    /// [`DerivedStat::ALL`] 순서. 빈 칸 = 공통 수식.
    pub(crate) formulas: [String; 5],
}

/// 스탯 체계 + 직업 전체 — 디자이너가 통째로 고치고 통째로 저장한다.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct StatForm {
    pub(crate) stats: Vec<StatRow>,
    /// [`DerivedStat::ALL`] 순서.
    pub(crate) derived: [String; 5],
    pub(crate) classes: Vec<ClassForm>,
}

impl DerivedFile {
    fn to_array(&self) -> [String; 5] {
        DerivedStat::ALL.map(|d| self.get(d).unwrap_or("").to_owned())
    }
}

impl StatForm {
    /// 두 파일의 블록에서 만든다 (나머지 내용은 보지 않는다).
    pub(crate) fn from_blocks(
        stats: &StatsFile,
        classes: &BTreeMap<u32, ClassFile>,
        class_looks: &BTreeMap<u32, ClassLookFile>,
        stat_labels: &BTreeMap<String, String>,
    ) -> Self {
        let per_stat = |m: &BTreeMap<String, i64>| -> Vec<i64> {
            stats
                .base
                .iter()
                .map(|k| m.get(k).copied().unwrap_or(0))
                .collect()
        };
        Self {
            stats: stats
                .base
                .iter()
                .map(|k| StatRow {
                    key: k.clone(),
                    label: stat_labels.get(k).cloned().unwrap_or_default(),
                })
                .collect(),
            derived: stats.derived.to_array(),
            classes: classes
                .iter()
                .map(|(id, c)| {
                    let look = class_looks.get(id);
                    ClassForm {
                        id: *id,
                        name: look.map(|l| l.name.clone()).unwrap_or_default(),
                        description: look.map(|l| l.description.clone()).unwrap_or_default(),
                        base: per_stat(&c.base),
                        growth: per_stat(&c.growth),
                        skills: c.skills.clone(),
                        formulas: c.formulas.to_array(),
                    }
                })
                .collect(),
        }
    }

    /// 기본 스탯 하나를 끝에 더한다 — 직업마다 0 칸이 생긴다.
    pub(crate) fn push_stat(&mut self, key: String) {
        self.stats.push(StatRow {
            key,
            label: String::new(),
        });
        for c in &mut self.classes {
            c.base.push(0);
            c.growth.push(0);
        }
    }

    /// `i` 번 기본 스탯을 뺀다 — 직업의 그 칸도 함께.
    pub(crate) fn remove_stat(&mut self, i: usize) {
        if i < self.stats.len() {
            self.stats.remove(i);
            for c in &mut self.classes {
                for v in [&mut c.base, &mut c.growth] {
                    if i < v.len() {
                        v.remove(i);
                    }
                }
            }
        }
    }

    /// 수식이 읽을 수 있는 이름.
    pub(crate) fn variables(&self) -> Vec<String> {
        let keys: Vec<String> = self.stats.iter().map(|s| s.key.clone()).collect();
        StatRules::variables(&keys)
    }

    /// 수식 하나를 검사한다 — 디자이너가 칸 아래에 바로 오류를 띄운다. 빈 칸은 `Ok`.
    pub(crate) fn check_formula(&self, src: &str) -> Result<(), String> {
        if src.trim().is_empty() {
            return Ok(());
        }
        Formula::parse(src, &self.variables())
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// `class` 번째 직업의 레벨별 스탯 — **게임과 같은 계산**(`StatRules::sheet`). 장비 없이.
    /// 수식에 오류가 있으면 그 이유.
    pub(crate) fn preview(
        &self,
        class: usize,
        levels: &[u32],
    ) -> Result<Vec<(u32, nexus_sim::StatSheet)>, String> {
        let vars = self.variables();
        let parse = |src: &str, what: &str| -> Result<Formula, String> {
            if src.trim().is_empty() {
                return Ok(Formula::constant(0));
            }
            Formula::parse(src, &vars).map_err(|e| format!("{what}: {e}"))
        };
        let mut formulas = Vec::with_capacity(5);
        for (d, src) in DerivedStat::ALL.into_iter().zip(&self.derived) {
            formulas.push(parse(src, d.key())?);
        }
        let formulas: [Formula; 5] = formulas
            .try_into()
            .map_err(|_| String::from("파생 수식 수가 맞지 않음"))?;
        let rules = StatRules::new(self.stats.iter().map(|s| s.key.clone()).collect(), formulas);
        let c = self.classes.get(class).ok_or("직업이 없음")?;
        let mut overrides = Vec::new();
        for (d, src) in DerivedStat::ALL.into_iter().zip(&c.formulas) {
            if !src.trim().is_empty() {
                overrides.push((d, parse(src, d.key())?));
            }
        }
        let def = ClassDef {
            base: c.base.clone(),
            growth: c.growth.clone(),
            formulas: overrides,
        };
        Ok(levels
            .iter()
            .map(|lv| {
                (
                    *lv,
                    rules.sheet(&def, *lv, &nexus_sim::EquipTotals::default()),
                )
            })
            .collect())
    }

    /// `rules.ron` 의 `stats`·`classes` 블록 줄.
    fn rules_blocks(&self) -> (Vec<String>, Vec<String>) {
        let q = crate::ron_patch::quote;
        let keys: Vec<String> = self.stats.iter().map(|s| q(&s.key)).collect();
        let mut stats = vec![
            format!("base: [{}],", keys.join(", ")),
            String::from("derived: ("),
        ];
        for (d, src) in DerivedStat::ALL.into_iter().zip(&self.derived) {
            if !src.trim().is_empty() {
                stats.push(format!("    {}: {},", d.key(), q(src.trim())));
            }
        }
        stats.push(String::from("),"));
        let map = |vals: &[i64]| -> String {
            let list: Vec<String> = self
                .stats
                .iter()
                .zip(vals)
                .filter(|(_, v)| **v != 0)
                .map(|(s, v)| format!("{}: {v}", q(&s.key)))
                .collect();
            format!("{{{}}}", list.join(", "))
        };
        let mut classes = Vec::new();
        for c in &self.classes {
            classes.push(format!("{}: (", c.id));
            classes.push(format!("    base: {},", map(&c.base)));
            classes.push(format!("    growth: {},", map(&c.growth)));
            if !c.skills.is_empty() {
                let list: Vec<String> = c.skills.iter().map(u32::to_string).collect();
                classes.push(format!("    skills: [{}],", list.join(", ")));
            }
            let over: Vec<String> = DerivedStat::ALL
                .into_iter()
                .zip(&c.formulas)
                .filter(|(_, src)| !src.trim().is_empty())
                .map(|(d, src)| format!("{}: {}", d.key(), q(src.trim())))
                .collect();
            if !over.is_empty() {
                classes.push(format!("    formulas: ({}),", over.join(", ")));
            }
            classes.push(String::from("),"));
        }
        (stats, classes)
    }

    /// `display.ron` 의 `classes`·`stats` 블록 줄. 빈 화면 이름은 적지 않는다.
    fn display_blocks(&self) -> (Vec<String>, Vec<String>) {
        let q = crate::ron_patch::quote;
        let classes = self
            .classes
            .iter()
            .filter(|c| !c.name.trim().is_empty())
            .map(|c| {
                let desc = if c.description.trim().is_empty() {
                    String::new()
                } else {
                    format!(", description: {}", q(c.description.trim()))
                };
                format!("{}: (name: {}{desc}),", c.id, q(c.name.trim()))
            })
            .collect();
        let stats = self
            .stats
            .iter()
            .filter(|s| !s.label.trim().is_empty())
            .map(|s| format!("{}: (name: {}),", q(&s.key), q(s.label.trim())))
            .collect();
        (classes, stats)
    }

    /// 두 파일 텍스트에 폼을 써 넣는다 — 네 블록(`stats`·`classes` / `classes`·`stats`)만 갈아 끼운다.
    /// ⚠ 그 블록 **안**의 주석은 남지 않는다. 검증은 부르는 쪽이 두 파일을 함께 한다.
    pub(crate) fn patch(&self, rules: &str, display: &str) -> Result<(String, String), String> {
        use crate::ron_patch::replace_block;
        let (stats, classes) = self.rules_blocks();
        let rules = replace_block(rules, "stats", '(', &stats)?;
        let rules = replace_block(&rules, "classes", '{', &classes)?;
        let (class_looks, stat_labels) = self.display_blocks();
        let display = replace_block(display, "classes", '{', &class_looks)?;
        let display = replace_block(&display, "stats", '{', &stat_labels)?;
        Ok((rules, display))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(text: &str) -> StatsFile {
        ron::from_str(text).unwrap()
    }

    fn classes(text: &str) -> BTreeMap<u32, ClassFile> {
        ron::from_str(text).unwrap()
    }

    const STATS: &str = r#"(base: ["str", "vit"], derived: (
        max_hp: "vit * 10", attack: "str * 2 + equip_attack", defense: "vit / 2" ))"#;

    #[test]
    fn the_module_example_builds() {
        let s = stats(STATS);
        let c = classes(
            r#"{ 1: (base: {"str": 5, "vit": 4}, growth: {"str": 1}, skills: [1]),
                 2: (formulas: (max_mp: "vit * 3")) }"#,
        );
        assert!(validate(Some(&s), &c, &[], |id| id == 1).is_empty());
        let (rules, classes) = build(Some(&s), &c).unwrap();
        assert_eq!(rules.stats(), ["str", "vit"]);
        assert_eq!(classes[0].1.base, [5, 4]);
        assert_eq!(classes[0].1.growth, [1, 0], "적지 않은 스탯은 0");
        assert_eq!(classes[1].1.formulas[0].0, DerivedStat::MaxMp);
        let sheet = rules.sheet(&classes[0].1, 3, &nexus_sim::EquipTotals::default());
        assert_eq!(sheet.derived.attack, 14, "str 5 + 1×2 = 7, × 2");
        assert_eq!(sheet.derived.max_mp, 0, "적지 않은 파생 스탯은 0");
    }

    #[test]
    fn mistakes_are_named() {
        let s = stats(
            r#"(base: ["str", "Str", "level", "str"], derived: (
                max_hp: "vit * 10", attack: "str *" ))"#,
        );
        let c =
            classes(r#"{ 0: (), 3: (base: {"dex": 1}, skills: [9], formulas: (defense: "x")) }"#);
        let stats_map = BTreeMap::from([(String::from("luk"), 1)]);
        let equip = [EquipRef {
            item: 1101,
            stats: &stats_map,
            classes: &[7],
        }];
        let e = validate(Some(&s), &c, &equip, |_| false);
        for want in [
            "stats: 'Str' — 스탯 이름은",
            "stats: 'level' 은(는) 예약된 이름",
            "stats: 'str' 이(가) 두 번",
            "stats.derived: max_hp 수식 \"vit * 10\" — 1번째 글자: 모르는 이름 'vit'",
            "stats.derived: attack 수식",
            "stats.derived: defense 수식이 없음",
            "직업 0: 0 은",
            "직업 3: base 의 'dex'",
            "직업 3: 없는 스킬 9",
            "직업 3: defense 수식",
            "아이템 1101: stats 의 'luk'",
            "아이템 1101: classes 의 7",
        ] {
            assert!(
                e.iter().any(|m| m.starts_with(want)),
                "{want} 없음 — {e:#?}"
            );
        }
    }

    // ── 디자이너 폼 ──────────────────────────────────────────────────────────

    fn shipped_form() -> StatForm {
        let (rules, display) = (
            crate::game_data::GameData::embedded_rules(),
            include_str!("../../../data/display.ron"),
        );
        crate::game_data::stat_form(rules, display).unwrap()
    }

    #[test]
    fn the_shipped_blocks_round_trip_through_the_form() {
        let rules = crate::game_data::GameData::embedded_rules();
        let display = include_str!("../../../data/display.ron");
        let form = shipped_form();
        assert_eq!(
            form.stats[0],
            StatRow {
                key: "str".into(),
                label: "힘".into()
            }
        );
        assert_eq!(form.classes.len(), 3);
        assert_eq!(
            form.classes[1].formulas[DerivedStat::MaxMp as usize],
            "int * 12 + 20"
        );
        let (r2, d2) = crate::game_data::patch_stat_form(rules, display, &form).unwrap();
        assert_eq!(
            crate::game_data::stat_form(&r2, &d2).unwrap(),
            form,
            "다시 읽으면 같은 폼"
        );
        // 블록 밖은 그대로 — 트리거·액터 주석이 남는다.
        assert!(r2.contains("// 플레이어가 슬라임을 쓰러뜨릴 때마다 센다."));
        assert!(r2.contains("// 서버 MonsterEntityData 기본값처럼 공격형."));
    }

    #[test]
    fn the_preview_uses_the_game_formulas() {
        let mut form = shipped_form();
        let rows = form.preview(0, &[1, 10]).unwrap();
        assert_eq!(rows[0].1.derived.max_hp, 200, "전사 = 예전 플레이어");
        assert_eq!(rows[1].1.derived.max_hp, 200 + 40 * 9);
        // 폼을 고치면 미리보기가 바로 따라간다 (저장 전).
        form.derived[DerivedStat::MaxHp as usize] = String::from("vit * 100");
        assert_eq!(form.preview(0, &[1]).unwrap()[0].1.derived.max_hp, 900);
        form.derived[DerivedStat::Attack as usize] = String::from("str *");
        assert!(form.preview(0, &[1]).unwrap_err().starts_with("attack:"));
        assert!(
            form.check_formula("str * luk")
                .unwrap_err()
                .contains("'luk'")
        );
        assert!(form.check_formula("  ").is_ok(), "빈 칸은 공통 수식");
    }

    #[test]
    fn adding_and_removing_a_stat_keeps_class_columns_aligned() {
        let mut form = shipped_form();
        form.push_stat(String::from("luk"));
        assert!(
            form.classes
                .iter()
                .all(|c| c.base.len() == 5 && c.growth.len() == 5)
        );
        form.classes[0].base[4] = 7;
        form.remove_stat(1); // vit
        assert_eq!(
            form.stats
                .iter()
                .map(|s| s.key.as_str())
                .collect::<Vec<_>>(),
            ["str", "int", "agi", "luk"]
        );
        assert_eq!(form.classes[0].base, [10, 5, 5, 7]);
        // vit 를 빼면 수식이 깨진다 — 저장 검사가 막는다.
        let err = crate::game_data::patch_stat_form(
            crate::game_data::GameData::embedded_rules(),
            include_str!("../../../data/display.ron"),
            &form,
        )
        .unwrap_err();
        assert!(err.contains("모르는 이름 'vit'"), "{err}");
    }

    #[test]
    fn a_typo_in_a_formula_name_is_a_read_error() {
        assert!(ron::from_str::<StatsFile>(r#"(base: [], derived: (powr: "1"))"#).is_err());
    }

    #[test]
    fn classes_and_equipment_rules_need_a_stat_system() {
        let c = classes("{ 1: () }");
        let stats_map = BTreeMap::new();
        let equip = [EquipRef {
            item: 5,
            stats: &stats_map,
            classes: &[1],
        }];
        let e = validate(None, &c, &equip, |_| true);
        assert_eq!(e.len(), 2, "{e:?}");
        assert!(
            validate(None, &BTreeMap::new(), &[], |_| true).is_empty(),
            "없어도 된다"
        );
    }
}
