//! 전투 규칙 — 스킬 데이터와 데미지 공식.
//!
//! **C++ 서버 `CombatProcessor::ProcessSingleTarget` 과 같은 규칙이다.** 단계 2 에서
//! 둘을 대조할 수 있도록 판정 순서와 공식을 그대로 옮겼다:
//!
//! 1. 공격자 생존 → 2. 대상 생존 → 3. 쿨타임 → (MP — 아직 없음) → 4. 사거리
//! 5. 쿨타임 등록 → 6. 데미지 `max(1, trunc(attack × damage_mult) − defense)` → 7. 적용
//!
//! 서버와 다른 점 (의도적):
//! - 사거리를 **지면(XY) 거리**로 잰다. 지형이 평지이고 높이는 이산 레벨이라 Z 는 0 이다.
//! - `immortal` 대상 공격을 거절한다. 서버는 `isImmortal` 필드만 있고 판정에 쓰지 않는다.
//! - 자기 자신은 대상이 될 수 없다.

use std::time::Duration;

/// 스킬 번호. 서버 `SkillDef::skillId` 와 같은 공간이다 — **ID 공간이 곧 프로토콜.**
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SkillId(pub u32);

/// 스킬 정적 데이터. 서버 `SkillDef` 의 단일 대상(Melee/Ranged) 부분.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkillDef {
    /// 최대 사거리 (m). 공격자와 대상의 지면 거리가 이 값 이하여야 한다.
    pub range: f32,
    /// 다시 쓸 때까지의 시간 (ms). 서버와 같은 단위.
    pub cooldown_ms: u32,
    /// 데미지 배율.
    pub damage_mult: f32,
}

impl SkillDef {
    #[must_use]
    pub fn cooldown(&self) -> Duration {
        Duration::from_millis(u64::from(self.cooldown_ms))
    }
}

/// 한 번의 적중 데미지. **최소 1** — 방어력이 높아도 0 이 되지 않는다.
///
/// 곱셈 결과는 0 쪽으로 자른다 (C++ `static_cast<int32_t>(float)` 와 같다).
#[must_use]
pub fn damage(attack: u32, damage_mult: f32, defense: u32) -> u32 {
    // NaN 은 `as` 변환에서 0 이 된다 — C++ 에서는 UB 지만 여기서는 최소 데미지로 떨어진다.
    let raw = (attack as f32 * damage_mult) as i64;
    (raw - i64::from(defense)).max(1).min(i64::from(u32::MAX)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damage_matches_server_formula() {
        // max(1, attack × mult − defense)
        assert_eq!(damage(20, 1.0, 5), 15);
        assert_eq!(damage(20, 2.5, 10), 40);
        // 곱셈 결과는 0 쪽으로 자른다: 15 × 1.5 = 22.5 → 22
        assert_eq!(damage(15, 1.5, 0), 22);
    }

    #[test]
    fn damage_is_at_least_one() {
        assert_eq!(damage(5, 1.0, 100), 1);
        assert_eq!(damage(0, 1.0, 0), 1);
        assert_eq!(damage(10, -3.0, 0), 1, "음수 배율");
        assert_eq!(damage(10, f32::NAN, 0), 1, "NaN 배율");
    }

    #[test]
    fn damage_saturates_instead_of_wrapping() {
        assert_eq!(damage(u32::MAX, 1000.0, 0), u32::MAX);
    }
}

/// Authority 를 거친 전투 흐름 — 판정 순서·쿨타임·사망.
#[cfg(test)]
mod flow_tests {
    use std::time::Duration;

    use nexus_core::{Entity, Vec2};

    use super::*;
    use crate::{
        Authority, Event, Intent, LocalAuthority, Rejection, SimWorld, Tile, TileMap, UnitDef,
    };

    const DT: Duration = Duration::from_millis(50);
    /// 근접 — 사거리 3m, 쿨타임 1초, 배율 1.0 (서버 샘플 근접 스킬과 같은 수치대).
    const MELEE: SkillId = SkillId(1);
    /// 원거리 — 사거리 20m, 쿨타임 2초, 배율 0.5.
    const ARROW: SkillId = SkillId(2);

    fn fighter() -> UnitDef {
        UnitDef {
            move_speed: 2.0,
            max_hp: 100,
            attack: 30,
            defense: 10,
            immortal: false,
        }
    }

    /// 20×20 평지, 공격자 (1.5, 1.5) / 대상 (3.5, 1.5) — 2m 거리.
    fn arena() -> (LocalAuthority, Entity, Entity) {
        let mut world = SimWorld::new(TileMap::new(20, 20, 1.0, Vec2::ZERO, Tile::default()));
        world.define_skill(
            MELEE,
            SkillDef {
                range: 3.0,
                cooldown_ms: 1000,
                damage_mult: 1.0,
            },
        );
        world.define_skill(
            ARROW,
            SkillDef {
                range: 20.0,
                cooldown_ms: 2000,
                damage_mult: 0.5,
            },
        );
        let a = world.spawn_unit(Vec2::new(1.5, 1.5), 0.0, fighter());
        let t = world.spawn_unit(Vec2::new(3.5, 1.5), 0.0, fighter());
        (LocalAuthority::new(world), a, t)
    }

    fn attack(
        auth: &mut LocalAuthority,
        unit: Entity,
        target: Entity,
        skill: SkillId,
    ) -> Vec<Event> {
        auth.submit(Intent::Attack {
            unit,
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

    fn hp(auth: &LocalAuthority, unit: Entity) -> u32 {
        auth.world().unit(unit).unwrap().hp()
    }

    #[test]
    fn hit_applies_server_formula() {
        let (mut auth, a, t) = arena();
        let events = attack(&mut auth, a, t, MELEE);
        // 30 × 1.0 − 10 = 20
        assert_eq!(
            events,
            vec![Event::Damaged {
                attacker: a,
                target: t,
                skill: MELEE,
                amount: 20,
                remaining_hp: 80,
            }]
        );
        assert_eq!(hp(&auth, t), 80);
    }

    #[test]
    fn attacker_turns_to_face_target() {
        // 공격자는 +X(0 rad) 를 보고 있다. 서쪽(-X) 대상을 치면 π 를 봐야 한다.
        let (mut auth, a, _) = arena();
        let behind = auth
            .world_mut()
            .spawn_unit(Vec2::new(0.5, 1.5), 0.0, fighter());
        attack(&mut auth, a, behind, MELEE);
        let heading = auth.world().unit(a).unwrap().heading();
        assert!((heading - std::f32::consts::PI).abs() < 1e-4, "{heading}");
    }

    #[test]
    fn cooldown_blocks_until_exactly_elapsed() {
        let (mut auth, a, t) = arena();
        assert!(rejected(&attack(&mut auth, a, t, MELEE)).is_none());
        // 1초 = 20 tick. 첫 공격이 tick 1 (시계 0ms) 이었으므로 tick 21 (시계 1000ms) 부터 가능.
        for n in 2..=20 {
            assert_eq!(
                rejected(&attack(&mut auth, a, t, MELEE)),
                Some(Rejection::OnCooldown),
                "tick {n}"
            );
        }
        assert!(
            rejected(&attack(&mut auth, a, t, MELEE)).is_none(),
            "tick 21"
        );
        assert_eq!(hp(&auth, t), 60);
    }

    #[test]
    fn cooldowns_are_per_skill() {
        let (mut auth, a, t) = arena();
        attack(&mut auth, a, t, MELEE);
        let events = attack(&mut auth, a, t, ARROW);
        assert!(
            rejected(&events).is_none(),
            "다른 스킬은 쿨타임을 공유하지 않는다"
        );
    }

    #[test]
    fn range_is_checked_on_the_ground_and_inclusive() {
        let (mut auth, a, _) = arena();
        // 정확히 3m — 경계는 포함.
        let edge = auth
            .world_mut()
            .spawn_unit(Vec2::new(1.5, 4.5), 0.0, fighter());
        assert!(rejected(&attack(&mut auth, a, edge, MELEE)).is_none());

        let (mut auth, a, _) = arena();
        let far = auth
            .world_mut()
            .spawn_unit(Vec2::new(1.5, 4.6), 0.0, fighter());
        assert_eq!(
            rejected(&attack(&mut auth, a, far, MELEE)),
            Some(Rejection::OutOfRange)
        );
        assert_eq!(hp(&auth, far), 100, "거절된 공격이 피해를 줬다");
        // 거절된 공격은 쿨타임도 걸지 않는다 (서버와 같다).
        assert!(
            auth.world()
                .unit(a)
                .unwrap()
                .is_ready(MELEE, auth.world().now())
        );
        // 원거리 스킬은 닿는다.
        assert!(rejected(&attack(&mut auth, a, far, ARROW)).is_none());
    }

    #[test]
    fn kill_emits_died_once_and_leaves_a_corpse() {
        let (mut auth, a, t) = arena();
        // 100 HP, 한 대 20 → 다섯 대. 쿨타임을 기다리며 친다.
        let mut died = 0;
        for _ in 0..200 {
            let events = attack(&mut auth, a, t, MELEE);
            died += events
                .iter()
                .filter(|e| **e == Event::Died { unit: t, killer: a })
                .count();
            if hp(&auth, t) == 0 {
                break;
            }
        }
        assert_eq!(died, 1);
        let corpse = auth.world().unit(t).expect("시체는 월드에 남는다");
        assert!(!corpse.is_alive());
        assert_eq!(auth.world().unit_count(), 2);
    }

    #[test]
    fn the_dead_cannot_act_and_cannot_be_hit() {
        let (mut auth, a, t) = arena();
        // 한 방에 죽도록 공격력이 큰 유닛을 따로 둔다.
        let brute = auth.world_mut().spawn_unit(
            Vec2::new(2.5, 2.5),
            0.0,
            UnitDef {
                attack: 1000,
                ..fighter()
            },
        );
        // 대상이 걷는 중에 죽으면 멈춘다.
        auth.submit(Intent::MoveTo {
            unit: t,
            target: Vec2::new(3.5, 3.5),
        });
        auth.tick(DT);
        attack(&mut auth, brute, t, MELEE);
        assert!(!auth.world().unit(t).unwrap().is_moving());
        let at = auth.world().unit(t).unwrap().pos();
        auth.tick(DT);
        assert_eq!(auth.world().unit(t).unwrap().pos(), at, "시체가 걸었다");

        // 시체는 움직이지도 공격하지도 못하고, 맞지도 않는다.
        auth.submit(Intent::MoveTo {
            unit: t,
            target: Vec2::new(10.5, 10.5),
        });
        assert_eq!(rejected(&auth.tick(DT)), Some(Rejection::Dead));
        assert_eq!(
            rejected(&attack(&mut auth, t, a, MELEE)),
            Some(Rejection::Dead)
        );
        assert_eq!(
            rejected(&attack(&mut auth, a, t, MELEE)),
            Some(Rejection::TargetDead)
        );
    }

    #[test]
    fn second_hit_in_the_same_tick_sees_the_death() {
        let (mut auth, _, t) = arena();
        let one_shot = UnitDef {
            attack: 1000,
            ..fighter()
        };
        let b = auth
            .world_mut()
            .spawn_unit(Vec2::new(2.5, 1.5), 0.0, one_shot);
        let c = auth
            .world_mut()
            .spawn_unit(Vec2::new(3.5, 2.5), 0.0, one_shot);
        for unit in [b, c] {
            auth.submit(Intent::Attack {
                unit,
                target: t,
                skill: MELEE,
            });
        }
        let events = auth.tick(DT);
        let deaths = events
            .iter()
            .filter(|e| matches!(e, Event::Died { .. }))
            .count();
        assert_eq!(deaths, 1);
        assert_eq!(rejected(&events), Some(Rejection::TargetDead));
    }

    #[test]
    fn check_order_matches_server() {
        // 서버: 공격자 생존 → 대상 생존 → 쿨타임 → 사거리.
        let (mut auth, a, t) = arena();
        let far = auth
            .world_mut()
            .spawn_unit(Vec2::new(15.5, 15.5), 0.0, fighter());
        attack(&mut auth, a, t, MELEE);
        // 쿨타임 중이면서 사거리 밖 → 쿨타임이 먼저.
        assert_eq!(
            rejected(&attack(&mut auth, a, far, MELEE)),
            Some(Rejection::OnCooldown)
        );
    }

    #[test]
    fn invalid_attacks_are_rejected() {
        let (mut auth, a, t) = arena();
        assert_eq!(
            rejected(&attack(&mut auth, a, a, MELEE)),
            Some(Rejection::InvalidTarget)
        );
        assert_eq!(
            rejected(&attack(&mut auth, a, t, SkillId(99))),
            Some(Rejection::UnknownSkill)
        );

        let guard = auth.world_mut().spawn_unit(
            Vec2::new(2.5, 1.5),
            0.0,
            UnitDef {
                immortal: true,
                ..fighter()
            },
        );
        assert_eq!(
            rejected(&attack(&mut auth, a, guard, MELEE)),
            Some(Rejection::Invulnerable)
        );

        auth.world_mut().despawn(t);
        assert_eq!(
            rejected(&attack(&mut auth, a, t, MELEE)),
            Some(Rejection::UnknownEntity)
        );
    }
}
