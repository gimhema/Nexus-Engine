//! Intent → Authority → World — 게임플레이 상태를 바꾸는 유일한 경로.
//!
//! ```text
//! 입력  →  Intent  →  [ Authority ]  →  SimWorld  →  렌더
//!                          │
//!             단계 1: LocalAuthority  (즉시 처리)
//!             단계 2: ServerAuthority (전송 + 로컬 예측)
//! ```
//!
//! 입력이 월드를 직접 고치면 서버를 붙일 때 게임플레이 코드를 전부 다시 써야 한다.
//! 호출하는 쪽은 Authority 가 로컬인지 원격인지 모른다 — 그래서 [`Authority::submit`] 은
//! 결과를 바로 돌려주지 않고, 결과는 [`Authority::tick`] 이 [`Event`] 로 알린다
//! (원격이면 응답이 몇 tick 뒤에 온다).

use std::time::Duration;

use nexus_core::{Entity, Vec2};

use crate::ai;
use crate::combat::SkillId;
use crate::item::{BagKind, EquipSlot, ItemStack};
use crate::script::ScriptHost;
use crate::world::SimWorld;

/// 플레이어(또는 AI)가 하려는 일. **요청일 뿐** — 받아들일지는 Authority 가 정한다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Intent {
    /// `target`(월드 m) 으로 걸어간다. 이전 이동은 취소된다.
    MoveTo { unit: Entity, target: Vec2 },
    /// 제자리에 멈춘다.
    Stop { unit: Entity },
    /// `skill` 로 `target` 유닛을 공격한다. 사거리 밖이면 거절된다 —
    /// 다가가는 것은 Intent 를 내는 쪽(입력·AI)의 일이다.
    Attack {
        unit: Entity,
        target: Entity,
        skill: SkillId,
    },
    /// 땅의 아이템을 줍는다. 줍기 사거리 안이어야 한다 — 다가가는 것은 내는 쪽의 일.
    PickUp { unit: Entity, item: Entity },
    /// 가방 한 칸을 통째로 발밑에 버린다.
    DropItem {
        unit: Entity,
        bag: BagKind,
        slot: u16,
    },
    /// 소모품 가방의 `slot` 에서 하나를 쓴다.
    UseItem { unit: Entity, slot: u16 },
    /// 장비 가방의 `slot` 을 장착한다 (그 자리의 장비와 맞바꿈).
    Equip { unit: Entity, slot: u16 },
    /// `slot` 자리의 장비를 벗는다.
    Unequip { unit: Entity, slot: EquipSlot },
}

impl Intent {
    /// Intent 를 낸 유닛.
    #[must_use]
    pub fn unit(&self) -> Entity {
        match *self {
            Self::MoveTo { unit, .. }
            | Self::Stop { unit }
            | Self::Attack { unit, .. }
            | Self::PickUp { unit, .. }
            | Self::DropItem { unit, .. }
            | Self::UseItem { unit, .. }
            | Self::Equip { unit, .. }
            | Self::Unequip { unit, .. } => unit,
        }
    }
}

/// Intent 를 거절한 이유.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Rejection {
    /// 없는 유닛 (디스폰됐거나 낡은 핸들). 공격 대상이 없을 때도.
    UnknownEntity,
    /// 목적지까지 걸어서 갈 길이 없다 (벽·레벨·맵 밖).
    NoPath,
    /// 이동 속도가 0 인 유닛.
    Immobile,
    /// Intent 를 낸 유닛이 죽어 있다.
    Dead,
    /// 공격 대상이 이미 죽었다.
    TargetDead,
    /// 등록되지 않은 스킬.
    UnknownSkill,
    /// 스킬 쿨타임 중.
    OnCooldown,
    /// 대상이 스킬 사거리 밖.
    OutOfRange,
    /// 자기 자신은 대상이 될 수 없다.
    InvalidTarget,
    /// 대상이 피해를 받지 않는다 (`UnitDef::immortal`).
    Invulnerable,
    /// 우호 진영은 공격할 수 없다.
    Friendly,
    /// 없는 땅의 아이템, 또는 정의되지 않은 아이템.
    UnknownItem,
    /// 빈 칸.
    EmptySlot,
    /// 가방에 다 들어가지 않는다. 일부만 줍지 않는다.
    InventoryFull,
    /// 그 방식으로 쓸 수 없는 아이템 (장비를 "사용" 등).
    NotUsable,
}

/// tick 동안 일어난 일.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// 최종 목적지에 도착했다.
    Arrived { unit: Entity },
    /// 이동 도중 길이 막혀 멈췄다 (경로를 잡은 뒤 타일이 바뀜).
    Blocked { unit: Entity },
    /// Intent 가 거절됐다. 상태는 바뀌지 않았다.
    Rejected { intent: Intent, reason: Rejection },
    /// 공격이 맞았다.
    Damaged {
        attacker: Entity,
        target: Entity,
        skill: SkillId,
        amount: u32,
        remaining_hp: u32,
    },
    /// 유닛이 죽었다. 월드에는 시체로 남는다 — 치울지는 스폰한 쪽이 정한다.
    Died { unit: Entity, killer: Entity },
    /// AI 가 싸울 상대를 정했다 (먼저 발견했거나 반격).
    Engaged { unit: Entity, target: Entity },
    /// AI 가 추격 한계를 넘어 포기하고 스폰 지점으로 돌아간다.
    Evading { unit: Entity },
    /// 땅에 아이템이 생겼다 (몬스터 드롭 또는 버리기).
    ItemSpawned {
        item: Entity,
        stack: ItemStack,
        pos: Vec2,
    },
    /// 땅의 아이템을 주웠다. `item` 핸들은 이제 무효다.
    PickedUp {
        unit: Entity,
        item: Entity,
        stack: ItemStack,
    },
    /// 소모품으로 HP 가 올랐다. `amount` 는 실제로 오른 양 (가득이면 0).
    Healed {
        unit: Entity,
        amount: u32,
        remaining_hp: u32,
    },
    /// 장착 상태가 바뀌었다. 공격력·방어력이 달라졌을 수 있다.
    EquipmentChanged { unit: Entity, slot: EquipSlot },
}

/// 게임플레이 상태의 권한자.
pub trait Authority {
    /// Intent 를 낸다. **다음 [`tick`](Self::tick) 에서** 처리된다.
    fn submit(&mut self, intent: Intent);

    /// 시뮬레이션을 한 tick 진행하고 그동안의 이벤트를 돌려준다.
    fn tick(&mut self, dt: Duration) -> Vec<Event>;

    /// 렌더·UI 가 읽는 현재 상태.
    fn world(&self) -> &SimWorld;
}

/// 오프라인 권한자 — Intent 를 그 자리에서 받아들인다.
#[derive(Debug)]
pub struct LocalAuthority {
    world: SimWorld,
    pending: Vec<Intent>,
    ticks: u64,
    /// 액터 스크립트 (P2). 없으면 스크립트 단계를 건너뛴다.
    scripts: Option<Box<dyn ScriptHost>>,
    /// 이전 tick 의 이벤트 — 스크립트 훅(피격·사망 등)의 재료.
    last_events: Vec<Event>,
}

impl LocalAuthority {
    #[must_use]
    pub fn new(world: SimWorld) -> Self {
        Self {
            world,
            pending: Vec::new(),
            ticks: 0,
            scripts: None,
            last_events: Vec::new(),
        }
    }

    /// 설정용 접근 — 존을 읽어 유닛을 스폰하거나 타일을 반영할 때.
    /// 게임플레이 중 상태 변경에는 쓰지 말고 [`submit`](Authority::submit) 을 쓸 것.
    pub fn world_mut(&mut self) -> &mut SimWorld {
        &mut self.world
    }

    /// 액터 스크립트 호스트를 붙인다 — 설정 작업이다 (스폰과 같은 때).
    pub fn set_script_host(&mut self, host: Box<dyn ScriptHost>) {
        self.scripts = Some(host);
    }

    /// 붙인 스크립트 호스트 — 호출한 쪽이 로그·오류를 꺼내 갈 때.
    pub fn script_host_mut(&mut self) -> Option<&mut (dyn ScriptHost + 'static)> {
        self.scripts.as_deref_mut()
    }

    /// 지금까지 진행한 tick 수.
    #[must_use]
    pub fn ticks(&self) -> u64 {
        self.ticks
    }
}

impl Authority for LocalAuthority {
    fn submit(&mut self, intent: Intent) {
        self.pending.push(intent);
    }

    fn tick(&mut self, dt: Duration) -> Vec<Event> {
        let mut events = Vec::new();
        // 낸 순서대로 처리한다 — 같은 유닛에 두 번 내면 나중 것이 이긴다.
        for intent in std::mem::take(&mut self.pending) {
            if let Err(reason) = apply(&mut self.world, intent, &mut events) {
                events.push(Event::Rejected { intent, reason });
            }
        }
        // AI 는 플레이어 Intent 가 반영된 상태를 보고 판단한다. 같은 규칙으로 판정받지만,
        // 거절은 호출한 쪽에 알리지 않는다 (호출한 쪽이 낸 Intent 가 아니므로).
        for intent in ai::think(&mut self.world, &mut events) {
            if let Err(reason) = apply(&mut self.world, intent, &mut events) {
                ai::on_rejected(&mut self.world, intent, reason);
            }
        }
        // 스크립트는 AI 뒤에 돈다 — 같은 유닛이면 나중 Intent 가 이기므로 AI 의 결정을 덮을 수 있다.
        // 거절은 AI 처럼 알리지 않는다.
        if let Some(host) = self.scripts.as_mut() {
            let mut intents = Vec::new();
            host.run(&self.world, dt, &self.last_events, &mut intents);
            for intent in intents {
                let _ = apply(&mut self.world, intent, &mut events);
            }
        }
        self.world.step(dt, &mut events);
        self.ticks += 1;
        if self.scripts.is_some() {
            self.last_events.clone_from(&events);
        }
        events
    }

    fn world(&self) -> &SimWorld {
        &self.world
    }
}

/// Intent 하나를 월드 규칙으로 판정·적용한다.
fn apply(world: &mut SimWorld, intent: Intent, events: &mut Vec<Event>) -> Result<(), Rejection> {
    match intent {
        Intent::MoveTo { unit, target } => world.plan_move(unit, target),
        Intent::Stop { unit } => world.stop(unit),
        Intent::Attack {
            unit,
            target,
            skill,
        } => world.attack(unit, target, skill, events),
        Intent::PickUp { unit, item } => world.pick_up(unit, item, events),
        Intent::DropItem { unit, bag, slot } => world.drop_item(unit, bag, slot, events),
        Intent::UseItem { unit, slot } => world.use_item(unit, slot, events),
        Intent::Equip { unit, slot } => world.equip(unit, slot, events),
        Intent::Unequip { unit, slot } => world.unequip(unit, slot, events),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Tile, TileCoord, TileMap, UnitDef};

    /// 서버 ZoneActor 와 같은 50ms.
    const DT: Duration = Duration::from_millis(50);
    const SPEED: f32 = 2.0;

    fn flat(w: u32, h: u32) -> TileMap {
        TileMap::new(w, h, 1.0, Vec2::ZERO, Tile::default())
    }

    fn wall() -> Tile {
        Tile {
            walkable: false,
            ..Tile::default()
        }
    }

    fn setup(map: TileMap, at: Vec2) -> (LocalAuthority, Entity) {
        let mut auth = LocalAuthority::new(SimWorld::new(map));
        let unit = auth.world_mut().spawn_unit(
            at,
            0.0,
            UnitDef {
                move_speed: SPEED,
                ..UnitDef::default()
            },
        );
        (auth, unit)
    }

    fn pos(auth: &LocalAuthority, unit: Entity) -> Vec2 {
        auth.world().unit(unit).unwrap().pos()
    }

    /// 도착하거나 막힐 때까지 돌리고, 매 tick 위치를 모은다.
    fn run(auth: &mut LocalAuthority, unit: Entity, max_ticks: u32) -> (Vec<Vec2>, Vec<Event>) {
        let mut trail = vec![pos(auth, unit)];
        let mut all = Vec::new();
        for _ in 0..max_ticks {
            let events = auth.tick(DT);
            trail.push(pos(auth, unit));
            let done = events
                .iter()
                .any(|e| matches!(e, Event::Arrived { .. } | Event::Blocked { .. }));
            all.extend(events);
            if done {
                break;
            }
        }
        (trail, all)
    }

    // ── 기본 이동 ────────────────────────────────────────────────────────────

    #[test]
    fn intent_is_applied_on_tick_not_on_submit() {
        let (mut auth, unit) = setup(flat(10, 10), Vec2::new(0.5, 0.5));
        auth.submit(Intent::MoveTo {
            unit,
            target: Vec2::new(8.5, 0.5),
        });
        assert!(
            !auth.world().unit(unit).unwrap().is_moving(),
            "submit 만으로 상태가 바뀌었다"
        );
        auth.tick(DT);
        assert!(auth.world().unit(unit).unwrap().is_moving());
    }

    #[test]
    fn moves_at_def_speed_and_arrives_exactly() {
        let (mut auth, unit) = setup(flat(10, 10), Vec2::new(0.5, 0.5));
        let target = Vec2::new(8.3, 0.7);
        auth.submit(Intent::MoveTo { unit, target });

        let (trail, events) = run(&mut auth, unit, 200);
        let per_tick = SPEED * DT.as_secs_f32();
        for w in trail.windows(2) {
            assert!(
                w[0].distance(w[1]) <= per_tick + 1e-4,
                "한 tick 에 속도보다 멀리 갔다: {w:?}"
            );
        }
        assert_eq!(*trail.last().unwrap(), target, "목적지에 정확히 서야 한다");
        let arrivals = events
            .iter()
            .filter(|e| **e == Event::Arrived { unit })
            .count();
        assert_eq!(arrivals, 1);
        assert!(!auth.world().unit(unit).unwrap().is_moving());
    }

    #[test]
    fn heading_follows_movement() {
        let (mut auth, unit) = setup(flat(10, 10), Vec2::new(5.5, 5.5));
        auth.submit(Intent::MoveTo {
            unit,
            target: Vec2::new(5.5, 9.5),
        });
        auth.tick(DT);
        let heading = auth.world().unit(unit).unwrap().heading();
        assert!(
            (heading - std::f32::consts::FRAC_PI_2).abs() < 1e-4,
            "북쪽(+Y)으로 가면 π/2: {heading}"
        );
    }

    #[test]
    fn large_dt_crosses_several_waypoints_without_overshoot() {
        let (mut auth, unit) = setup(flat(10, 10), Vec2::new(0.5, 0.5));
        let target = Vec2::new(6.5, 6.5);
        auth.submit(Intent::MoveTo { unit, target });
        // 창 최소화 복귀처럼 긴 dt 가 한 번에 와도 목적지를 지나치지 않는다.
        let events = auth.tick(Duration::from_secs(60));
        assert_eq!(pos(&auth, unit), target);
        assert!(events.contains(&Event::Arrived { unit }));
    }

    #[test]
    fn prev_pos_tracks_last_tick_for_interpolation() {
        let (mut auth, unit) = setup(flat(10, 10), Vec2::new(0.5, 0.5));
        auth.submit(Intent::MoveTo {
            unit,
            target: Vec2::new(9.5, 0.5),
        });
        auth.tick(DT);
        let before = pos(&auth, unit);
        auth.tick(DT);
        let u = auth.world().unit(unit).unwrap();
        assert_eq!(u.prev_pos(), before);
        assert_ne!(u.pos(), before);

        // 멈춘 유닛은 prev == pos 여야 보간이 떨리지 않는다.
        auth.submit(Intent::Stop { unit });
        auth.tick(DT);
        auth.tick(DT);
        let u = auth.world().unit(unit).unwrap();
        assert_eq!(u.prev_pos(), u.pos());
    }

    #[test]
    fn later_intent_replaces_earlier_path() {
        let (mut auth, unit) = setup(flat(10, 10), Vec2::new(5.5, 5.5));
        auth.submit(Intent::MoveTo {
            unit,
            target: Vec2::new(9.5, 5.5),
        });
        auth.submit(Intent::MoveTo {
            unit,
            target: Vec2::new(1.5, 5.5),
        });
        let (trail, _) = run(&mut auth, unit, 200);
        assert_eq!(*trail.last().unwrap(), Vec2::new(1.5, 5.5));
        assert!(
            trail.iter().all(|p| p.x <= 5.5 + 1e-4),
            "동쪽으로 간 적이 있다"
        );
    }

    #[test]
    fn stop_halts_in_place() {
        let (mut auth, unit) = setup(flat(10, 10), Vec2::new(0.5, 0.5));
        auth.submit(Intent::MoveTo {
            unit,
            target: Vec2::new(9.5, 0.5),
        });
        auth.tick(DT);
        auth.submit(Intent::Stop { unit });
        auth.tick(DT);
        let stopped = pos(&auth, unit);
        let events = auth.tick(DT);
        assert_eq!(pos(&auth, unit), stopped);
        assert!(
            events.is_empty(),
            "멈춘 뒤 도착 이벤트가 나왔다: {events:?}"
        );
    }

    // ── 타일 규칙 ────────────────────────────────────────────────────────────

    /// 궤적의 모든 연속 위치가 타일 규칙을 지켰는지 — 같은 칸이거나 한 걸음 가능한 이웃.
    fn assert_legal(map: &TileMap, trail: &[Vec2]) {
        for w in trail.windows(2) {
            let (a, b) = (map.world_to_tile(w[0]), map.world_to_tile(w[1]));
            assert!(
                map.get(b).is_some_and(|t| t.walkable),
                "못 걷는 칸 {b:?} 에 들어갔다"
            );
            assert!(a == b || map.can_step(a, b), "{a:?} → {b:?} 는 불법 걸음");
        }
    }

    #[test]
    fn walks_around_a_wall() {
        let mut map = flat(10, 10);
        for y in 0..8 {
            map.set(TileCoord::new(5, y), wall());
        }
        let (mut auth, unit) = setup(map.clone(), Vec2::new(1.5, 1.5));
        let target = Vec2::new(8.5, 1.5);
        auth.submit(Intent::MoveTo { unit, target });

        let (trail, _) = run(&mut auth, unit, 400);
        assert_eq!(*trail.last().unwrap(), target);
        assert_legal(&map, &trail);
        assert!(
            trail.iter().any(|p| p.y >= 8.0),
            "벽 끝(y=8)을 돌아가야 한다"
        );
    }

    #[test]
    fn climbs_to_high_ground_only_by_ramp() {
        // 동쪽 절반이 레벨 1, 경사로는 (5, 7) 한 칸.
        let mut map = flat(10, 10);
        for y in 0..10 {
            for x in 5..10 {
                map.set(
                    TileCoord::new(x, y),
                    Tile {
                        walkable: true,
                        level: 1,
                        ramp: x == 5 && y == 7,
                    },
                );
            }
        }
        let (mut auth, unit) = setup(map.clone(), Vec2::new(1.5, 1.5));
        let target = Vec2::new(8.5, 1.5);
        auth.submit(Intent::MoveTo { unit, target });

        let (trail, _) = run(&mut auth, unit, 400);
        assert_eq!(*trail.last().unwrap(), target);
        assert_legal(&map, &trail);
        assert!(
            trail
                .iter()
                .any(|p| map.world_to_tile(*p) == TileCoord::new(5, 7)),
            "경사로를 지나지 않았다"
        );
    }

    #[test]
    fn unreachable_target_is_rejected_and_unit_stays() {
        let mut map = flat(10, 10);
        for y in 0..10 {
            map.set(TileCoord::new(5, y), wall());
        }
        let start = Vec2::new(1.5, 1.5);
        let (mut auth, unit) = setup(map, start);
        let intent = Intent::MoveTo {
            unit,
            target: Vec2::new(8.5, 1.5),
        };
        auth.submit(intent);
        let events = auth.tick(DT);
        assert_eq!(
            events,
            vec![Event::Rejected {
                intent,
                reason: Rejection::NoPath
            }]
        );
        assert_eq!(pos(&auth, unit), start);
    }

    #[test]
    fn target_off_the_map_is_rejected() {
        let (mut auth, unit) = setup(flat(10, 10), Vec2::new(1.5, 1.5));
        auth.submit(Intent::MoveTo {
            unit,
            target: Vec2::new(-30.0, 4.0),
        });
        let events = auth.tick(DT);
        assert!(matches!(
            events[..],
            [Event::Rejected {
                reason: Rejection::NoPath,
                ..
            }]
        ));
    }

    #[test]
    fn tile_changed_mid_route_blocks_instead_of_walking_through() {
        let (mut auth, unit) = setup(flat(10, 3), Vec2::new(0.5, 1.5));
        auth.submit(Intent::MoveTo {
            unit,
            target: Vec2::new(9.5, 1.5),
        });
        auth.tick(DT);
        // 경로를 잡은 뒤 길 전체를 벽으로 막는다 (에디터에서 칠한 상황).
        for y in 0..3 {
            auth.world_mut()
                .tiles_mut()
                .set(TileCoord::new(4, y), wall());
        }
        let (trail, events) = run(&mut auth, unit, 400);
        assert!(events.contains(&Event::Blocked { unit }));
        assert!(!events.contains(&Event::Arrived { unit }));
        assert!(trail.iter().all(|p| p.x < 4.0), "벽을 통과했다");
        assert!(!auth.world().unit(unit).unwrap().is_moving());
    }

    // ── 거절 ─────────────────────────────────────────────────────────────────

    #[test]
    fn despawned_unit_is_rejected() {
        let (mut auth, unit) = setup(flat(4, 4), Vec2::new(0.5, 0.5));
        assert!(auth.world_mut().despawn(unit));
        // 같은 슬롯을 재사용한 새 유닛이 낡은 핸들의 명령을 받으면 안 된다.
        let newcomer = auth.world_mut().spawn_unit(
            Vec2::new(0.5, 0.5),
            0.0,
            UnitDef {
                move_speed: SPEED,
                ..UnitDef::default()
            },
        );
        assert_eq!(newcomer.index(), unit.index());

        auth.submit(Intent::Stop { unit });
        let events = auth.tick(DT);
        assert!(matches!(
            events[..],
            [Event::Rejected {
                reason: Rejection::UnknownEntity,
                ..
            }]
        ));
        assert!(auth.world().unit(unit).is_none());
        assert!(auth.world().unit(newcomer).is_some());
    }

    #[test]
    fn immobile_unit_rejects_move() {
        let mut auth = LocalAuthority::new(SimWorld::new(flat(4, 4)));
        let rock = auth.world_mut().spawn_unit(
            Vec2::new(0.5, 0.5),
            0.0,
            UnitDef {
                move_speed: 0.0,
                ..UnitDef::default()
            },
        );
        auth.submit(Intent::MoveTo {
            unit: rock,
            target: Vec2::new(3.5, 0.5),
        });
        let events = auth.tick(DT);
        assert!(matches!(
            events[..],
            [Event::Rejected {
                reason: Rejection::Immobile,
                ..
            }]
        ));
    }

    #[test]
    fn units_iterates_live_units_only() {
        let mut world = SimWorld::new(flat(4, 4));
        let def = UnitDef {
            move_speed: 1.0,
            ..UnitDef::default()
        };
        let a = world.spawn_unit(Vec2::ZERO, 0.0, def);
        let b = world.spawn_unit(Vec2::ONE, 0.0, def);
        world.despawn(a);
        let live: Vec<Entity> = world.units().map(|(e, _)| e).collect();
        assert_eq!(live, vec![b]);
        assert_eq!(world.unit_count(), 1);
    }
}
