//! `rules.ron` 의 `triggers` 블록 — 파일 형식·검증·시뮬레이션 타입으로 옮기기 (R1).
//!
//! ```ron
//! triggers: {
//!     // 슬라임 처치 수를 센다
//!     1: (
//!         on: Killed(actor: Some(100)),
//!         when: Actor(1),
//!         then: [AddFlag(scope: Character, flag: 1, delta: 1)],
//!     ),
//!     // 다섯 마리째에 한 번만 보상
//!     2: (
//!         on: Killed(actor: Some(100)),
//!         when: All([
//!             Flag(scope: Character, flag: 1, cmp: Ge, value: 5),
//!             Not(Flag(scope: Character, flag: 2, cmp: Eq, value: 1)),
//!         ]),
//!         then: [GiveItem(item: 501, count: 3), SetFlag(scope: Character, flag: 2, value: 1)],
//!     ),
//! }
//! ```
//!
//! 규칙(언제·만약·한다)은 `nexus_sim::trigger` 에 있다 — 여기는 그 **파일 모양**뿐이다.
//! 다른 표와 같이 파일 타입을 따로 두는 이유는 `zone_file` 과 같다(시뮬레이션이 직렬화를 모르게).
//!
//! 트리거 이름·설명은 아직 없다 — 번호 위의 주석이 그 역할이다. 트리거 편집기가 생기면
//! 표시 이름을 `display.ron` 으로 뺀다(규칙/표시 분리).

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use nexus_core::Vec2;
use nexus_sim::{
    ActorType, Cmp, Condition, Effect, FactionId, FlagId, FlagScope, ItemId, On, SkillId,
    TriggerDef, TriggerId,
};
use serde::Deserialize;

/// 트리거 하나. `when` 을 적지 않으면 늘 참.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TriggerFile {
    on: OnFile,
    #[serde(default = "always")]
    when: ConditionFile,
    then: Vec<EffectFile>,
}

pub(crate) fn always() -> ConditionFile {
    ConditionFile::Always
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
enum OnFile {
    Killed {
        #[serde(default)]
        actor: Option<u32>,
    },
    LevelReached(u32),
    PickedUp {
        #[serde(default)]
        item: Option<u32>,
    },
    Entered {
        center: (f32, f32),
        radius: f32,
    },
}

/// 조건 — 트리거의 `when` 과 스킬의 `requires` 가 같은 형식을 쓴다 (R3).
/// 표 편집기가 다시 써내야 해서 `Serialize` 도 붙어 있다.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum ConditionFile {
    Always,
    Flag {
        scope: ScopeFile,
        flag: u32,
        cmp: CmpFile,
        value: i32,
    },
    Level {
        cmp: CmpFile,
        value: u32,
    },
    HasItem {
        item: u32,
        count: u32,
    },
    Actor(u32),
    Faction(u32),
    All(Vec<ConditionFile>),
    Any(Vec<ConditionFile>),
    Not(Box<ConditionFile>),
    /// 대상이 그 스킬을 몇 레벨 배웠나 (배우지 않았으면 0, R3).
    SkillLevel {
        skill: u32,
        cmp: CmpFile,
        value: u32,
    },
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
enum EffectFile {
    SetFlag {
        scope: ScopeFile,
        flag: u32,
        value: i32,
    },
    AddFlag {
        scope: ScopeFile,
        flag: u32,
        delta: i32,
    },
    GiveItem {
        item: u32,
        count: u32,
    },
    TakeItem {
        item: u32,
        count: u32,
    },
    GiveExp(u32),
    /// 스킬을 배운다 / `levels` 만큼 올린다 (R3). `levels` 를 적지 않으면 1.
    GrantSkill {
        skill: u32,
        #[serde(default = "one")]
        levels: u32,
    },
    RemoveSkill {
        skill: u32,
    },
}

fn one() -> u32 {
    1
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub(crate) enum ScopeFile {
    Character,
    World,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub(crate) enum CmpFile {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// 조건 중첩 상한 — 손으로 쓰는 표에서 이보다 깊으면 실수다 (그리고 판정이 재귀다).
const MAX_DEPTH: u32 = 8;

/// 트리거 표를 검증한다. `item_exists`·`actor_exists` 는 같은 파일의 표를 본다.
/// 오류는 `트리거 N: …` 꼴로 모아 돌려준다 — 다른 표의 검증과 같은 방식.
pub(crate) fn validate(
    triggers: &BTreeMap<u32, TriggerFile>,
    item_exists: impl Fn(u32) -> bool,
    actor_exists: impl Fn(u32) -> bool,
    skill_exists: impl Fn(u32) -> bool,
) -> Vec<String> {
    let mut errors = Vec::new();
    for (id, t) in triggers {
        let mut err = |msg: String| errors.push(format!("트리거 {id}: {msg}"));
        match t.on {
            OnFile::Killed { actor: Some(a) } if !actor_exists(a) => {
                err(format!("on: 없는 액터 {a}"));
            }
            OnFile::PickedUp { item: Some(i) } if !item_exists(i) => {
                err(format!("on: 없는 아이템 {i}"));
            }
            OnFile::LevelReached(0) => err(String::from("on: LevelReached 는 1 이상")),
            OnFile::Entered { center, radius }
                if !(center.0.is_finite()
                    && center.1.is_finite()
                    && radius.is_finite()
                    && radius > 0.0) =>
            {
                err(String::from(
                    "on: Entered 의 중심은 유한한 수, 반경은 0 보다 커야 함",
                ));
            }
            _ => {}
        }
        check_condition(
            &t.when,
            0,
            &item_exists,
            &actor_exists,
            &skill_exists,
            &mut err,
        );
        if t.then.is_empty() {
            err(String::from("then 이 비어 있음 — 하는 일이 없는 트리거"));
        }
        for e in &t.then {
            match *e {
                EffectFile::GiveItem { item, count } | EffectFile::TakeItem { item, count } => {
                    if !item_exists(item) {
                        err(format!("then: 없는 아이템 {item}"));
                    }
                    if count == 0 {
                        err(String::from("then: 아이템 개수가 0"));
                    }
                }
                EffectFile::GiveExp(0) => err(String::from("then: GiveExp 가 0")),
                EffectFile::GrantSkill { skill, .. } | EffectFile::RemoveSkill { skill }
                    if !skill_exists(skill) =>
                {
                    err(format!("then: 없는 스킬 {skill}"));
                }
                _ => {}
            }
        }
    }
    errors
}

/// 조건 하나를 검사한다 — 스킬의 `requires` 도 이것을 쓴다 (R3). 오류는 `when: …` 꼴.
pub(crate) fn check_condition(
    c: &ConditionFile,
    depth: u32,
    item_exists: &impl Fn(u32) -> bool,
    actor_exists: &impl Fn(u32) -> bool,
    skill_exists: &impl Fn(u32) -> bool,
    err: &mut impl FnMut(String),
) {
    if depth > MAX_DEPTH {
        err(format!("when: 조건이 {MAX_DEPTH} 겹보다 깊음"));
        return;
    }
    match c {
        ConditionFile::HasItem { item, .. } if !item_exists(*item) => {
            err(format!("when: 없는 아이템 {item}"));
        }
        ConditionFile::Actor(a) if !actor_exists(*a) => err(format!("when: 없는 액터 {a}")),
        ConditionFile::SkillLevel { skill, .. } if !skill_exists(*skill) => {
            err(format!("when: 없는 스킬 {skill}"));
        }
        ConditionFile::All(list) | ConditionFile::Any(list) => {
            for c in list {
                check_condition(c, depth + 1, item_exists, actor_exists, skill_exists, err);
            }
        }
        ConditionFile::Not(c) => {
            check_condition(c, depth + 1, item_exists, actor_exists, skill_exists, err);
        }
        _ => {}
    }
}

/// 트리거 하나가 가리키는 다른 표의 번호 — 콘텐츠 브라우저의 "쓰는 곳"·지우기 거부·번호 바꾸기용.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Refs {
    pub(crate) items: BTreeSet<u32>,
    pub(crate) actors: BTreeSet<u32>,
    pub(crate) skills: BTreeSet<u32>,
}

pub(crate) fn refs(t: &TriggerFile) -> Refs {
    let mut r = Refs::default();
    match t.on {
        OnFile::Killed { actor: Some(a) } => {
            r.actors.insert(a);
        }
        OnFile::PickedUp { item: Some(i) } => {
            r.items.insert(i);
        }
        _ => {}
    }
    condition_refs(&t.when, &mut r);
    for e in &t.then {
        match *e {
            EffectFile::GiveItem { item, .. } | EffectFile::TakeItem { item, .. } => {
                r.items.insert(item);
            }
            EffectFile::GrantSkill { skill, .. } | EffectFile::RemoveSkill { skill } => {
                r.skills.insert(skill);
            }
            _ => {}
        }
    }
    r
}

/// 조건 하나가 가리키는 번호 (스킬의 `requires` 용, R3).
pub(crate) fn condition_refs_of(c: &ConditionFile) -> Refs {
    let mut r = Refs::default();
    condition_refs(c, &mut r);
    r
}

fn condition_refs(c: &ConditionFile, r: &mut Refs) {
    match c {
        ConditionFile::HasItem { item, .. } => {
            r.items.insert(*item);
        }
        ConditionFile::Actor(a) => {
            r.actors.insert(*a);
        }
        ConditionFile::All(list) | ConditionFile::Any(list) => {
            for c in list {
                condition_refs(c, r);
            }
        }
        ConditionFile::Not(c) => condition_refs(c, r),
        ConditionFile::SkillLevel { skill, .. } => {
            r.skills.insert(*skill);
        }
        _ => {}
    }
}

/// 검증을 마친 표를 시뮬레이션 타입으로.
pub(crate) fn to_sim(triggers: &BTreeMap<u32, TriggerFile>) -> Vec<(TriggerId, TriggerDef)> {
    triggers
        .iter()
        .map(|(id, t)| {
            (
                TriggerId(*id),
                TriggerDef {
                    on: t.on.into(),
                    when: (&t.when).into(),
                    then: t.then.iter().map(|e| (*e).into()).collect(),
                },
            )
        })
        .collect()
}

impl From<OnFile> for On {
    fn from(o: OnFile) -> Self {
        match o {
            OnFile::Killed { actor } => On::Killed {
                actor: actor.map(ActorType),
            },
            OnFile::LevelReached(n) => On::LevelReached(n),
            OnFile::PickedUp { item } => On::PickedUp {
                item: item.map(ItemId),
            },
            OnFile::Entered { center, radius } => On::Entered {
                center: Vec2::new(center.0, center.1),
                radius,
            },
        }
    }
}

impl From<&ConditionFile> for Condition {
    fn from(c: &ConditionFile) -> Self {
        match c {
            ConditionFile::Always => Condition::Always,
            ConditionFile::Flag {
                scope,
                flag,
                cmp,
                value,
            } => Condition::Flag {
                scope: (*scope).into(),
                flag: FlagId(*flag),
                cmp: (*cmp).into(),
                value: *value,
            },
            ConditionFile::Level { cmp, value } => Condition::Level {
                cmp: (*cmp).into(),
                value: *value,
            },
            ConditionFile::HasItem { item, count } => Condition::HasItem {
                item: ItemId(*item),
                count: *count,
            },
            ConditionFile::Actor(a) => Condition::Actor(ActorType(*a)),
            ConditionFile::Faction(f) => Condition::Faction(FactionId(*f)),
            ConditionFile::All(list) => Condition::All(list.iter().map(Into::into).collect()),
            ConditionFile::Any(list) => Condition::Any(list.iter().map(Into::into).collect()),
            ConditionFile::Not(c) => Condition::Not(Box::new(c.as_ref().into())),
            ConditionFile::SkillLevel { skill, cmp, value } => Condition::SkillLevel {
                skill: SkillId(*skill),
                cmp: (*cmp).into(),
                value: *value,
            },
        }
    }
}

impl From<EffectFile> for Effect {
    fn from(e: EffectFile) -> Self {
        match e {
            EffectFile::SetFlag { scope, flag, value } => Effect::SetFlag {
                scope: scope.into(),
                flag: FlagId(flag),
                value,
            },
            EffectFile::AddFlag { scope, flag, delta } => Effect::AddFlag {
                scope: scope.into(),
                flag: FlagId(flag),
                delta,
            },
            EffectFile::GiveItem { item, count } => Effect::GiveItem {
                item: ItemId(item),
                count,
            },
            EffectFile::TakeItem { item, count } => Effect::TakeItem {
                item: ItemId(item),
                count,
            },
            EffectFile::GiveExp(n) => Effect::GiveExp(n),
            EffectFile::GrantSkill { skill, levels } => Effect::GrantSkill {
                skill: SkillId(skill),
                levels,
            },
            EffectFile::RemoveSkill { skill } => Effect::RemoveSkill {
                skill: SkillId(skill),
            },
        }
    }
}

impl From<ScopeFile> for FlagScope {
    fn from(s: ScopeFile) -> Self {
        match s {
            ScopeFile::Character => FlagScope::Character,
            ScopeFile::World => FlagScope::World,
        }
    }
}

impl From<CmpFile> for Cmp {
    fn from(c: CmpFile) -> Self {
        match c {
            CmpFile::Eq => Cmp::Eq,
            CmpFile::Ne => Cmp::Ne,
            CmpFile::Lt => Cmp::Lt,
            CmpFile::Le => Cmp::Le,
            CmpFile::Gt => Cmp::Gt,
            CmpFile::Ge => Cmp::Ge,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> BTreeMap<u32, TriggerFile> {
        ron::from_str(text).unwrap()
    }

    fn errors(text: &str) -> Vec<String> {
        validate(
            &parse(text),
            |i| i == 501,
            |a| a == 1 || a == 100,
            |s| s == 5,
        )
    }

    #[test]
    fn the_module_example_reads_and_converts() {
        let text = "{
            1: (on: Killed(actor: Some(100)), when: Actor(1),
                then: [AddFlag(scope: Character, flag: 1, delta: 1)]),
            2: (on: Killed(actor: Some(100)),
                when: All([
                    Flag(scope: Character, flag: 1, cmp: Ge, value: 5),
                    Not(Flag(scope: Character, flag: 2, cmp: Eq, value: 1)),
                ]),
                then: [GiveItem(item: 501, count: 3), SetFlag(scope: Character, flag: 2, value: 1)]),
            3: (on: LevelReached(5), then: [GiveExp(10)]),
        }";
        assert!(errors(text).is_empty(), "{:?}", errors(text));
        let sim = to_sim(&parse(text));
        assert_eq!(sim.len(), 3);
        assert_eq!(
            sim[0].1.on,
            On::Killed {
                actor: Some(ActorType(100))
            }
        );
        assert_eq!(
            sim[2].1.when,
            Condition::Always,
            "when 을 적지 않으면 늘 참"
        );
        assert_eq!(
            sim[1].1.then[0],
            Effect::GiveItem {
                item: ItemId(501),
                count: 3
            }
        );
    }

    #[test]
    fn dangling_numbers_and_empty_triggers_are_errors() {
        let e = errors(
            "{
            1: (on: Killed(actor: Some(7)), then: [GiveExp(1)]),
            2: (on: PickedUp(item: Some(9)), when: Any([HasItem(item: 8, count: 1)]), then: []),
            3: (on: LevelReached(0), then: [TakeItem(item: 501, count: 0)]),
            4: (on: Entered(center: (0.0, 0.0), radius: 0.0), then: [GiveExp(0)]),
        }",
        );
        for want in [
            "트리거 1: on: 없는 액터 7",
            "트리거 2: on: 없는 아이템 9",
            "트리거 2: when: 없는 아이템 8",
            "트리거 2: then 이 비어 있음",
            "트리거 3: on: LevelReached 는 1 이상",
            "트리거 3: then: 아이템 개수가 0",
            "트리거 4: on: Entered",
            "트리거 4: then: GiveExp 가 0",
        ] {
            assert!(e.iter().any(|m| m.starts_with(want)), "{want} 없음 — {e:?}");
        }
    }

    #[test]
    fn refs_walk_every_part_of_a_trigger() {
        let t = parse(
            "{ 1: (on: Killed(actor: Some(100)),
                   when: Not(Any([Actor(1), HasItem(item: 7, count: 1)])),
                   then: [GiveItem(item: 501, count: 1), GiveExp(5)]) }",
        );
        let r = refs(&t[&1]);
        assert_eq!(r.actors, BTreeSet::from([1, 100]));
        assert_eq!(r.items, BTreeSet::from([7, 501]));
    }

    #[test]
    fn typos_are_errors() {
        let bad = "{ 1: (on: Killed(actor: Some(100)), then: [GiveExp(1)], whne: Always) }";
        assert!(ron::from_str::<BTreeMap<u32, TriggerFile>>(bad).is_err());
    }
}
