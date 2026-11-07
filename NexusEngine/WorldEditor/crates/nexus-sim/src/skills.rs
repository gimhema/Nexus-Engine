//! 스킬 부착 — **스킬은 독립된 객체이고, 캐릭터에 붙였다 뗀다** (R3).
//!
//! ```text
//!   얻는 경로 (전부 같은 문으로 들어온다)           부착 규칙 (SkillRules)
//!   ─ 직업의 시작 스킬 (설정 작업)               ─ 배울 수 있는 직업 (비우면 누구나)
//!   ─ 트리거 효과 GrantSkill — 퀘스트·레벨 보상   ─ 습득 조건 (R1 Condition — 레벨·다른 스킬·플래그…)
//!   ─ 스킬북 아이템 사용 — 드롭·상점
//!                     │
//!                     ▼
//!   SimWorld::learn(unit, skill)  ──▶  Unit::skills { 가진 스킬 }  ──▶  Event::SkillLearned
//! ```
//!
//! 습득 경로를 엔진이 하나로 정하지 않는다 *(사용자 결정 2026-10-24)* — 구매·퀘스트·드롭 어느 쪽이든
//! 데이터가 고른다. 엔진은 **붙이는 규칙 하나**만 갖고, 경로는 그 규칙을 부르는 입구일 뿐이다.
//!
//! # 스킬 레벨은 없다 (K1, 사용자 결정 2026-11-07)
//!
//! 스킬은 **처음 얻기만** 한다 — 포인트를 찍거나 레벨을 올리지 않는다. 이미 가진 스킬을 다시 배우면
//! `Rejection::SkillKnown`. 세기는 공식이 정한다: 위력·회복량·쿨타임·MP 를 숫자 대신 **공식**으로 적고,
//! 변수는 기본 스탯 + `level`(캐릭터 레벨)·`equip_attack`·`equip_defense` 다 (`stats` 모듈과 같은 문법).
//! 변주는 나중의 **스킬 특성**(K9)이 스킬 번호·표지에 수정자를 거는 식으로 붙는다.
//!
//! # 누가 배워야 쓰나
//!
//! **직업이 있는 유닛**(플레이어)은 가진 스킬과 자기 기본 공격만 쓸 수 있다
//! (`Rejection::SkillNotLearned`). 직업이 없는 유닛(몬스터·NPC)은 데이터에 적힌 스킬을 그냥 쓴다.
//! 이 판정은 서버와 겹치는 판정 **뒤에** 붙는다 (대조할 수 있게 — `combat` 모듈의 원칙).

use crate::stats::{ClassId, Formula, RESERVED};
use crate::trigger::Condition;

/// 스킬 공식이 쓸 수 있는 이름 — 기본 스탯 + [`RESERVED`] (`level`·`equip_attack`·`equip_defense`).
#[must_use]
pub fn skill_variables(stats: &[String]) -> Vec<String> {
    stats
        .iter()
        .cloned()
        .chain(RESERVED.iter().map(|s| (*s).to_owned()))
        .collect()
}

/// 스킬 하나의 부착 규칙과 수치 공식. 없으면 "누구나, 숫자 그대로" 다.
#[derive(Clone, Debug, PartialEq)]
pub struct SkillRules {
    /// 배울 수 있는 직업. 비우면 누구나 (직업이 없는 유닛도).
    pub classes: Vec<ClassId>,
    /// 습득 조건 — 배우는 유닛이 대상이다. 레벨을 올릴 때도 매번 본다.
    pub requires: Condition,
    /// 효과 수치 공식 `(효과 번호, 공식)` (K1) — 피해 효과면 위력 천분율, 회복이면 양을 덮는다.
    pub amounts: Vec<(usize, Formula)>,
    /// 수식 칸 — 있으면 `SkillDef` 의 그 값을 덮는다.
    pub cooldown_ms: Option<Formula>,
    pub mp_cost: Option<Formula>,
}

impl Default for SkillRules {
    fn default() -> Self {
        Self {
            classes: Vec::new(),
            requires: Condition::Always,
            amounts: Vec::new(),
            cooldown_ms: None,
            mp_cost: None,
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
    use crate::stats::{ClassDef, StatRules};
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
        SkillDef::legacy(range, 1000, 1.0, 0, 0.0, heal)
    }

    /// 마법사 하나 (기본 공격 = 베기) + 허수아비. 화염구는 마법사만, 공식은 지능·캐릭터 레벨.
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
        let sf = |s: &str| Formula::parse(s, &sv).unwrap();
        w.define_skill_rules(
            FIREBALL,
            SkillRules {
                classes: vec![MAGE],
                requires: Condition::Level {
                    cmp: Cmp::Ge,
                    value: 1,
                },
                // 위력 = 1000 + 캐릭터 레벨 × 500 (천분율), MP = 지능 ÷ 2, 쿨타임은 레벨마다 짧아진다.
                amounts: vec![(0, sf("1000 + level * 500"))],
                cooldown_ms: Some(sf("3000 - level * 500")),
                mp_cost: Some(sf("int / 2")),
            },
        );
        w.define_skill_rules(
            HEAL,
            SkillRules {
                amounts: vec![(0, sf("int * 2 + level * 10"))],
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
    fn a_skill_book_teaches_once_and_the_formulas_follow_the_character() {
        let (mut auth, me, dummy) = world(MAGE);
        assert!(auth.world_mut().give_item(me, BOOK, 2));
        auth.submit(Intent::UseItem { unit: me, slot: 0 });
        let events = auth.tick(DT);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::SkillLearned { skill, .. } if *skill == FIREBALL))
        );
        // 두 번째 책 — 이미 가졌으므로 거절되고 책은 남는다 (스킬 레벨이 없다).
        auth.submit(Intent::UseItem { unit: me, slot: 0 });
        assert_eq!(rejected(&auth.tick(DT)), Some(Rejection::SkillKnown));
        let left = auth
            .world()
            .unit(me)
            .unwrap()
            .inventory()
            .bag(BagKind::Consumable)
            .count_of(BOOK);
        assert_eq!(left, 1, "배우지 못한 책은 사라지지 않는다");

        // 캐릭터 레벨 1: 위력 1.5배 → 15, MP int 10 ÷ 2 = 5, 쿨타임 2.5초.
        let def = auth.world().effective_skill(me, FIREBALL).unwrap();
        assert_eq!((def.mp_cost, def.cooldown_ms), (5, 2500));
        assert_eq!(damage(&cast(&mut auth, me, dummy, FIREBALL)), 15);
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
                    Effect::GrantSkill { skill: FIREBALL },
                    Effect::GrantSkill { skill: HEAL },
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
        assert!(!u.knows(FIREBALL));
        assert!(u.knows(HEAL), "퀘스트·레벨 보상 경로");
    }

    #[test]
    fn skill_formulas_read_the_casters_stats_and_level() {
        let (mut auth, me, _) = world(MAGE);
        // heal = int 10 × 2 + 레벨 1 × 10
        assert_eq!(
            auth.world()
                .effective_skill(me, HEAL)
                .unwrap()
                .heal_amount(),
            Some(30)
        );
        auth.world_mut().set_known(me, HEAL, true);
        assert!(auth.world().unit(me).unwrap().knows(HEAL));
        auth.world_mut().set_known(me, HEAL, false);
        assert_eq!(auth.world().unit(me).unwrap().learned_skills().count(), 0);
        // 스킬 공식에 skill_level 은 없다.
        assert!(Formula::parse("skill_level * 2", &skill_variables(&[])).is_err());
    }

    #[test]
    fn has_skill_conditions_and_removal() {
        let (mut auth, me, _) = world(MAGE);
        let w = auth.world_mut();
        w.set_known(me, FIREBALL, true);
        let has = Condition::HasSkill(FIREBALL);
        assert!(w.check(&has, me));
        let mut events = Vec::new();
        w.forget(me, FIREBALL, &mut events);
        assert_eq!(
            events,
            [Event::SkillForgotten {
                unit: me,
                skill: FIREBALL
            }]
        );
        assert!(!w.check(&has, me));
    }
}
