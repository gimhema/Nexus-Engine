//! 던전 클리어 — **이 존(인스턴스)의 목표 + 보상** (R6-2).
//!
//! ```text
//! 레벨 파일 clear: (objectives: [Kill(actor: 103)], rewards: [GiveExp(200)])
//!        │  존을 열 때 앱이 등록한다 (SimWorld::set_clear)
//!        ▼
//! tick 끝: 트리거 → 퀘스트 → **클리어** → 포털
//!        │  목표를 다 채우면 한 번만
//!        ▼
//! Event::DungeonCleared + 보상(플레이어마다) ── Condition::Cleared 가 참이 된다 → 나가는 문(포털 requires) 열림
//! ```
//!
//! # 판정 체계를 새로 만들지 않는다
//!
//! 목표는 퀘스트의 [`Objective`] 그대로, 보상은 R1 의 [`Effect`] 그대로다. 퀘스트와 다른 점은
//! **누구의 것인가**뿐이다 — 퀘스트 진행은 캐릭터의 것이고, 클리어 진행은 **존(인스턴스)의 것**이다.
//!
//! - **처치**는 누가 쓰러뜨렸든 센다 — "보스가 쓰러졌다" 가 던전의 사실이다.
//! - **도달·대화**는 플레이어(직업이 있는 유닛)가 했을 때만 센다 — 쫓아오던 몬스터가 밟아도 세지 않는다
//!   (포털·스킬 제한과 같은 기준).
//! - **수집·조건**은 플레이어 **중 하나라도** 채우면 된다 (MMO 파티면 누구든).
//! - 보상은 클리어한 순간 그 존에 있는 **플레이어 모두**에게 준다.
//!
//! # 저장하지 않는다
//!
//! 클리어 상태와 인스턴스 플래그([`FlagScope::Instance`](crate::FlagScope::Instance))는 존과 함께 사라진다 —
//! 단계 1 에서 레벨은 열 때마다 처음 상태다. 단계 2 에서는 서버가 인스턴스마다 들고 있는다.
//! "한 번 깬 던전" 을 기억하려면 보상에 캐릭터·월드 플래그를 넣는다.

use crate::quest::Objective;
use crate::trigger::Effect;

/// 클리어 정의 — 존을 열 때 앱이 등록한다.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClearDef {
    /// 전부 채우면 클리어. 비어 있으면 클리어가 없는 존이다 (등록하지 않은 것과 같다).
    pub objectives: Vec<Objective>,
    /// 클리어한 순간 플레이어마다 적용한다.
    pub rewards: Vec<Effect>,
}

/// 클리어 진행 — `SimWorld` 가 들고 있다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClearState {
    /// 목표별 센 수 (상태 목표 자리는 0).
    pub(crate) counts: Vec<u32>,
    pub(crate) done: bool,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use nexus_core::{Entity, Vec2};

    use super::*;
    use crate::{
        ActorType, Authority, ClassId, Condition, Event, FactionId, FlagId, FlagScope, Intent,
        LocalAuthority, PortalDef, SimWorld, SkillDef, SkillId, Tile, TileMap, UnitDef,
    };

    const DT: Duration = Duration::from_millis(50);
    const HIT: SkillId = SkillId(1);
    const BOSS: ActorType = ActorType(103);
    const MINION: ActorType = ActorType(100);
    const DOOR: FlagId = FlagId(1);

    /// 플레이어 하나 + 보스 하나 + 졸개 하나. 플레이어는 한 방에 쓰러뜨린다.
    fn dungeon(def: ClearDef) -> (LocalAuthority, Entity, Entity, Entity) {
        let mut w = SimWorld::new(TileMap::new(30, 30, 1.0, Vec2::ZERO, Tile::default()));
        w.define_skill(
            HIT,
            SkillDef {
                range: 3.0,
                damage_mult: 1.0,
                ..SkillDef::default()
            },
        );
        w.set_clear(def);
        let me = w.spawn_unit(
            Vec2::new(5.5, 5.5),
            0.0,
            UnitDef {
                class: Some(ClassId(1)),
                max_hp: 100,
                attack: 1000,
                basic_attack: Some(HIT),
                move_speed: 5.0,
                faction: FactionId(1),
                ..UnitDef::default()
            },
        );
        let boss = w.spawn_unit(
            Vec2::new(6.5, 5.5),
            0.0,
            UnitDef {
                actor: BOSS,
                faction: FactionId(2),
                ..UnitDef::default()
            },
        );
        let minion = w.spawn_unit(
            Vec2::new(5.5, 6.5),
            0.0,
            UnitDef {
                actor: MINION,
                faction: FactionId(2),
                ..UnitDef::default()
            },
        );
        (LocalAuthority::new(w), me, boss, minion)
    }

    fn hit(auth: &mut LocalAuthority, me: Entity, target: Entity) -> Vec<Event> {
        auth.submit(Intent::Attack {
            unit: me,
            target,
            skill: HIT,
        });
        auth.tick(DT)
    }

    fn cleared(events: &[Event]) -> usize {
        events
            .iter()
            .filter(|e| matches!(e, Event::DungeonCleared))
            .count()
    }

    #[test]
    fn killing_the_boss_clears_once_and_rewards_the_player() {
        let (mut auth, me, boss, minion) = dungeon(ClearDef {
            objectives: vec![Objective::Kill {
                actor: Some(BOSS),
                count: 1,
            }],
            rewards: vec![
                Effect::GiveExp(50),
                Effect::SetFlag {
                    scope: FlagScope::Instance,
                    flag: DOOR,
                    value: 1,
                },
            ],
        });
        // 졸개는 세지 않는다.
        let events = hit(&mut auth, me, minion);
        assert_eq!(cleared(&events), 0);
        assert!(!auth.world().is_cleared());
        assert_eq!(auth.world().clear_counts(), Some(&[0][..]));

        let events = hit(&mut auth, me, boss);
        assert_eq!(cleared(&events), 1);
        assert!(events.contains(&Event::ClearProgressed {
            objective: 0,
            count: 1
        }));
        assert!(
            events.iter().any(
                |e| matches!(e, Event::ExperienceGained { unit, amount: 50, .. } if *unit == me)
            )
        );
        assert!(events.contains(&Event::FlagChanged {
            scope: FlagScope::Instance,
            unit: None,
            flag: DOOR,
            value: 1,
        }));
        let w = auth.world();
        assert!(w.is_cleared());
        assert!(w.check(&Condition::Cleared, me));
        assert_eq!(w.instance_flags().get(DOOR), 1);
        assert_eq!(
            w.world_flag(DOOR),
            0,
            "인스턴스 플래그는 월드 플래그와 따로다"
        );

        // 다시 일어나지 않는다.
        for _ in 0..5 {
            assert_eq!(cleared(&auth.tick(DT)), 0);
        }
    }

    #[test]
    fn the_exit_portal_opens_after_the_clear() {
        let (mut auth, me, boss, _) = dungeon(ClearDef {
            objectives: vec![Objective::Kill {
                actor: Some(BOSS),
                count: 1,
            }],
            rewards: Vec::new(),
        });
        auth.world_mut().add_portal(PortalDef {
            center: Vec2::new(5.5, 10.5),
            radius: 1.0,
            travel: 7,
            pos: Vec2::new(1.0, 1.0),
            requires: Condition::Cleared,
        });
        let walk = |auth: &mut LocalAuthority, to: Vec2| -> Vec<Event> {
            auth.submit(Intent::MoveTo {
                unit: me,
                target: to,
            });
            (0..30).flat_map(|_| auth.tick(DT)).collect()
        };
        let events = walk(&mut auth, Vec2::new(5.5, 10.5));
        assert!(events.contains(&Event::PortalBlocked {
            unit: me,
            portal: 0
        }));
        // 돌아와서 보스를 쓰러뜨리고 다시 들어간다.
        walk(&mut auth, Vec2::new(5.5, 5.5));
        assert_eq!(cleared(&hit(&mut auth, me, boss)), 1);
        let events = walk(&mut auth, Vec2::new(5.5, 10.5));
        assert!(events.contains(&Event::Traveled {
            unit: me,
            travel: 7,
            pos: Vec2::new(1.0, 1.0),
        }));
    }

    #[test]
    fn reaching_counts_only_players_and_all_objectives_are_needed() {
        let (mut auth, me, boss, minion) = dungeon(ClearDef {
            objectives: vec![
                Objective::Kill {
                    actor: Some(BOSS),
                    count: 1,
                },
                Objective::Reach {
                    center: Vec2::new(15.5, 5.5),
                    radius: 1.0,
                },
            ],
            rewards: Vec::new(),
        });
        // 졸개가 목표 자리를 밟아도 세지 않는다.
        auth.submit(Intent::MoveTo {
            unit: minion,
            target: Vec2::new(15.5, 6.5),
        });
        // 보스부터 — 처치만으로는 클리어가 아니다.
        let first = hit(&mut auth, me, boss);
        assert_eq!(cleared(&first), 0);
        let mut events = Vec::new();
        for _ in 0..80 {
            events.extend(auth.tick(DT));
        }
        assert_eq!(cleared(&events), 0, "졸개의 도달은 세지 않는다");
        assert_eq!(auth.world().clear_counts(), Some(&[1, 0][..]));

        auth.submit(Intent::MoveTo {
            unit: me,
            target: Vec2::new(15.5, 5.5),
        });
        let events: Vec<Event> = (0..80).flat_map(|_| auth.tick(DT)).collect();
        assert_eq!(cleared(&events), 1);
    }

    #[test]
    fn a_world_without_a_clear_is_never_cleared() {
        let mut w = SimWorld::new(TileMap::new(4, 4, 1.0, Vec2::ZERO, Tile::default()));
        assert!(!w.is_cleared());
        assert_eq!(w.clear_counts(), None);
        // 목표가 빈 정의는 등록하지 않은 것과 같다 — 곧바로 깨지면 곤란하다.
        w.set_clear(ClearDef::default());
        let mut auth = LocalAuthority::new(w);
        assert_eq!(cleared(&auth.tick(DT)), 0);
        assert!(!auth.world().is_cleared());
    }
}
