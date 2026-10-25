//! `rules.ron` 의 스킬 항목 중 R3 에서 늘어난 부분 — 레벨 수식·부착 규칙·경험치 곡선.
//!
//! ```ron
//! skills: {
//!     // 숫자 칸은 예전 그대로 읽힌다.
//!     1: (range: 2.0, cooldown_ms: 800, damage_mult: 1.0),
//!     // 레벨이 있는 스킬 — 숫자 대신 수식(문자열)을 적을 수 있다. power 는 위력 천분율.
//!     9: (
//!         range: 6.0, max_level: 5, classes: [2],
//!         requires: Level(cmp: Ge, value: 3),
//!         power: "1500 + skill_level * 200 + int * 20",
//!         cooldown_ms: "3000 - skill_level * 200",
//!         mp_cost: "10 + skill_level * 3",
//!     ),
//! },
//! exp_curve: "level * level * 100",   // 적지 않으면 서버와 같은 이 식
//! ```
//!
//! 규칙(부착·레벨 계산)은 `nexus_sim::skills` 에 있다. 수식 문법은 스탯과 같고 변수에 `skill_level` 이 더해진다.

use nexus_sim::{ClassId, Formula, SkillRules, skill_variables};
use serde::Deserialize;

use crate::trigger_file::ConditionFile;

/// 스킬 최대 레벨 상한 — 손으로 쓰는 표에서 이보다 크면 실수다.
pub(crate) const MAX_SKILL_LEVEL: u32 = 100;

/// 숫자 또는 수식 — 예전 항목(숫자)이 그대로 읽히게.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub(crate) enum NumOr {
    Num(u32),
    Formula(String),
}

impl Default for NumOr {
    fn default() -> Self {
        Self::Num(0)
    }
}

impl NumOr {
    /// 숫자면 그 값, 수식이면 `if_formula`.
    pub(crate) fn base(&self, if_formula: u32) -> u32 {
        match self {
            Self::Num(n) => *n,
            Self::Formula(_) => if_formula,
        }
    }

    /// 숫자로 적혔으면 그 값 — 수식이면 레벨마다 달라 미리 알 수 없다.
    pub(crate) fn fixed(&self) -> Option<u32> {
        match self {
            Self::Num(n) => Some(*n),
            Self::Formula(_) => None,
        }
    }

    /// 0 으로 적혔다 (수식은 0 이 아니라고 본다 — 회복 수식이 있으면 회복 스킬이다).
    pub(crate) fn is_zero(&self) -> bool {
        *self == Self::Num(0)
    }

    pub(crate) fn formula(&self) -> Option<&str> {
        match self {
            Self::Formula(s) if !s.trim().is_empty() => Some(s),
            _ => None,
        }
    }
}

/// 스킬 항목의 R3 칸만 빌려 온 것 — 검증·변환 입력.
pub(crate) struct SkillLevelRef<'a> {
    pub(crate) id: u32,
    pub(crate) max_level: u32,
    pub(crate) classes: &'a [u32],
    pub(crate) requires: &'a ConditionFile,
    pub(crate) power: &'a str,
    pub(crate) cooldown_ms: &'a NumOr,
    pub(crate) mp_cost: &'a NumOr,
    pub(crate) heal: &'a NumOr,
}

impl SkillLevelRef<'_> {
    fn formulas(&self) -> [(&'static str, Option<&str>); 4] {
        [
            (
                "power",
                (!self.power.trim().is_empty()).then_some(self.power),
            ),
            ("cooldown_ms", self.cooldown_ms.formula()),
            ("mp_cost", self.mp_cost.formula()),
            ("heal", self.heal.formula()),
        ]
    }

    /// 특별한 것이 하나도 없다 — 부착 규칙을 만들 필요가 없다.
    fn is_plain(&self) -> bool {
        self.max_level <= 1
            && self.classes.is_empty()
            && *self.requires == ConditionFile::Always
            && self.formulas().iter().all(|(_, f)| f.is_none())
    }
}

/// 검증. `stats` 는 기본 스탯 이름(없으면 빈 목록), 나머지는 같은 파일의 표를 본다.
pub(crate) fn validate(
    skill: &SkillLevelRef<'_>,
    stats: &[String],
    class_exists: impl Fn(u32) -> bool,
    known: &crate::trigger_file::Known<'_>,
) -> Vec<String> {
    let id = skill.id;
    let mut errors = Vec::new();
    if !(1..=MAX_SKILL_LEVEL).contains(&skill.max_level) {
        errors.push(format!("스킬 {id}: max_level 은 1~{MAX_SKILL_LEVEL}"));
    }
    for c in skill.classes.iter().filter(|c| !class_exists(**c)) {
        errors.push(format!("스킬 {id}: classes 의 {c} 은(는) 없는 직업"));
    }
    let vars = skill_variables(stats);
    for (what, src) in skill.formulas() {
        if let Some(src) = src
            && let Err(e) = Formula::parse(src, &vars)
        {
            errors.push(format!("스킬 {id}: {what} 수식 \"{src}\" — {e}"));
        }
    }
    let mut err = |m: String| errors.push(format!("스킬 {id}: requires — {m}"));
    crate::trigger_file::check_condition(skill.requires, 0, known, &mut err);
    errors
}

/// 부착 규칙 — 특별한 것이 없으면 `None` (등록하지 않는다). 검증을 마친 뒤에 부른다.
pub(crate) fn rules(skill: &SkillLevelRef<'_>, stats: &[String]) -> Option<SkillRules> {
    if skill.is_plain() {
        return None;
    }
    let vars = skill_variables(stats);
    let parse = |src: Option<&str>| src.and_then(|s| Formula::parse(s, &vars).ok());
    let [power, cooldown, mp, heal] = skill.formulas().map(|(_, f)| parse(f));
    Some(SkillRules {
        max_level: skill.max_level.max(1),
        classes: skill.classes.iter().map(|c| ClassId(*c)).collect(),
        requires: skill.requires.into(),
        power_per_mille: power,
        cooldown_ms: cooldown,
        mp_cost: mp,
        heal,
    })
}

/// 경험치 곡선 수식 — 변수는 `level` 하나. 빈 문자열이면 `None`(기본 곡선).
pub(crate) fn exp_curve(src: &str) -> Result<Option<Formula>, String> {
    if src.trim().is_empty() {
        return Ok(None);
    }
    Formula::parse(src, &[String::from("level")])
        .map(Some)
        .map_err(|e| format!("exp_curve 수식 \"{src}\" — {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize)]
    struct Probe {
        a: NumOr,
        b: NumOr,
    }

    #[test]
    fn numbers_and_formulas_both_read() {
        let p: Probe = ron::from_str(r#"(a: 800, b: "3000 - skill_level * 200")"#).unwrap();
        assert_eq!(p.a, NumOr::Num(800));
        assert_eq!(p.b.formula(), Some("3000 - skill_level * 200"));
        assert_eq!((p.a.base(7), p.b.base(7)), (800, 7));
    }

    #[test]
    fn formula_errors_and_dangling_numbers_are_named() {
        let req = ConditionFile::SkillLevel {
            skill: 77,
            cmp: ron::from_str("Ge").unwrap(),
            value: 1,
        };
        let cd = NumOr::Formula(String::from("skill_level * luk"));
        let s = SkillLevelRef {
            id: 9,
            max_level: 0,
            classes: &[4],
            requires: &req,
            power: "1000 +",
            cooldown_ms: &cd,
            mp_cost: &NumOr::Num(3),
            heal: &NumOr::Num(0),
        };
        let e = validate(
            &s,
            &[String::from("int")],
            |_| false,
            &crate::trigger_file::Known {
                item: &|_| true,
                actor: &|_| true,
                skill: &|id| id == 9,
                quest: &|_| true,
                loot: &|_| true,
            },
        );
        for want in [
            "스킬 9: max_level 은",
            "스킬 9: classes 의 4",
            "스킬 9: power 수식",
            "스킬 9: cooldown_ms 수식",
            "스킬 9: requires — when: 없는 스킬 77",
        ] {
            assert!(
                e.iter().any(|m| m.starts_with(want)),
                "{want} 없음 — {e:#?}"
            );
        }
    }

    #[test]
    fn the_exp_curve_reads_level_only() {
        let f = exp_curve("level * 50 + 50").unwrap().unwrap();
        assert_eq!(f.eval(&[3]), 200);
        assert!(exp_curve("").unwrap().is_none());
        assert!(exp_curve("str * 2").unwrap_err().contains("'str'"));
    }
}
