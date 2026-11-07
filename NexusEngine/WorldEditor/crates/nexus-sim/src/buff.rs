//! 버프·상태이상 — **지속 시간 동안 파생 스탯과 행동을 바꾸는 효과** (R2b).
//!
//! ```text
//! 스킬 적중 · 소모품 · 트리거 효과 ApplyBuff ──▶ Unit::buffs ──▶ attack()·defense()·max_hp() … 에 더해진다
//!                                                  │ step 맨 앞: 주기 HP(독·재생) → 끝난 것 지우기
//!                                                  ▼
//!                                       Event::BuffApplied / BuffTicked / BuffEnded
//! ```
//!
//! # 파생값에 더한다
//!
//! 수식 변수(`buff_*`)로 넣지 않고 **수식이 낸 파생값에 더한다.** 몬스터·NPC 는 수식이 없는데
//! 독과 약화는 그쪽에도 걸려야 하기 때문이다. 기본 스탯(힘 등)을 올리는 버프는 없다 — 공격·방어를 바로 올린다.
//!
//! # 규칙
//!
//! - **겹치지 않는다.** 같은 버프를 다시 걸면 시간만 처음부터 다시 잰다. 다른 버프끼리는 더해진다.
//! - 정수 연산이다. 시계도 시뮬레이션 시계뿐이라 결정적이다.
//! - 주기 피해는 **방어력을 보지 않고**, `immortal`·부활 보호 중에는 들어가지 않는다. 쓰러뜨리면 건 쪽의 처치다
//!   (경험치·보상 포함). 트리거가 건 독처럼 건 쪽이 없으면 `killer` 는 자기 자신이고 보상은 없다.
//! - 기절은 스킬을 막고(`Rejection::Stunned`) 걸음을 세운다 — 걷던 길은 풀리면 이어 간다. 침묵은 기본 공격이 아닌
//!   스킬을 막는다(`Rejection::Silenced`). 아이템 사용은 막지 않는다. 서버와 겹치는 판정 **뒤에** 붙였다.
//! - 죽으면 전부 사라진다. **저장하지 않는다** — 이어 하면 버프 없이 시작한다.

use std::time::Duration;

use nexus_core::Entity;

/// 버프 번호 — 서버와 같은 ID 공간.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BuffId(pub u32);

/// 버프 정의 — 데이터(`rules.ron` 의 `buffs`)에서 온다.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BuffDef {
    /// 지속 시간 (ms). 0 이면 걸리지 않는다.
    pub duration_ms: u32,
    pub attack: i32,
    pub defense: i32,
    pub max_hp: i32,
    pub max_mp: i32,
    pub mp_regen: i32,
    /// 이동 속도 증감 (%). `-50` 이면 절반, `-100` 이하면 멈춘다.
    pub move_pct: i32,
    /// 주기 HP 의 간격 (ms). 0 이면 주기 효과가 없다.
    pub tick_ms: u32,
    /// 주기마다 더할 HP. 음수면 피해(독), 양수면 회복(재생).
    pub tick_hp: i32,
    pub stun: bool,
    pub silence: bool,
}

/// 유닛에 걸려 있는 버프 하나. 정의를 복사해 들고 있어 유닛만 보고 스탯을 계산할 수 있다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActiveBuff {
    pub(crate) id: BuffId,
    pub(crate) def: BuffDef,
    pub(crate) until: Duration,
    pub(crate) next_tick: Duration,
    pub(crate) source: Option<Entity>,
}

impl ActiveBuff {
    #[must_use]
    pub fn id(&self) -> BuffId {
        self.id
    }

    #[must_use]
    pub fn def(&self) -> &BuffDef {
        &self.def
    }

    /// 남은 시간.
    #[must_use]
    pub fn remaining(&self, now: Duration) -> Duration {
        self.until.saturating_sub(now)
    }
}

#[cfg(test)]
mod tests {
    use nexus_core::Vec2;

    use super::*;
    use crate::On;
    use crate::{
        Authority, ClassDef, ClassId, Condition, Effect, Event, FactionId, Formula, Intent,
        ItemDef, ItemId, ItemKind, LocalAuthority, Rejection, SimWorld, SkillDef, SkillId,
        StatRules, Tile, TileMap, TriggerDef, TriggerId, UnitDef,
    };

    const DT: Duration = Duration::from_millis(50);
    const POISON: BuffId = BuffId(1);
    const GUARD: BuffId = BuffId(2);
    const STUN: BuffId = BuffId(3);
    const HIT: SkillId = SkillId(1);
    const POISON_DART: SkillId = SkillId(2);
    const FIRE: SkillId = SkillId(3);

    fn poison() -> BuffDef {
        BuffDef {
            duration_ms: 3000,
            tick_ms: 1000,
            tick_hp: -5,
            ..BuffDef::default()
        }
    }

    fn skill(mp_cost: u32) -> SkillDef {
        SkillDef::legacy(5.0, 0, 1.0, mp_cost, 0.0, 0)
    }

    /// 서로 적대하는 둘 — 몬스터(공격 0, 스킬 대상)와 플레이어 역할.
    fn arena() -> (LocalAuthority, Entity, Entity) {
        let mut w = SimWorld::new(TileMap::new(20, 20, 1.0, Vec2::ZERO, Tile::default()));
        w.define_buff(POISON, poison());
        w.define_buff(
            GUARD,
            BuffDef {
                duration_ms: 1000,
                attack: 7,
                defense: 10,
                max_hp: 50,
                move_pct: -50,
                ..BuffDef::default()
            },
        );
        w.define_buff(
            STUN,
            BuffDef {
                duration_ms: 500,
                stun: true,
                ..BuffDef::default()
            },
        );
        w.define_skill(HIT, skill(0));
        w.define_skill(POISON_DART, skill(0));
        w.define_skill_buff(POISON_DART, POISON);
        w.define_skill(FIRE, skill(1));
        let a = w.spawn_unit(
            Vec2::ZERO,
            0.0,
            UnitDef {
                max_hp: 100,
                max_mp: 10,
                attack: 10,
                move_speed: 2.0,
                faction: FactionId(1),
                basic_attack: Some(HIT),
                exp_reward: 0,
                ..UnitDef::default()
            },
        );
        let b = w.spawn_unit(
            Vec2::new(1.0, 0.0),
            0.0,
            UnitDef {
                max_hp: 100,
                attack: 10,
                move_speed: 2.0,
                faction: FactionId(2),
                exp_reward: 30,
                ..UnitDef::default()
            },
        );
        w.set_relation(FactionId(1), FactionId(2), crate::Relation::Hostile);
        (LocalAuthority::new(w), a, b)
    }

    fn run(auth: &mut LocalAuthority, ticks: u32) -> Vec<Event> {
        let mut all = Vec::new();
        for _ in 0..ticks {
            all.extend(auth.tick(DT));
        }
        all
    }

    fn hit_with(
        auth: &mut LocalAuthority,
        me: Entity,
        target: Entity,
        skill: SkillId,
    ) -> Vec<Event> {
        auth.submit(Intent::Attack {
            unit: me,
            target,
            skill,
        });
        auth.tick(DT)
    }

    #[test]
    fn modifiers_add_to_derived_values_and_end_with_the_buff() {
        let (mut auth, a, _) = arena();
        auth.world_mut().apply_buff(a, GUARD, None, &mut Vec::new());
        let u = auth.world().unit(a).unwrap();
        assert_eq!((u.attack(), u.defense(), u.max_hp()), (17, 10, 150));
        assert!((u.move_speed() - 1.0).abs() < 1e-6);

        let ev = run(&mut auth, 20);
        assert!(
            ev.iter()
                .any(|e| matches!(e, Event::BuffEnded { buff: GUARD, .. }))
        );
        let u = auth.world().unit(a).unwrap();
        assert_eq!((u.attack(), u.defense(), u.max_hp()), (10, 0, 100));
        assert!(u.buffs().is_empty());
    }

    #[test]
    fn losing_max_hp_clamps_current_hp() {
        let (mut auth, a, _) = arena();
        let w = auth.world_mut();
        w.apply_buff(a, GUARD, None, &mut Vec::new());
        w.unit_mut(a).unwrap().hp = 150;
        run(&mut auth, 20);
        assert_eq!(auth.world().unit(a).unwrap().hp(), 100);
    }

    #[test]
    fn poison_ignores_defense_ticks_on_schedule_and_is_deterministic() {
        let play = || {
            let (mut auth, a, b) = arena();
            hit_with(&mut auth, a, b, POISON_DART);
            let hp_after_hit = auth.world().unit(b).unwrap().hp();
            let ev = run(&mut auth, 80);
            let ticks: Vec<u32> = ev
                .iter()
                .filter_map(|e| match *e {
                    Event::BuffTicked {
                        hp_delta: -5,
                        remaining_hp,
                        ..
                    } => Some(remaining_hp),
                    _ => None,
                })
                .collect();
            (hp_after_hit, ticks)
        };
        let (hp, ticks) = play();
        // 1·2·3 초에 세 번, 그 뒤는 끝.
        assert_eq!(ticks, vec![hp - 5, hp - 10, hp - 15]);
        assert_eq!((hp, ticks), play());
    }

    #[test]
    fn reapplying_refreshes_without_stacking() {
        let (mut auth, a, b) = arena();
        hit_with(&mut auth, a, b, POISON_DART);
        run(&mut auth, 19);
        hit_with(&mut auth, a, b, POISON_DART);
        let u = auth.world().unit(b).unwrap();
        assert_eq!(u.buffs().len(), 1);
        assert_eq!(
            u.buffs()[0].remaining(auth.world().now()),
            Duration::from_millis(2950)
        );
    }

    #[test]
    fn a_poison_kill_belongs_to_whoever_applied_it() {
        let (mut auth, a, b) = arena();
        hit_with(&mut auth, a, b, POISON_DART);
        auth.world_mut().unit_mut(b).unwrap().hp = 3;
        let ev = run(&mut auth, 25);
        assert!(
            ev.iter()
                .any(|e| matches!(*e, Event::Died { unit, killer } if unit == b && killer == a))
        );
        assert!(
            ev.iter().any(
                |e| matches!(*e, Event::ExperienceGained { unit, amount: 30, .. } if unit == a)
            )
        );
        assert!(auth.world().unit(b).unwrap().buffs().is_empty());
    }

    #[test]
    fn immortal_units_take_no_poison() {
        let mut w = SimWorld::new(TileMap::new(5, 5, 1.0, Vec2::ZERO, Tile::default()));
        w.define_buff(POISON, poison());
        let npc = w.spawn_unit(
            Vec2::ZERO,
            0.0,
            UnitDef {
                max_hp: 10,
                immortal: true,
                ..UnitDef::default()
            },
        );
        w.apply_buff(npc, POISON, None, &mut Vec::new());
        let mut auth = LocalAuthority::new(w);
        run(&mut auth, 80);
        assert_eq!(auth.world().unit(npc).unwrap().hp(), 10);
    }

    #[test]
    fn stun_stops_walking_and_skills_then_walking_resumes() {
        let (mut auth, a, b) = arena();
        auth.submit(Intent::MoveTo {
            unit: a,
            target: Vec2::new(5.0, 5.0),
        });
        auth.world_mut().apply_buff(a, STUN, None, &mut Vec::new());
        let start = auth.world().unit(a).unwrap().pos();
        let ev = hit_with(&mut auth, a, b, HIT);
        assert!(ev.iter().any(|e| matches!(
            e,
            Event::Rejected {
                reason: Rejection::Stunned,
                ..
            }
        )));
        run(&mut auth, 5);
        assert_eq!(auth.world().unit(a).unwrap().pos(), start);
        run(&mut auth, 10);
        assert_ne!(auth.world().unit(a).unwrap().pos(), start);
    }

    #[test]
    fn silence_blocks_skills_but_not_the_basic_attack() {
        let (mut auth, a, b) = arena();
        let w = auth.world_mut();
        w.define_buff(
            BuffId(9),
            BuffDef {
                duration_ms: 5000,
                silence: true,
                ..BuffDef::default()
            },
        );
        w.apply_buff(a, BuffId(9), None, &mut Vec::new());
        let ev = hit_with(&mut auth, a, b, FIRE);
        assert!(ev.iter().any(|e| matches!(
            e,
            Event::Rejected {
                reason: Rejection::Silenced,
                ..
            }
        )));
        let ev = hit_with(&mut auth, a, b, HIT);
        assert!(ev.iter().any(|e| matches!(e, Event::Damaged { .. })));
    }

    #[test]
    fn a_stunned_monster_does_not_think() {
        let (mut auth, a, b) = arena();
        let w = auth.world_mut();
        w.apply_buff(b, STUN, None, &mut Vec::new());
        w.unit_mut(b).unwrap().def.ai = crate::AiKind::Aggressive;
        w.unit_mut(b).unwrap().def.aggro_range = 9.0;
        w.unit_mut(b).unwrap().def.leash_range = 9.0;
        w.unit_mut(b).unwrap().def.basic_attack = Some(HIT);
        let ev = run(&mut auth, 5);
        assert!(
            !ev.iter()
                .any(|e| matches!(*e, Event::Damaged { target, .. } if target == a))
        );
        assert!(!ev.iter().any(|e| matches!(e, Event::Rejected { .. })));
    }

    #[test]
    fn triggers_consumables_and_conditions_use_the_same_door() {
        let (mut auth, a, b) = arena();
        let w = auth.world_mut();
        let tonic = ItemId(50);
        w.define_item(
            tonic,
            ItemDef {
                kind: ItemKind::Consumable { heal: 0, mana: 0 },
                max_stack: 5,
            },
        );
        w.define_item_buff(tonic, GUARD);
        w.give_item(a, tonic, 1);
        w.define_trigger(
            TriggerId(1),
            TriggerDef {
                on: On::Killed { actor: None },
                when: Condition::HasBuff(GUARD),
                then: vec![Effect::RemoveBuff(GUARD), Effect::ApplyBuff(POISON)],
            },
        );
        auth.submit(Intent::UseItem { unit: a, slot: 0 });
        let ev = auth.tick(DT);
        assert!(
            ev.iter()
                .any(|e| matches!(*e, Event::BuffApplied { unit, buff: GUARD } if unit == a))
        );
        assert!(!ev.iter().any(|e| matches!(e, Event::Healed { .. })));

        auth.world_mut().unit_mut(b).unwrap().hp = 1;
        hit_with(&mut auth, a, b, HIT);
        let u = auth.world().unit(a).unwrap();
        assert!(!u.has_buff(GUARD));
        assert!(u.has_buff(POISON));
    }

    #[test]
    fn modifiers_apply_on_top_of_class_formulas() {
        let (mut auth, _, _) = arena();
        let num = |n| Formula::constant(n);
        let w = auth.world_mut();
        w.set_stat_rules(StatRules::new(
            vec![String::from("str")],
            [num(100), num(0), num(0), num(40), num(5)],
        ));
        w.define_class(
            ClassId(1),
            ClassDef {
                base: vec![1],
                growth: vec![0],
                formulas: Vec::new(),
            },
        );
        let hero = w.spawn_unit(
            Vec2::ZERO,
            0.0,
            UnitDef {
                class: Some(ClassId(1)),
                ..UnitDef::default()
            },
        );
        w.apply_buff(hero, GUARD, None, &mut Vec::new());
        let u = auth.world().unit(hero).unwrap();
        assert_eq!((u.attack(), u.defense(), u.max_hp()), (47, 15, 150));
    }
}
