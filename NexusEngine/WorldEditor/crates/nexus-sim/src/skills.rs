//! 스킬 부착과 스킬 레벨 — **스킬은 독립된 객체이고, 캐릭터에 붙였다 뗀다** (R3).
//!
//! ```text
//!   얻는 경로 (전부 같은 문으로 들어온다)           부착 규칙 (SkillRules)
//!   ─ 직업의 시작 스킬 (설정 작업)               ─ 배울 수 있는 직업 (비우면 누구나)
//!   ─ 트리거 효과 GrantSkill — 퀘스트·레벨 보상   ─ 습득 조건 (R1 Condition — 레벨·다른 스킬·플래그…)
//!   ─ 스킬북 아이템 사용 — 드롭·상점            ─ 최대 레벨
//!                     │
//!                     ▼
//!   SimWorld::learn(unit, skill, +레벨)  ──▶  Unit::skills { 스킬 → 레벨 }  ──▶  Event::SkillLearned
//! ```
//!
//! 습득 경로를 엔진이 하나로 정하지 않는다 *(사용자 결정 2026-10-24)* — 구매·퀘스트·드롭 어느 쪽이든
//! 데이터가 고른다. 엔진은 **붙이는 규칙 하나**만 갖고, 경로는 그 규칙을 부르는 입구일 뿐이다.
//! 새 경로(NPC 가 가르쳐 주기 등)도 같은 문을 부르면 된다.
//!
//! # 누가 배워야 쓰나
//!
//! **직업이 있는 유닛**(플레이어)은 배운 스킬과 자기 기본 공격만 쓸 수 있다
//! (`Rejection::SkillNotLearned`). 직업이 없는 유닛(몬스터·NPC)은 지금처럼 데이터에 적힌 스킬을 그냥 쓴다.
//! 이 판정은 서버와 겹치는 판정 **뒤에** 붙는다 (대조할 수 있게 — `combat` 모듈의 원칙).
//!
//! # 스킬 레벨 수식
//!
//! 스킬의 위력(천분율)·쿨타임·MP·회복량은 숫자 대신 **수식**으로 적을 수 있다. 수식 문법은
//! `stats` 모듈과 같고, 변수는 기본 스탯 + `level`·`equip_attack`·`equip_defense` + **`skill_level`** 이다.
//! 그래서 "지능에 비례하는 치유" 도 데이터로 된다(`heal: "20 + int * 3 + skill_level * 10"`).
//! 배우지 않은 스킬(몬스터의 스킬, 기본 공격)은 `skill_level = 1` 로 계산한다.

use crate::stats::{ClassId, Formula, RESERVED};
use crate::trigger::Condition;

/// 스킬 수식이 기본 스탯·[`RESERVED`] 뒤에 더 읽는 이름.
pub const SKILL_LEVEL_VAR: &str = "skill_level";

/// 스킬 수식이 쓸 수 있는 이름 — 기본 스탯 + [`RESERVED`] + [`SKILL_LEVEL_VAR`].
#[must_use]
pub fn skill_variables(stats: &[String]) -> Vec<String> {
    stats
        .iter()
        .cloned()
        .chain(RESERVED.iter().map(|s| (*s).to_owned()))
        .chain(std::iter::once(SKILL_LEVEL_VAR.to_owned()))
        .collect()
}

/// 스킬 하나의 부착 규칙과 레벨 수식. 없으면 "누구나, 최대 1레벨, 숫자 그대로" 다.
#[derive(Clone, Debug, PartialEq)]
pub struct SkillRules {
    /// 최대 레벨 (1 이상).
    pub max_level: u32,
    /// 배울 수 있는 직업. 비우면 누구나 (직업이 없는 유닛도).
    pub classes: Vec<ClassId>,
    /// 습득 조건 — 배우는 유닛이 대상이다. 레벨을 올릴 때도 매번 본다.
    pub requires: Condition,
    /// 수식 칸 — 있으면 `SkillDef` 의 그 값을 덮는다.
    pub power_per_mille: Option<Formula>,
    pub cooldown_ms: Option<Formula>,
    pub mp_cost: Option<Formula>,
    pub heal: Option<Formula>,
}

impl Default for SkillRules {
    fn default() -> Self {
        Self {
            max_level: 1,
            classes: Vec::new(),
            requires: Condition::Always,
            power_per_mille: None,
            cooldown_ms: None,
            mp_cost: None,
            heal: None,
        }
    }
}

impl SkillRules {
    /// `class` 가 배울 수 있는가.
    #[must_use]
    pub fn allows(&self, class: Option<ClassId>) -> bool {
        self.classes.is_empty() || class.is_some_and(|c| self.classes.contains(&c))
    }
}

/// 수식 결과를 `u32` 로 — 음수는 0.
pub(crate) fn to_u32(v: i64) -> u32 {
    u32::try_from(v.max(0)).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use nexus_core::{Entity, Vec2};

    use super::*;
    use crate::stats::{ClassDef, DerivedStat, StatRules};
    use crate::trigger::{Cmp, Effect, On, TriggerDef, TriggerId};
    use crate::{
        Authority, BagKind, Event, Intent, ItemDef, ItemId, ItemKind, LocalAuthority, Rejection,
        SimWorld, SkillDef, SkillId, Tile, TileMap, UnitDef,
    };

    const DT: Duration = Duration::from_millis(50);
    const SLASH: SkillId = SkillId(1);
    const FIREBALL: SkillId = SkillId(2);
    const HEAL: SkillId = SkillId(3);
    const MAGE: ClassId = ClassId(2);
    const WARRIOR: ClassId = ClassId(1);
    const BOOK: ItemId = ItemId(700);

    fn skill(range: f32, heal: u32) -> SkillDef {
        SkillDef {
            range,
            cooldown_ms: 1000,
            damage_mult: 1.0,
            mp_cost: 0,
            radius: 0.0,
            heal,
        }
    }

    /// 마법사 하나 (기본 공격 = 베기) + 허수아비. 화염구는 마법사만, 레벨 수식 있음.
    fn world(class: ClassId) -> (LocalAuthority, Entity, Entity) {
        let mut w = SimWorld::new(TileMap::new(20, 20, 1.0, Vec2::ZERO, Tile::default()));
        let stats = vec![String::from("int")];
        let v = StatRules::variables(&stats);
        let f = |s: &str| Formula::parse(s, &v).unwrap();
        w.set_stat_rules(StatRules::new(
            stats.clone(),
            [f("1000"), f("100"), f("0"), f("10"), f("0")],
        ));
        for c in [WARRIOR, MAGE] {
            w.define_class(
                c,
                ClassDef {
                    base: vec![10],
                    ..ClassDef::default()
                },
            );
        }
        w.define_skill(SLASH, skill(3.0, 0));
        w.define_skill(FIREBALL, skill(8.0, 0));
        w.define_skill(HEAL, skill(0.0, 1));
        let sv = skill_variables(&stats);
        let sf = |s: &str| Some(Formula::parse(s, &sv).unwrap());
        w.define_skill_rules(
            FIREBALL,
            SkillRules {
                max_level: 3,
                classes: vec![MAGE],
                requires: Condition::Level {
                    cmp: Cmp::Ge,
                    value: 1,
                },
                // 위력 = 1000 + 500 × 레벨 (천분율), MP = 레벨 × 4, 쿨타임은 레벨마다 짧아진다.
                power_per_mille: sf("1000 + skill_level * 500"),
                cooldown_ms: sf("3000 - skill_level * 500"),
                mp_cost: sf("skill_level * 4"),
                heal: None,
            },
        );
        w.define_skill_rules(
            HEAL,
            SkillRules {
                max_level: 5,
                heal: sf("int * 2 + skill_level * 10"),
                ..SkillRules::default()
            },
        );
        w.define_item(
            BOOK,
            ItemDef {
                kind: ItemKind::SkillBook { skill: FIREBALL },
                max_stack: 5,
            },
        );
        let me = w.spawn_unit(
            Vec2::new(5.5, 5.5),
            0.0,
            UnitDef {
                class: Some(class),
                basic_attack: Some(SLASH),
                ..UnitDef::default()
            },
        );
        let dummy = w.spawn_unit(
            Vec2::new(7.5, 5.5),
            0.0,
            UnitDef {
                max_hp: 10_000,
                ..UnitDef::default()
            },
        );
        (LocalAuthority::new(w), me, dummy)
    }

    fn cast(auth: &mut LocalAuthority, me: Entity, target: Entity, skill: SkillId) -> Vec<Event> {
        auth.submit(Intent::Attack {
            unit: me,
            target,
            skill,
        });
        auth.tick(DT)
    }

    fn rejected(events: &[Event]) -> Option<Rejection> {
        events.iter().find_map(|e| match e {
            Event::Rejected { reason, .. } => Some(*reason),
            _ => None,
        })
    }

    fn damage(events: &[Event]) -> u32 {
        events
            .iter()
            .find_map(|e| match e {
                Event::Damaged { amount, .. } => Some(*amount),
                _ => None,
            })
            .unwrap_or(0)
    }

    #[test]
    fn a_class_unit_uses_only_learned_skills_and_its_basic_attack() {
        let (mut auth, me, dummy) = world(MAGE);
        assert_eq!(
            damage(&cast(&mut auth, me, dummy, SLASH)),
            10,
            "기본 공격은 배우지 않아도"
        );
        assert_eq!(
            rejected(&cast(&mut auth, me, dummy, FIREBALL)),
            Some(Rejection::SkillNotLearned)
        );
    }

    #[test]
    fn a_skill_book_teaches_then_levels_up_and_the_formulas_follow() {
        let (mut auth, me, dummy) = world(MAGE);
        assert!(auth.world_mut().give_item(me, BOOK, 4));
        let mut learned = Vec::new();
        for _ in 0..4 {
            auth.submit(Intent::UseItem { unit: me, slot: 0 });
            let events = auth.tick(DT);
            learned.extend(events.iter().filter_map(|e| match e {
                Event::SkillLearned { level, .. } => Some(*level),
                _ => None,
            }));
            if let Some(r) = rejected(&events) {
                assert_eq!(r, Rejection::SkillMaxed, "네 번째는 최대 레벨");
            }
        }
        assert_eq!(learned, [1, 2, 3]);
        let left = auth
            .world()
            .unit(me)
            .unwrap()
            .inventory()
            .bag(BagKind::Consumable)
            .count_of(BOOK);
        assert_eq!(left, 1, "배우지 못한 책은 사라지지 않는다");

        // 3레벨 화염구: 위력 2.5배 → 25, MP 12, 쿨타임 1.5초.
        let def = auth.world().effective_skill(me, FIREBALL).unwrap();
        assert_eq!((def.mp_cost, def.cooldown_ms), (12, 1500));
        assert_eq!(damage(&cast(&mut auth, me, dummy, FIREBALL)), 25);
    }

    #[test]
    fn the_wrong_class_cannot_learn_and_effects_fail_quietly() {
        let (mut auth, me, _) = world(WARRIOR);
        assert!(auth.world_mut().give_item(me, BOOK, 1));
        auth.submit(Intent::UseItem { unit: me, slot: 0 });
        assert_eq!(rejected(&auth.tick(DT)), Some(Rejection::SkillRequirement));
        // 트리거 효과는 조용히 실패한다 (다른 효과처럼).
        let w = auth.world_mut();
        w.define_trigger(
            TriggerId(1),
            TriggerDef {
                on: On::LevelReached(2),
                when: Condition::Always,
                then: vec![
                    Effect::GrantSkill {
                        skill: FIREBALL,
                        levels: 1,
                    },
                    Effect::GrantSkill {
                        skill: HEAL,
                        levels: 2,
                    },
                ],
            },
        );
        let victim = w.spawn_unit(
            Vec2::new(6.5, 5.5),
            0.0,
            UnitDef {
                exp_reward: 100,
                ..UnitDef::default()
            },
        );
        cast(&mut auth, me, victim, SLASH);
        let u = auth.world().unit(me).unwrap();
        assert_eq!(u.skill_level(FIREBALL), 0);
        assert_eq!(u.skill_level(HEAL), 2, "퀘스트·레벨 보상 경로");
    }

    #[test]
    fn skill_formulas_read_the_casters_stats() {
        let (mut auth, me, _) = world(MAGE);
        auth.world_mut().set_skill_level(me, HEAL, 3);
        // heal = int 10 × 2 + 3 × 10
        assert_eq!(auth.world().effective_skill(me, HEAL).unwrap().heal, 50);
        auth.world_mut().set_skill_level(me, HEAL, 99);
        assert_eq!(
            auth.world().unit(me).unwrap().skill_level(HEAL),
            5,
            "최대 레벨로 자른다"
        );
        auth.world_mut().set_skill_level(me, HEAL, 0);
        assert_eq!(
            auth.world().unit(me).unwrap().learned_skills().count(),
            0,
            "0 이면 뗀다"
        );
        let _ = DerivedStat::MaxHp;
    }

    #[test]
    fn skill_level_conditions_and_removal() {
        let (mut auth, me, _) = world(MAGE);
        let w = auth.world_mut();
        w.set_skill_level(me, FIREBALL, 2);
        let at_least_2 = Condition::SkillLevel {
            skill: FIREBALL,
            cmp: Cmp::Ge,
            value: 2,
        };
        assert!(w.check(&at_least_2, me));
        let mut events = Vec::new();
        w.forget(me, FIREBALL, &mut events);
        assert_eq!(
            events,
            [Event::SkillForgotten {
                unit: me,
                skill: FIREBALL
            }]
        );
        assert!(!w.check(&at_least_2, me));
    }
}
