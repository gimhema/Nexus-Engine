//! 전투 규칙 — 스킬 데이터와 데미지 공식.
//!
//! **C++ 서버 `CombatProcessor::ProcessSingleTarget` 과 같은 규칙이다.** 단계 2 에서
//! 둘을 대조할 수 있도록 판정 순서와 공식을 그대로 옮겼다:
//!
//! 1. 공격자 생존 → 2. 대상 생존 → 3. 쿨타임 → 3-1. MP → 4. 사거리
//! 5. 쿨타임 등록·MP 차감 → 6. 데미지 `max(1, trunc(attack × damage_mult) − defense)` → 7. 적용
//!
//! 서버와 다른 점 (의도적):
//! - 사거리를 **지면(XY) 거리**로 잰다. 지형이 평지이고 높이는 이산 레벨이라 Z 는 0 이다.
//! - `immortal` 대상 공격을 거절한다. 서버는 `isImmortal` 필드만 있고 판정에 쓰지 않는다.
//! - **우호 진영**은 공격할 수 없다 (서버 `EFactionRelation::FRIENDLY` 의 정의). 서버
//!   `CombatProcessor` 는 진영을 보지 않는다.
//! - 자기 자신은 대상이 될 수 없다.
//!
//! 위 셋은 사거리 확인 **뒤에** 본다 — 서버와 겹치는 판정의 순서를 그대로 두기 위해서다.

use std::time::Duration;

use nexus_core::Vec2;

/// 스킬 번호. 서버 `SkillDef::skillId` 와 같은 공간이다 — **ID 공간이 곧 프로토콜.**
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SkillId(pub u32);

/// 어디를 겨누는가 (K1) — 입력이 무엇을 골라야 하는지를 정한다.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Aim {
    /// 유닛 하나를 고른다 (대상 지정). 예전 스킬은 전부 이것이다.
    #[default]
    Unit,
    /// 땅의 한 점을 고른다 (지점 범위) — K2 의 `Intent::Cast` 로만 쓴다.
    Point,
    /// 방향을 고른다 (스킬샷) — K2 의 `Intent::Cast` 로만 쓴다.
    Direction,
    /// 고르지 않는다 — 시전자 자신이 기준이다 (자기 버프·주변 범위).
    Caster,
}

/// 판정 모양 (K1). 기준점은 조준 방식이 정한다 — 대상 지정이면 대상, 자신이면 시전자.
/// 직선·부채꼴은 **시전자에서** 겨눈 쪽으로 뻗는다.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Area {
    /// 고른 대상 하나 (자신 조준이면 시전자).
    #[default]
    Single,
    /// 기준점 둘레 반경 (m).
    Circle { radius: f32 },
    /// 시전자에서 겨눈 쪽으로 사거리만큼, 폭 (m).
    Line { width: f32 },
    /// 시전자에서 겨눈 쪽으로 반경 (m) · 벌어진 각 (라디안, 전체 각).
    Cone { radius: f32, angle: f32 },
}

/// 효과가 누구에게 들어가는가 (K1). 판정 모양 안의 유닛에서 고른다.
///
/// 고른 대상(대상 지정)은 조준 판정을 이미 통과했으므로 `Enemies`·`Allies` 에 **늘 들어간다**
/// — 중립 몬스터를 범위 스킬로 쳐도 그 몬스터는 맞는다 (예전 범위 스킬과 같다).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Affects {
    /// 고른 대상만 (자신 조준이면 시전자).
    Target,
    /// 고른 대상 + 판정 안의 적대 유닛 (불사·부활 보호 제외).
    Enemies,
    /// 고른 대상 + 판정 안의 우호 유닛 (시전자 제외).
    Allies,
    /// [`Allies`](Self::Allies) + 시전자.
    AlliesAndCaster,
    /// 시전자만 — 판정 모양과 무관하다.
    Caster,
}

/// 효과 한 가지 (K1). 수치는 쓸 때 공식으로 덮일 수 있다 (`SimWorld::effective_skill`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EffectKind {
    /// 피해 — 위력 천분율 (1000 = 공격력 그대로). 공식은 서버와 같다 ([`damage`]).
    Damage { power_per_mille: u32 },
    /// HP 회복.
    Heal { amount: u32 },
    /// 버프·상태이상을 건다.
    Buff(crate::BuffId),
}

/// 효과 하나 — 누구에게 무엇을.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkillEffect {
    pub to: Affects,
    pub kind: EffectKind,
}

/// 스킬 정적 데이터 (K1) — 쓰는 방식(사거리·쿨타임·MP·시전 시간·조준·판정 모양) + **효과 목록**.
///
/// 서버 `SkillDef` 의 단일 대상 스킬은 `Aim::Unit` + `Area::Single` + 피해 하나다 ([`SkillDef::legacy`]).
/// 효과는 적은 순서대로 들어간다 — 맞은 유닛마다 그 유닛에 해당하는 효과를 차례로.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkillDef {
    /// 최대 사거리 (m). 대상 지정이면 대상까지의 지면 거리, 직선이면 그 길이.
    pub range: f32,
    /// 다시 쓸 때까지의 시간 (ms). 서버와 같은 단위.
    pub cooldown_ms: u32,
    /// 쓸 때 드는 MP. 0 이면 MP 를 보지 않는다. 거절된 공격은 MP 를 쓰지 않는다 (쿨타임과 같다).
    pub mp_cost: u32,
    /// 시전 시간 (ms) — 0 이면 바로. **K2 에서 쓴다** (지금은 데이터 자리만).
    pub cast_ms: u32,
    pub aim: Aim,
    pub area: Area,
    pub effects: Vec<SkillEffect>,
}

impl SkillDef {
    /// 예전 한 줄 스킬 — 대상 지정, `radius > 0` 이면 대상 둘레 원, 피해 **또는** 회복 하나.
    ///
    /// 회복(`heal > 0`)이면 대상(+ 원 안의 우호·시전자)을, 아니면 대상(+ 원 안의 적대)을 친다.
    /// 예전 `attack` 의 판정과 결과가 같다.
    #[must_use]
    pub fn legacy(
        range: f32,
        cooldown_ms: u32,
        damage_mult: f32,
        mp_cost: u32,
        radius: f32,
        heal: u32,
    ) -> Self {
        let wide = radius > 0.0;
        let (to, kind) = if heal > 0 {
            let to = if wide {
                Affects::AlliesAndCaster
            } else {
                Affects::Target
            };
            (to, EffectKind::Heal { amount: heal })
        } else {
            let to = if wide {
                Affects::Enemies
            } else {
                Affects::Target
            };
            (
                to,
                EffectKind::Damage {
                    power_per_mille: per_mille(damage_mult),
                },
            )
        };
        Self {
            range,
            cooldown_ms,
            mp_cost,
            cast_ms: 0,
            aim: Aim::Unit,
            area: if wide {
                Area::Circle { radius }
            } else {
                Area::Single
            },
            effects: vec![SkillEffect { to, kind }],
        }
    }

    /// 피해 효과가 있는 스킬 — 대상 지정이면 적을 골라야 한다.
    #[must_use]
    pub fn is_harmful(&self) -> bool {
        self.effects
            .iter()
            .any(|e| matches!(e.kind, EffectKind::Damage { .. }))
    }

    /// 회복 스킬인가 — 피해 없이 회복이 있다. 대상 지정이면 자신이나 적대가 아닌 유닛을 고른다.
    #[must_use]
    pub fn is_heal(&self) -> bool {
        !self.is_harmful()
            && self
                .effects
                .iter()
                .any(|e| matches!(e.kind, EffectKind::Heal { .. }))
    }

    /// 첫 피해 효과의 위력 천분율 — UI 표시용.
    #[must_use]
    pub fn power_per_mille(&self) -> Option<u32> {
        self.effects.iter().find_map(|e| match e.kind {
            EffectKind::Damage { power_per_mille } => Some(power_per_mille),
            _ => None,
        })
    }

    /// 첫 회복 효과의 양 — UI 표시용.
    #[must_use]
    pub fn heal_amount(&self) -> Option<u32> {
        self.effects.iter().find_map(|e| match e.kind {
            EffectKind::Heal { amount } => Some(amount),
            _ => None,
        })
    }

    #[must_use]
    pub fn cooldown(&self) -> Duration {
        Duration::from_millis(u64::from(self.cooldown_ms))
    }
}

/// 배율 → 천분율. 예전 `damage_mult` 를 읽을 때만 쓴다 — 계산은 천분율로.
#[must_use]
pub fn per_mille(mult: f32) -> u32 {
    if mult.is_finite() && mult > 0.0 {
        (mult * 1000.0).round().min(u32::MAX as f32) as u32
    } else {
        0
    }
}

/// `pos` 가 판정 모양 안인가 (K1).
///
/// `origin` 은 시전자, `anchor` 는 기준점(대상·지점·시전자), `dir` 은 겨눈 쪽(정규화 안 해도 된다),
/// `length` 는 직선의 길이(사거리). `Single` 은 늘 `false` — 고른 대상은 부르는 쪽이 넣는다.
#[must_use]
pub fn in_area(area: Area, origin: Vec2, anchor: Vec2, dir: Vec2, length: f32, pos: Vec2) -> bool {
    let dir = dir.normalize_or_zero();
    match area {
        Area::Single => false,
        Area::Circle { radius } => pos.distance_squared(anchor) <= radius * radius,
        Area::Line { width } => {
            let d = pos - origin;
            let along = d.dot(dir);
            let across = (d - dir * along).length();
            dir != Vec2::ZERO && (0.0..=length).contains(&along) && across <= width * 0.5
        }
        Area::Cone { radius, angle } => {
            let d = pos - origin;
            if d.length_squared() > radius * radius {
                return false;
            }
            if d.length_squared() <= f32::EPSILON {
                return true;
            }
            dir != Vec2::ZERO && d.normalize().dot(dir) >= (angle * 0.5).cos()
        }
    }
}

/// 날아가는 공격 (T8) — 쏜 순간 사거리·쿨타임·MP 를 판정하고, **닿을 때** 피해와 버프가 들어간다.
///
/// 대상을 따라가므로 빗나가지 않는다. 닿기 전에 대상이 쓰러지면 사라진다. 시뮬레이션 안의 상태라 결정적이다.
#[derive(Clone, Debug, PartialEq)]
pub struct Projectile {
    pub(crate) attacker: nexus_core::Entity,
    pub(crate) target: nexus_core::Entity,
    pub(crate) skill: SkillId,
    /// 닿을 때 이 유닛에게 들어갈 효과 — 쏠 때 판정 모양·대상 필터로 골라 둔 것.
    pub(crate) effects: Vec<EffectKind>,
    pub(crate) speed: f32,
    pub(crate) pos: nexus_core::Vec2,
    pub(crate) prev_pos: nexus_core::Vec2,
}

impl Projectile {
    #[must_use]
    pub fn pos(&self) -> nexus_core::Vec2 {
        self.pos
    }

    /// 직전 tick 의 위치 — 렌더 보간용.
    #[must_use]
    pub fn prev_pos(&self) -> nexus_core::Vec2 {
        self.prev_pos
    }

    #[must_use]
    pub fn skill(&self) -> SkillId {
        self.skill
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

    /// 투사체 (T8) — 쏜 순간에는 피해가 없고, 거리만큼 뒤에 들어가며, 버프도 닿을 때 걸린다.
    #[test]
    fn a_projectile_hits_when_it_arrives_and_vanishes_if_the_target_dies() {
        use nexus_core::Vec2;

        use crate::{
            Authority, BuffDef, BuffId, Event, FactionId, Intent, LocalAuthority, Relation,
            SimWorld, Tile, TileMap, UnitDef,
        };
        let dt = Duration::from_millis(50);
        let bolt = SkillId(1);
        let mut w = SimWorld::new(TileMap::new(20, 20, 1.0, Vec2::ZERO, Tile::default()));
        w.define_skill(bolt, SkillDef::legacy(10.0, 0, 1.0, 0, 0.0, 0));
        w.define_skill_projectile(bolt, 10.0);
        w.define_buff(
            BuffId(1),
            BuffDef {
                duration_ms: 5000,
                defense: 1,
                ..BuffDef::default()
            },
        );
        w.define_skill_buff(bolt, BuffId(1));
        w.set_relation(FactionId(1), FactionId(2), Relation::Hostile);
        let unit = |faction, hp| UnitDef {
            faction: FactionId(faction),
            max_hp: hp,
            attack: 10,
            basic_attack: Some(bolt),
            ..UnitDef::default()
        };
        let me = w.spawn_unit(Vec2::new(1.0, 1.0), 0.0, unit(1, 100));
        let far = w.spawn_unit(Vec2::new(6.0, 1.0), 0.0, unit(2, 100));
        let near = w.spawn_unit(Vec2::new(1.0, 3.0), 0.0, unit(2, 10));
        let mut auth = LocalAuthority::new(w);

        auth.submit(Intent::Attack {
            unit: me,
            target: far,
            skill: bolt,
        });
        let first = auth.tick(dt);
        assert!(!first.iter().any(|e| matches!(e, Event::Damaged { .. })));
        assert_eq!(auth.world().projectiles().len(), 1);
        // 5m ÷ 10m/s = 0.5초.
        let mut hit_at = None;
        for t in 1..20 {
            if auth
                .tick(dt)
                .iter()
                .any(|e| matches!(*e, Event::Damaged { target, .. } if target == far))
            {
                hit_at = Some(t);
                break;
            }
        }
        assert_eq!(hit_at, Some(9));
        assert!(auth.world().projectiles().is_empty());
        assert!(
            auth.world().unit(far).unwrap().has_buff(BuffId(1)),
            "버프는 닿을 때"
        );

        // 날아가는 동안 대상이 쓰러지면 사라진다.
        auth.submit(Intent::Attack {
            unit: me,
            target: near,
            skill: bolt,
        });
        auth.tick(dt);
        auth.world_mut().unit_mut(near).unwrap().hp = 0;
        let events = auth.tick(dt);
        assert!(auth.world().projectiles().is_empty());
        assert!(!events.iter().any(|e| matches!(e, Event::Damaged { .. })));
    }

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
            ..UnitDef::default()
        }
    }

    /// 20×20 평지, 공격자 (1.5, 1.5) / 대상 (3.5, 1.5) — 2m 거리.
    fn arena() -> (LocalAuthority, Entity, Entity) {
        let mut world = SimWorld::new(TileMap::new(20, 20, 1.0, Vec2::ZERO, Tile::default()));
        world.define_skill(MELEE, SkillDef::legacy(3.0, 1000, 1.0, 0, 0.0, 0));
        world.define_skill(ARROW, SkillDef::legacy(20.0, 2000, 0.5, 0, 0.0, 0));
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
    fn a_respawning_unit_comes_back_fresh_at_its_spawn_point_after_its_time() {
        let (mut auth, a, _) = arena();
        // 한 방(30 − 10 = 20)에 죽고 1초 뒤 리스폰.
        let def = UnitDef {
            max_hp: 20,
            respawn_ms: 1000,
            ..fighter()
        };
        let spawn = Vec2::new(3.5, 2.5);
        let t = auth.world_mut().spawn_unit(spawn, 1.0, def);
        // 죽기 전에 자리를 옮겨 둔다 — 리스폰은 죽은 자리가 아니라 스폰 지점이다.
        auth.world_mut().unit_mut(t).unwrap().pos = Vec2::new(2.5, 2.5);
        let died = attack(&mut auth, a, t, MELEE); // tick 1 — 시계 0 에 죽는다
        assert!(died.contains(&Event::Died { unit: t, killer: a }));

        for n in 2..=19 {
            let events = auth.tick(DT);
            assert!(
                !events.iter().any(|e| matches!(e, Event::Respawned { .. })),
                "tick {n}: 아직"
            );
            assert!(auth.world().unit(t).is_some(), "그동안은 시체로 남는다");
        }
        let events = auth.tick(DT); // tick 20 — 시계 1000ms
        let new = events
            .iter()
            .find_map(|e| match *e {
                Event::Respawned { unit, replaces } if replaces == t => Some(unit),
                _ => None,
            })
            .expect("1초 뒤 리스폰");
        assert_ne!(new, t, "새 핸들");
        assert!(auth.world().unit(t).is_none(), "시체는 치워졌다");
        let u = auth.world().unit(new).unwrap();
        assert_eq!((u.hp(), u.pos(), u.heading()), (20, spawn, 1.0));
        assert!(u.is_ready(MELEE, auth.world().now()));
    }

    #[test]
    fn revive_stands_the_same_unit_up_with_part_hp_and_an_exp_penalty() {
        let (mut auth, a, _) = arena();
        let t = auth.world_mut().spawn_unit(
            Vec2::new(3.5, 2.5),
            0.0,
            UnitDef {
                max_hp: 20,
                max_mp: 40,
                ..fighter()
            },
        );
        auth.world_mut()
            .set_progress(t, crate::Progress::new(2, 300));
        let revive = |auth: &mut LocalAuthority| {
            auth.submit(Intent::Revive {
                unit: t,
                at: Vec2::new(10.5, 10.5),
                hp_per_mille: 500,
                exp_loss_per_mille: 100,
                protect_ms: 0,
            });
            auth.tick(DT)
        };
        assert_eq!(
            rejected(&revive(&mut auth)),
            Some(Rejection::NotDead),
            "살아 있으면 거절"
        );

        attack(&mut auth, a, t, MELEE); // 한 방에 쓰러진다 — 쿨타임이 남는다
        let events = revive(&mut auth);
        assert!(events.contains(&Event::Revived {
            unit: t,
            pos: Vec2::new(10.5, 10.5),
            hp: 10,
            exp_lost: 30,
        }));
        let u = auth.world().unit(t).unwrap();
        assert!(u.is_alive(), "같은 핸들");
        assert_eq!((u.hp(), u.mp(), u.pos()), (10, 20, Vec2::new(10.5, 10.5)));
        assert_eq!(
            (u.progress().level(), u.progress().exp()),
            (2, 270),
            "레벨은 그대로"
        );
    }

    /// 진영 둘(1 ↔ 2 적대)로 나눈 범위 스킬 시험장 — `[시전자, 대상, 대상 옆 적, 먼 적, 대상 옆 아군]`.
    fn melee_crowd() -> (LocalAuthority, [Entity; 5]) {
        use crate::{FactionId, Relation};
        let mut world = SimWorld::new(TileMap::new(20, 20, 1.0, Vec2::ZERO, Tile::default()));
        world.set_relation(FactionId(1), FactionId(2), Relation::Hostile);
        world.define_skill(WHIRL, SkillDef::legacy(3.0, 1000, 1.0, 0, 2.5, 0));
        world.define_skill(MEND, SkillDef::legacy(5.0, 1000, 0.0, 0, 3.0, 50));
        let side = |faction| UnitDef {
            faction: FactionId(faction),
            ..fighter()
        };
        let caster = world.spawn_unit(Vec2::new(1.5, 1.5), 0.0, side(1));
        let target = world.spawn_unit(Vec2::new(3.5, 1.5), 0.0, side(2));
        let near = world.spawn_unit(Vec2::new(5.5, 1.5), 0.0, side(2));
        let far = world.spawn_unit(Vec2::new(9.5, 1.5), 0.0, side(2));
        let ally = world.spawn_unit(Vec2::new(3.5, 3.5), 0.0, side(1));
        (
            LocalAuthority::new(world),
            [caster, target, near, far, ally],
        )
    }

    const WHIRL: SkillId = SkillId(7);
    const MEND: SkillId = SkillId(8);

    #[test]
    fn an_area_skill_hits_every_hostile_around_the_target_and_nobody_else() {
        let (mut auth, [caster, target, near, far, ally]) = melee_crowd();
        let events = attack(&mut auth, caster, target, WHIRL);
        let hit: Vec<Entity> = events
            .iter()
            .filter_map(|e| match e {
                Event::Damaged { target, .. } => Some(*target),
                _ => None,
            })
            .collect();
        // 대상이 먼저, 그다음 범위 안의 적 — 먼 적·아군·시전자는 맞지 않는다.
        assert_eq!(hit, [target, near]);
        for e in [far, ally, caster] {
            assert_eq!(auth.world().unit(e).unwrap().hp(), 100, "{e:?}");
        }
        // 쿨타임은 한 번만 — 여러 마리를 맞혀도 스킬은 한 번 쓴 것이다.
        assert_eq!(
            rejected(&attack(&mut auth, caster, near, WHIRL)),
            Some(Rejection::OnCooldown)
        );
    }

    #[test]
    fn a_heal_skill_mends_self_and_allies_and_refuses_enemies() {
        let (mut auth, [caster, target, near, _, ally]) = melee_crowd();
        // 아군·시전자를 다치게 해 둔다.
        auth.world_mut().set_hp(caster, 40);
        auth.world_mut().set_hp(ally, 90);
        auth.world_mut().set_hp(near, 10);

        // 적에게는 쓸 수 없다.
        assert_eq!(
            rejected(&attack(&mut auth, caster, target, MEND)),
            Some(Rejection::InvalidTarget)
        );
        // 자신에게 — 범위 안의 아군도 함께, 적은 빠진다. 최대치를 넘지 않는다.
        let events = attack(&mut auth, caster, caster, MEND);
        let healed: Vec<(Entity, u32)> = events
            .iter()
            .filter_map(|e| match e {
                Event::Healed { unit, amount, .. } => Some((*unit, *amount)),
                _ => None,
            })
            .collect();
        assert_eq!(healed, [(caster, 50), (ally, 10)]);
        assert_eq!(auth.world().unit(caster).unwrap().hp(), 90);
        assert_eq!(auth.world().unit(ally).unwrap().hp(), 100);
        assert_eq!(
            auth.world().unit(near).unwrap().hp(),
            10,
            "적은 회복하지 않는다"
        );
    }

    #[test]
    fn a_revived_unit_is_protected_for_a_while() {
        let (mut auth, a, _) = arena();
        let t = auth.world_mut().spawn_unit(
            Vec2::new(3.5, 2.5),
            0.0,
            UnitDef {
                max_hp: 20,
                ..fighter()
            },
        );
        attack(&mut auth, a, t, MELEE);
        for _ in 0..40 {
            auth.tick(DT); // 쿨타임이 풀리게
        }
        auth.submit(Intent::Revive {
            unit: t,
            at: Vec2::new(3.5, 2.5),
            hp_per_mille: 1000,
            exp_loss_per_mille: 0,
            protect_ms: 1000,
        });
        auth.tick(DT);
        let now = auth.world().now();
        assert!(auth.world().unit(t).unwrap().is_protected(now));
        let mut hit_at = None;
        for n in 1..=40 {
            let events = attack(&mut auth, a, t, MELEE);
            if events.iter().any(|e| matches!(e, Event::Damaged { .. })) {
                hit_at = Some(n);
                break;
            }
            assert_eq!(rejected(&events), Some(Rejection::Invulnerable), "{n}");
        }
        // 일어난 tick 에서 1초 (20 tick) — 그 뒤 첫 일격이 들어간다.
        assert_eq!(hit_at, Some(20));
        let u = auth.world().unit(t).unwrap();
        assert_eq!(u.protection_left(auth.world().now()), Duration::ZERO);
    }

    #[test]
    fn a_revived_unit_does_not_also_respawn() {
        let (mut auth, a, _) = arena();
        let t = auth.world_mut().spawn_unit(
            Vec2::new(3.5, 2.5),
            0.0,
            UnitDef {
                max_hp: 20,
                respawn_ms: 1000,
                ..fighter()
            },
        );
        attack(&mut auth, a, t, MELEE);
        auth.submit(Intent::Revive {
            unit: t,
            at: Vec2::new(3.5, 2.5),
            hp_per_mille: 1000,
            exp_loss_per_mille: 0,
            protect_ms: 0,
        });
        for _ in 0..60 {
            assert!(
                !auth
                    .tick(DT)
                    .iter()
                    .any(|e| matches!(e, Event::Respawned { .. })),
                "부활하면 리스폰 예약이 지워진다"
            );
        }
    }

    #[test]
    fn units_without_respawn_stay_as_corpses() {
        let (mut auth, a, _) = arena();
        let t = auth.world_mut().spawn_unit(
            Vec2::new(3.5, 2.5),
            0.0,
            UnitDef {
                max_hp: 20,
                ..fighter()
            },
        );
        attack(&mut auth, a, t, MELEE);
        for _ in 0..200 {
            assert!(
                !auth
                    .tick(DT)
                    .iter()
                    .any(|e| matches!(e, Event::Respawned { .. }))
            );
        }
        assert!(!auth.world().unit(t).unwrap().is_alive());
    }

    #[test]
    fn cooldown_left_counts_down_to_zero() {
        let (mut auth, a, t) = arena();
        let left = |auth: &LocalAuthority| {
            let w = auth.world();
            w.unit(a).unwrap().cooldown_left(MELEE, w.now())
        };
        assert_eq!(left(&auth), Duration::ZERO, "쓴 적 없으면 0");
        attack(&mut auth, a, t, MELEE); // 시계 0 에 써서 1000ms 에 풀린다. 지금 시계 50ms.
        assert_eq!(left(&auth), Duration::from_millis(950));
        for _ in 0..19 {
            auth.tick(DT);
        }
        assert_eq!(left(&auth), Duration::ZERO);
        assert_eq!(
            auth.world()
                .unit(a)
                .unwrap()
                .cooldown_left(ARROW, Duration::ZERO),
            Duration::ZERO
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

    // ── 경험치·레벨 (P3) ────────────────────────────────────────────────────

    /// 죽으면 경험치를 주는 대상으로 바꾼 무대 — 성장치는 레벨마다 HP +50 · 공격 +5.
    fn exp_arena() -> (LocalAuthority, Entity, Entity) {
        let (mut auth, _, _) = arena();
        let world = auth.world_mut();
        let killer = world.spawn_unit(
            Vec2::new(1.5, 1.5),
            0.0,
            UnitDef {
                growth: crate::Growth {
                    max_hp: 50,
                    max_mp: 0,
                    attack: 5,
                    defense: 0,
                },
                ..fighter()
            },
        );
        let prey = world.spawn_unit(
            Vec2::new(3.5, 1.5),
            0.0,
            UnitDef {
                max_hp: 30,
                exp_reward: 120,
                ..fighter()
            },
        );
        (auth, killer, prey)
    }

    #[test]
    fn killing_grants_experience_and_levels_up_the_killer() {
        let (mut auth, a, t) = exp_arena();
        // 30HP 짜리 대상은 한 방(30 - 10 = 20 피해)에 죽지 않는다 — 두 번 친다.
        let mut events = attack(&mut auth, a, t, MELEE);
        for _ in 0..20 {
            events = auth.tick(DT);
        }
        events.extend(attack(&mut auth, a, t, MELEE));

        assert!(events.iter().any(|e| matches!(e, Event::Died { .. })));
        let gained = events.iter().find_map(|e| match e {
            Event::ExperienceGained {
                amount, progress, ..
            } => Some((*amount, *progress)),
            _ => None,
        });
        let (amount, progress) = gained.expect("경험치 이벤트가 없다");
        assert_eq!((amount, progress.level(), progress.exp()), (120, 2, 20));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::LeveledUp { level: 2, .. }))
        );

        // 레벨 2 → 성장치가 붙고 HP 가 가득 찬다.
        let u = auth.world().unit(a).unwrap();
        assert_eq!((u.max_hp(), u.attack(), u.hp()), (150, 35, 150));
    }

    #[test]
    fn a_kill_without_a_reward_changes_nothing() {
        let (mut auth, a, t) = arena(); // exp_reward 0
        attack(&mut auth, a, t, MELEE);
        for _ in 0..200 {
            let events = auth.tick(DT);
            assert!(
                !events
                    .iter()
                    .any(|e| matches!(e, Event::ExperienceGained { .. })),
                "보상이 0 이면 이벤트도 없다"
            );
            auth.submit(Intent::Attack {
                unit: a,
                target: t,
                skill: MELEE,
            });
        }
        let u = auth.world().unit(a).unwrap();
        assert_eq!((u.progress().level(), u.max_hp()), (1, 100), "성장치 없음");
    }

    #[test]
    fn a_restored_level_survives_into_combat() {
        // 저장 파일에서 되살리는 경로 — 레벨을 심으면 수치와 HP 상한이 함께 오른다.
        let (mut auth, a, t) = exp_arena();
        auth.world_mut()
            .set_progress(a, crate::Progress::new(3, 10));
        auth.world_mut().set_hp(a, 999);
        let u = auth.world().unit(a).unwrap();
        assert_eq!((u.max_hp(), u.attack(), u.hp()), (200, 40, 200));

        // 공격력 40 - 방어 10 = 30 피해.
        let events = attack(&mut auth, a, t, MELEE);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::Damaged { amount, .. } if *amount == 30))
        );
    }

    // ── MP ───────────────────────────────────────────────────────────────────

    /// MP 30 이 드는 원거리 주문. 쿨타임이 없어 MP 만이 제약이다.
    const BOLT: SkillId = SkillId(3);

    /// 최대 MP 50 · 초당 회복 `regen` 인 시전자를 공격자 자리에 하나 더 세운다.
    fn caster_arena(regen: u32) -> (LocalAuthority, Entity, Entity) {
        let (mut auth, _, t) = arena();
        let world = auth.world_mut();
        world.define_skill(BOLT, SkillDef::legacy(20.0, 0, 1.0, 30, 0.0, 0));
        let caster = world.spawn_unit(
            Vec2::new(1.5, 2.5),
            0.0,
            UnitDef {
                max_mp: 50,
                mp_regen: regen,
                ..fighter()
            },
        );
        (auth, caster, t)
    }

    fn mp(auth: &LocalAuthority, unit: Entity) -> u32 {
        auth.world().unit(unit).unwrap().mp()
    }

    #[test]
    fn a_skill_spends_mp_and_is_refused_when_short() {
        let (mut auth, c, t) = caster_arena(0);
        assert_eq!(mp(&auth, c), 50, "가득 찬 채로 태어난다");
        assert!(rejected(&attack(&mut auth, c, t, BOLT)).is_none());
        assert_eq!(mp(&auth, c), 20);

        let before = hp(&auth, t);
        assert_eq!(
            rejected(&attack(&mut auth, c, t, BOLT)),
            Some(Rejection::NotEnoughMp)
        );
        assert_eq!(
            (mp(&auth, c), hp(&auth, t)),
            (20, before),
            "거절된 공격은 MP 도 피해도 없다"
        );
        // MP 를 보지 않는 스킬은 그대로 쓸 수 있다.
        assert!(rejected(&attack(&mut auth, c, t, MELEE)).is_none());
    }

    #[test]
    fn mp_check_comes_after_cooldown_like_the_server_slot() {
        // 쿨타임과 MP 가 둘 다 모자라면 쿨타임이 먼저 보고된다 (3 → 3-1 순서).
        let (mut auth, c, t) = caster_arena(0);
        auth.world_mut()
            .define_skill(BOLT, SkillDef::legacy(20.0, 5000, 1.0, 30, 0.0, 0));
        attack(&mut auth, c, t, BOLT);
        assert_eq!(
            rejected(&attack(&mut auth, c, t, BOLT)),
            Some(Rejection::OnCooldown)
        );
    }

    #[test]
    fn mp_regenerates_in_whole_points_and_stops_at_max() {
        // 초당 3 — tick(50ms) 마다 0.15 씩 모였다가 정수로 넘어간다.
        let (mut auth, c, t) = caster_arena(3);
        attack(&mut auth, c, t, BOLT); // tick 1: 50 → 20, 자투리 0.15
        for _ in 2..=19 {
            auth.tick(DT);
        }
        assert_eq!(mp(&auth, c), 22, "0.95초 — 2.85 가 모였다");
        auth.tick(DT);
        assert_eq!(mp(&auth, c), 23, "1초 — 정확히 3");
        for _ in 0..400 {
            auth.tick(DT);
        }
        assert_eq!(mp(&auth, c), 50, "상한에서 멈춘다");
    }

    #[test]
    fn dead_units_do_not_regenerate() {
        let (mut auth, c, _) = caster_arena(100);
        auth.world_mut().set_mp(c, 0);
        auth.world_mut().unit_mut(c).unwrap().hp = 0;
        for _ in 0..40 {
            auth.tick(DT);
        }
        assert_eq!(mp(&auth, c), 0);
    }

    #[test]
    fn levelling_up_grows_and_refills_mp() {
        let (mut auth, a, t) = exp_arena();
        auth.world_mut().unit_mut(a).unwrap().def.growth.max_mp = 10;
        auth.world_mut().unit_mut(a).unwrap().def.max_mp = 40;
        auth.world_mut().set_mp(a, 5);
        attack(&mut auth, a, t, MELEE);
        for _ in 0..20 {
            auth.tick(DT);
        }
        attack(&mut auth, a, t, MELEE); // 120 경험치 → 레벨 2
        let u = auth.world().unit(a).unwrap();
        assert_eq!(u.progress().level(), 2);
        assert_eq!(
            (u.max_mp(), u.mp()),
            (50, 50),
            "성장치만큼 오르고 가득 찬다"
        );
    }
}

/// 스킬 데이터 모양 (K1) — 조준·판정 모양·효과 목록.
#[cfg(test)]
mod shape_tests {
    use std::f32::consts::FRAC_PI_2;
    use std::time::Duration;

    use nexus_core::{Entity, Vec2};

    use super::*;
    use crate::{
        Authority, BuffDef, BuffId, Event, FactionId, Intent, LocalAuthority, Rejection, Relation,
        SimWorld, Tile, TileMap, UnitDef,
    };

    const DT: Duration = Duration::from_millis(50);
    const SPELL: SkillId = SkillId(1);

    #[test]
    fn areas_cover_what_they_say() {
        let o = Vec2::ZERO;
        let east = Vec2::X;
        let circle = Area::Circle { radius: 2.0 };
        assert!(in_area(
            circle,
            o,
            Vec2::new(5.0, 0.0),
            east,
            8.0,
            Vec2::new(6.5, 0.5)
        ));
        assert!(!in_area(
            circle,
            o,
            Vec2::new(5.0, 0.0),
            east,
            8.0,
            Vec2::new(7.5, 0.0)
        ));
        // 직선 — 시전자에서 사거리까지, 폭의 절반 안.
        let line = Area::Line { width: 1.0 };
        assert!(in_area(line, o, o, east, 8.0, Vec2::new(7.9, 0.4)));
        assert!(
            !in_area(line, o, o, east, 8.0, Vec2::new(7.9, 0.6)),
            "폭 밖"
        );
        assert!(
            !in_area(line, o, o, east, 8.0, Vec2::new(-0.5, 0.0)),
            "등 뒤"
        );
        assert!(
            !in_area(line, o, o, east, 8.0, Vec2::new(8.5, 0.0)),
            "사거리 밖"
        );
        // 부채꼴 — 90° 면 좌우 45° 까지.
        let cone = Area::Cone {
            radius: 3.0,
            angle: FRAC_PI_2,
        };
        assert!(in_area(cone, o, o, east, 0.0, Vec2::new(2.0, 1.9)));
        assert!(
            !in_area(cone, o, o, east, 0.0, Vec2::new(1.0, 1.5)),
            "각 밖"
        );
        assert!(
            !in_area(cone, o, o, east, 0.0, Vec2::new(3.5, 0.0)),
            "반경 밖"
        );
        assert!(!in_area(Area::Single, o, o, east, 9.0, o));
    }

    #[test]
    fn legacy_skills_read_as_one_effect() {
        let hit = SkillDef::legacy(2.0, 800, 1.2, 0, 0.0, 0);
        assert_eq!(hit.aim, Aim::Unit);
        assert_eq!(hit.area, Area::Single);
        assert_eq!(hit.power_per_mille(), Some(1200));
        assert!(hit.is_harmful() && !hit.is_heal());
        let mend = SkillDef::legacy(5.0, 1000, 0.0, 10, 3.0, 50);
        assert!(mend.is_heal());
        assert_eq!(mend.effects[0].to, Affects::AlliesAndCaster);
        assert_eq!(per_mille(0.8), 800);
    }

    /// 시전자 (5, 5) 에서 동쪽을 본다. 적 셋: 가까운 동쪽·먼 동쪽·서쪽. 아군 하나.
    fn arena(def: SkillDef) -> (LocalAuthority, Entity, [Entity; 4]) {
        let mut w = SimWorld::new(TileMap::new(20, 20, 1.0, Vec2::ZERO, Tile::default()));
        w.set_relation(FactionId(1), FactionId(2), Relation::Hostile);
        w.define_skill(SPELL, def);
        w.define_buff(
            BuffId(1),
            BuffDef {
                duration_ms: 5000,
                defense: 1,
                ..BuffDef::default()
            },
        );
        let unit = |faction, hp| UnitDef {
            faction: FactionId(faction),
            max_hp: hp,
            attack: 20,
            ..UnitDef::default()
        };
        let me = w.spawn_unit(Vec2::new(5.0, 5.0), 0.0, unit(1, 100));
        let near = w.spawn_unit(Vec2::new(6.5, 5.0), 0.0, unit(2, 100));
        let far = w.spawn_unit(Vec2::new(9.0, 5.2), 0.0, unit(2, 100));
        let west = w.spawn_unit(Vec2::new(3.5, 5.0), 0.0, unit(2, 100));
        let friend = w.spawn_unit(Vec2::new(5.0, 6.0), 0.0, unit(1, 100));
        auth_hurt(&mut w, me);
        (LocalAuthority::new(w), me, [near, far, west, friend])
    }

    /// 시전자 HP 를 깎아 둔다 — 회복이 보이게.
    fn auth_hurt(w: &mut SimWorld, me: Entity) {
        w.set_hp(me, 40);
    }

    fn cast(auth: &mut LocalAuthority, me: Entity, target: Entity) -> Vec<Event> {
        auth.submit(Intent::Attack {
            unit: me,
            target,
            skill: SPELL,
        });
        auth.tick(DT)
    }

    fn damaged(events: &[Event]) -> Vec<Entity> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::Damaged { target, .. } => Some(*target),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_self_cast_hits_enemies_around_and_heals_the_caster() {
        // 회오리 — 자신 둘레 2m 의 적에게 피해 + 둔화 버프, 자신은 회복.
        let def = SkillDef {
            range: 0.0,
            aim: Aim::Caster,
            area: Area::Circle { radius: 2.0 },
            effects: vec![
                SkillEffect {
                    to: Affects::Enemies,
                    kind: EffectKind::Damage {
                        power_per_mille: 1000,
                    },
                },
                SkillEffect {
                    to: Affects::Enemies,
                    kind: EffectKind::Buff(BuffId(1)),
                },
                SkillEffect {
                    to: Affects::Caster,
                    kind: EffectKind::Heal { amount: 15 },
                },
            ],
            ..SkillDef::default()
        };
        let (mut auth, me, [near, far, west, friend]) = arena(def);
        // 대상은 무엇을 넘겨도 시전자다.
        let events = cast(&mut auth, me, far);
        let hit = damaged(&events);
        assert!(hit.contains(&near) && hit.contains(&west), "{hit:?}");
        assert!(!hit.contains(&far) && !hit.contains(&friend) && !hit.contains(&me));
        let w = auth.world();
        assert!(w.unit(near).unwrap().has_buff(BuffId(1)));
        assert!(!w.unit(friend).unwrap().has_buff(BuffId(1)));
        assert_eq!(w.unit(me).unwrap().hp(), 55, "40 + 15");
    }

    #[test]
    fn a_line_from_the_caster_to_the_target_hits_everything_on_it() {
        let def = SkillDef {
            range: 8.0,
            area: Area::Line { width: 1.0 },
            effects: vec![SkillEffect {
                to: Affects::Enemies,
                kind: EffectKind::Damage {
                    power_per_mille: 1000,
                },
            }],
            ..SkillDef::default()
        };
        let (mut auth, me, [near, far, west, _]) = arena(def);
        let hit = damaged(&cast(&mut auth, me, far));
        assert_eq!(hit, vec![far, near], "고른 대상이 먼저, 그다음 줄 위의 적");
        assert!(!hit.contains(&west));
    }

    #[test]
    fn point_and_direction_skills_need_a_cast() {
        let def = SkillDef {
            range: 8.0,
            aim: Aim::Point,
            area: Area::Circle { radius: 2.0 },
            ..SkillDef::default()
        };
        let (mut auth, me, [near, ..]) = arena(def);
        let events = cast(&mut auth, me, near);
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Rejected {
                reason: Rejection::NeedsAim,
                ..
            }
        )));
    }
}
