//! 전투 AI — 매 tick 유닛마다 **Intent 를 만든다.**
//!
//! AI 는 월드를 직접 고치지 않는다. 플레이어 입력과 같은 [`Intent`] 를 내고, 같은 규칙
//! (사거리·쿨타임·진영)으로 판정받는다. 그래서 AI 가 할 수 있는 일은 플레이어도 할 수 있고,
//! 규칙이 두 벌로 갈라지지 않는다.
//!
//! AI 는 **권한자 쪽**에서 돈다 — 단계 2 에서는 서버가 몬스터 AI 를 돌리고 클라이언트는
//! 결과만 받는다. 그래서 [`LocalAuthority`](crate::LocalAuthority) 가 tick 안에서 부른다.
//!
//! # 행동 (서버 `EAIType` 의 정의를 따른다)
//!
//! | | 먼저 덤빔 | 반격 |
//! |---|---|---|
//! | `Passive` | ✗ | ✗ |
//! | `Defensive` | ✗ | ✓ |
//! | `Aggressive` | 어그로 범위 안의 **적대** 진영, 보이는(레벨 규칙) 것 중 가장 가까운 것 | ✓ |
//!
//! 상대가 있으면: 기본 공격 사거리 안이면 멈춰서 친다(쿨타임이면 기다린다),
//! 밖이면 쫓아간다. **스폰 지점에서 `leash_range` 를 넘으면 포기하고 돌아간다** —
//! 돌아가는 동안은 반격도 하지 않는다 (WoW 의 evade).

use std::time::Duration;

use nexus_core::Entity;

use crate::authority::{Event, Intent, Rejection};
use crate::faction::Relation;
use crate::unit::{AiKind, AiState};
use crate::world::SimWorld;

/// 쫓는 대상이 마지막 경로 지점에서 이만큼(m) 벗어나야 경로를 다시 잡는다.
/// 매 tick A* 를 돌지 않기 위한 엔진 조율값이다 (게임 수치가 아니다).
const REPATH_DISTANCE: f32 = 0.5;

/// 갈 수 없는 적을 포기한 뒤 새 적을 찾기까지 기다리는 시간.
/// 같은 적을 매 tick 다시 쫓아 A* 를 반복하지 않게 한다.
const REACQUIRE_DELAY: Duration = Duration::from_secs(1);

/// 이번 tick 의 AI Intent 를 만든다. AI 상태(상대·귀환)도 여기서 갱신한다.
pub(crate) fn think(world: &mut SimWorld, events: &mut Vec<Event>) -> Vec<Intent> {
    let thinkers: Vec<Entity> = world
        .units()
        .filter(|(_, u)| u.is_alive() && (u.def().ai != AiKind::Passive || u.ai.target.is_some()))
        .map(|(e, _)| e)
        .collect();

    let mut intents = Vec::new();
    for unit in thinkers {
        think_one(world, unit, events, &mut intents);
    }
    intents
}

fn think_one(world: &mut SimWorld, me: Entity, events: &mut Vec<Event>, out: &mut Vec<Intent>) {
    let now = world.now();
    let Some(u) = world.unit(me) else { return };
    let (def, pos, home, moving, mut ai) = (u.def(), u.pos(), u.home(), u.is_moving(), u.ai);

    // ── 귀환 중: 도착할 때까지 아무것도 받지 않는다 ──────────────────────────
    if ai.returning {
        if moving {
            return;
        }
        ai.returning = false;
    }

    // ── 상대가 아직 유효한가 ─────────────────────────────────────────────────
    let mut target = ai.target.filter(|&t| {
        world.unit(t).is_some_and(|tu| {
            tu.is_alive() && world.relation(def.faction, tu.def().faction) != Relation::Friendly
        })
    });

    // ── 추격 한계 ────────────────────────────────────────────────────────────
    if target.is_some() && pos.distance(home) > def.leash_range {
        ai.target = None;
        ai.chase_goal = None;
        ai.returning = true;
        set_ai(world, me, ai);
        events.push(Event::Evading { unit: me });
        out.push(Intent::MoveTo {
            unit: me,
            target: home,
        });
        return;
    }

    // ── 새 상대 찾기 (공격형만) ──────────────────────────────────────────────
    if target.is_none() && def.ai == AiKind::Aggressive && now >= ai.acquire_after {
        target = nearest_hostile(world, me);
    }
    if let Some(found) = target.filter(|&t| Some(t) != ai.target) {
        events.push(Event::Engaged {
            unit: me,
            target: found,
        });
    }
    ai.target = target;

    let Some(target) = target else {
        // 싸움이 끝났다. 쫓던 중이면 그 자리에 선다.
        if ai.chase_goal.take().is_some() {
            out.push(Intent::Stop { unit: me });
        }
        set_ai(world, me, ai);
        return;
    };

    // ── 치거나 쫓는다 ───────────────────────────────────────────────────────
    let skill = def
        .basic_attack
        .and_then(|id| world.skill(id).map(|s| (id, *s)));
    let target_pos = world.unit(target).map_or(pos, |t| t.pos());
    let in_range = skill.is_some_and(|(_, s)| pos.distance(target_pos) <= s.range);

    if let Some((skill_id, _)) = skill.filter(|_| in_range) {
        if moving {
            out.push(Intent::Stop { unit: me });
        }
        ai.chase_goal = None;
        let ready = world.unit(me).is_some_and(|u| u.is_ready(skill_id, now));
        if ready {
            out.push(Intent::Attack {
                unit: me,
                target,
                skill: skill_id,
            });
        }
    } else if skill.is_some() && def.move_speed > 0.0 {
        let stale = ai
            .chase_goal
            .is_none_or(|g| g.distance(target_pos) > REPATH_DISTANCE);
        if stale || !moving {
            ai.chase_goal = Some(target_pos);
            out.push(Intent::MoveTo {
                unit: me,
                target: target_pos,
            });
        }
    }
    set_ai(world, me, ai);
}

/// 어그로 범위 안에서 보이는 가장 가까운 적대 유닛. 거리가 같으면 슬롯 순서가 앞선 것.
fn nearest_hostile(world: &SimWorld, me: Entity) -> Option<Entity> {
    let u = world.unit(me)?;
    let (pos, def) = (u.pos(), u.def());
    let tiles = world.tiles();
    let eye = tiles.world_to_tile(pos);

    let mut best: Option<(f32, Entity)> = None;
    for (other, o) in world.units() {
        if other == me || !o.is_alive() {
            continue;
        }
        if world.relation(def.faction, o.def().faction) != Relation::Hostile {
            continue;
        }
        let d = pos.distance(o.pos());
        if d > def.aggro_range || !tiles.can_see(eye, tiles.world_to_tile(o.pos())) {
            continue;
        }
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, other));
        }
    }
    best.map(|(_, e)| e)
}

fn set_ai(world: &mut SimWorld, me: Entity, ai: AiState) {
    if let Some(u) = world.unit_mut(me) {
        u.ai = ai;
    }
}

/// AI 가 낸 Intent 가 거절됐을 때. 호출한 쪽에는 알리지 않는다 — 플레이어가 낸 것이 아니므로.
///
/// 길이 없어 쫓을 수 없으면 상대를 버리고 잠시 새 적을 찾지 않는다.
pub(crate) fn on_rejected(world: &mut SimWorld, intent: Intent, reason: Rejection) {
    if !matches!((intent, reason), (Intent::MoveTo { .. }, Rejection::NoPath)) {
        return;
    }
    let now = world.now();
    if let Some(u) = world.unit_mut(intent.unit()) {
        u.ai.target = None;
        u.ai.chase_goal = None;
        u.ai.returning = false;
        u.ai.acquire_after = now + REACQUIRE_DELAY;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use nexus_core::{Entity, Vec2};

    use crate::{
        AiKind, Authority, Event, FactionId, Intent, LocalAuthority, Rejection, Relation, SimWorld,
        SkillDef, SkillId, Tile, TileCoord, TileMap, Unit, UnitDef,
    };

    const DT: Duration = Duration::from_millis(50);
    const PLAYERS: FactionId = FactionId(1);
    const WILD: FactionId = FactionId(201);
    const TOWN: FactionId = FactionId(100);

    const BITE: SkillId = SkillId(10);
    const SWORD: SkillId = SkillId(11);
    const ARROW: SkillId = SkillId(12);

    fn slime() -> UnitDef {
        UnitDef {
            move_speed: 1.5,
            max_hp: 50,
            attack: 12,
            defense: 2,
            faction: WILD,
            ai: AiKind::Aggressive,
            aggro_range: 5.0,
            leash_range: 12.0,
            basic_attack: Some(BITE),
            ..UnitDef::default()
        }
    }

    fn player() -> UnitDef {
        UnitDef {
            move_speed: 2.5,
            max_hp: 100,
            attack: 20,
            defense: 5,
            faction: PLAYERS,
            basic_attack: Some(SWORD),
            ..UnitDef::default()
        }
    }

    fn world_with(map: TileMap) -> SimWorld {
        let mut w = SimWorld::new(map);
        w.set_relation(PLAYERS, WILD, Relation::Hostile);
        let skill = |range, cooldown_ms| SkillDef {
            range,
            cooldown_ms,
            damage_mult: 1.0,
        };
        w.define_skill(BITE, skill(1.5, 1000));
        w.define_skill(SWORD, skill(2.0, 800));
        w.define_skill(ARROW, skill(40.0, 500));
        w
    }

    fn flat() -> TileMap {
        TileMap::new(40, 30, 1.0, Vec2::ZERO, Tile::default())
    }

    fn unit(auth: &LocalAuthority, e: Entity) -> &Unit {
        auth.world().unit(e).unwrap()
    }

    fn run(auth: &mut LocalAuthority, ticks: u32) -> Vec<Event> {
        (0..ticks).flat_map(|_| auth.tick(DT)).collect()
    }

    fn count(events: &[Event], f: impl Fn(&Event) -> bool) -> usize {
        events.iter().filter(|e| f(e)).count()
    }

    // ── 공격형 ───────────────────────────────────────────────────────────────

    #[test]
    fn aggressive_engages_chases_and_bites_a_hostile_in_range() {
        let mut w = world_with(flat());
        let s = w.spawn_unit(Vec2::new(10.5, 10.5), 0.0, slime());
        let p = w.spawn_unit(Vec2::new(14.5, 10.5), 0.0, player());
        let mut auth = LocalAuthority::new(w);

        let events = run(&mut auth, 100);
        assert_eq!(
            events.first(),
            Some(&Event::Engaged { unit: s, target: p }),
            "첫 tick 에 발견해야 한다"
        );
        assert_eq!(count(&events, |e| matches!(e, Event::Engaged { .. })), 1);
        let bites = count(
            &events,
            |e| matches!(e, Event::Damaged { attacker, .. } if *attacker == s),
        );
        // 다가가는 데 약 1.7초, 이후 1초 쿨타임 → 5초 동안 3~4번.
        assert!((3..=4).contains(&bites), "물기 {bites}회");
        assert!(unit(&auth, s).pos().distance(unit(&auth, p).pos()) <= 1.5);
        assert!(!unit(&auth, s).is_moving(), "사거리 안에서는 멈춰서 친다");
        assert!(
            !events.iter().any(|e| matches!(e, Event::Rejected { .. })),
            "AI 가 거절될 Intent 를 냈거나, 거절이 호출자에게 새어 나왔다"
        );
    }

    #[test]
    fn aggressive_ignores_non_hostile_and_far_units() {
        let mut w = world_with(flat());
        w.spawn_unit(Vec2::new(10.5, 10.5), 0.0, slime());
        // 중립(관계 미등록), 우호(같은 진영), 적대지만 어그로 밖.
        let neutral = UnitDef {
            faction: TOWN,
            ..player()
        };
        let kin = UnitDef {
            ai: AiKind::Passive,
            ..slime()
        };
        w.spawn_unit(Vec2::new(12.5, 10.5), 0.0, neutral);
        w.spawn_unit(Vec2::new(10.5, 12.5), 0.0, kin);
        w.spawn_unit(Vec2::new(16.5, 10.5), 0.0, player());
        let mut auth = LocalAuthority::new(w);
        let events = run(&mut auth, 40);
        assert!(events.is_empty(), "{events:?}");
    }

    #[test]
    fn aggressive_picks_the_nearest_hostile() {
        let mut w = world_with(flat());
        let s = w.spawn_unit(Vec2::new(10.5, 10.5), 0.0, slime());
        w.spawn_unit(Vec2::new(14.5, 10.5), 0.0, player());
        let near = w.spawn_unit(Vec2::new(10.5, 8.0), 0.0, player());
        let mut auth = LocalAuthority::new(w);
        auth.tick(DT);
        assert_eq!(unit(&auth, s).target(), Some(near));
    }

    #[test]
    fn low_ground_cannot_see_high_ground_but_high_can_see_low() {
        // x >= 20 이 레벨 1 고지대.
        let mut map = flat();
        let high = Tile {
            walkable: true,
            level: 1,
            ramp: false,
        };
        for y in 0..30 {
            for x in 20..40 {
                map.set(TileCoord::new(x, y), high);
            }
        }
        let mut w = world_with(map.clone());
        let below = w.spawn_unit(Vec2::new(18.5, 10.5), 0.0, slime());
        w.spawn_unit(Vec2::new(21.5, 10.5), 0.0, player());
        let mut auth = LocalAuthority::new(w);
        auth.tick(DT);
        assert_eq!(
            unit(&auth, below).target(),
            None,
            "낮은 곳에서 높은 곳이 보였다"
        );

        // 고지대의 원거리 유닛은 아래를 보고 쏜다. (경사로가 없어 근접이면 내려갈 길이
        // 없어 곧바로 포기한다 — 그건 `unreachable_target_...` 이 다룬다.)
        let mut w = world_with(map);
        let archer = UnitDef {
            basic_attack: Some(ARROW),
            ..slime()
        };
        let above = w.spawn_unit(Vec2::new(21.5, 10.5), 0.0, archer);
        let p = w.spawn_unit(Vec2::new(18.5, 10.5), 0.0, player());
        let mut auth = LocalAuthority::new(w);
        let events = auth.tick(DT);
        assert!(events.contains(&Event::Engaged {
            unit: above,
            target: p
        }));
        let shot = events.iter().any(
            |e| matches!(e, Event::Damaged { attacker, target, .. } if *attacker == above && *target == p),
        );
        assert!(shot, "높은 곳에서 아래를 쏘지 못했다: {events:?}");
    }

    // ── 반격 ─────────────────────────────────────────────────────────────────

    #[test]
    fn defensive_does_not_start_but_strikes_back() {
        let mut w = world_with(flat());
        let guard = UnitDef {
            faction: TOWN,
            ai: AiKind::Defensive,
            aggro_range: 10.0,
            ..slime()
        };
        let g = w.spawn_unit(Vec2::new(10.5, 10.5), 0.0, guard);
        let p = w.spawn_unit(Vec2::new(12.0, 10.5), 0.0, player());
        let mut auth = LocalAuthority::new(w);
        assert!(run(&mut auth, 20).is_empty(), "방어형이 먼저 덤볐다");

        auth.submit(Intent::Attack {
            unit: p,
            target: g,
            skill: SWORD,
        });
        let events = run(&mut auth, 30);
        assert!(events.contains(&Event::Engaged { unit: g, target: p }));
        let struck_back = events.iter().any(|e| {
            matches!(e, Event::Damaged { attacker, target, .. } if *attacker == g && *target == p)
        });
        assert!(struck_back, "반격하지 않았다");
    }

    #[test]
    fn passive_never_strikes_back() {
        let mut w = world_with(flat());
        let rabbit_def = UnitDef {
            ai: AiKind::Passive,
            ..slime()
        };
        let rabbit = w.spawn_unit(Vec2::new(10.5, 10.5), 0.0, rabbit_def);
        let p = w.spawn_unit(Vec2::new(12.0, 10.5), 0.0, player());
        let mut auth = LocalAuthority::new(w);
        auth.submit(Intent::Attack {
            unit: p,
            target: rabbit,
            skill: SWORD,
        });
        let events = run(&mut auth, 40);
        assert_eq!(count(&events, |e| matches!(e, Event::Damaged { .. })), 1);
        assert_eq!(unit(&auth, rabbit).target(), None);
    }

    #[test]
    fn friendly_units_cannot_be_attacked() {
        let mut w = world_with(flat());
        let a = w.spawn_unit(Vec2::new(10.5, 10.5), 0.0, player());
        let b = w.spawn_unit(Vec2::new(11.5, 10.5), 0.0, player());
        let mut auth = LocalAuthority::new(w);
        auth.submit(Intent::Attack {
            unit: a,
            target: b,
            skill: SWORD,
        });
        let events = auth.tick(DT);
        assert!(matches!(
            events[..],
            [Event::Rejected {
                reason: Rejection::Friendly,
                ..
            }]
        ));
    }

    // ── 추격 한계 · 포기 ─────────────────────────────────────────────────────

    #[test]
    fn leash_sends_it_home_and_it_ignores_hits_on_the_way() {
        let mut w = world_with(flat());
        let home = Vec2::new(10.5, 10.5);
        let s = w.spawn_unit(home, 0.0, slime());
        let p = w.spawn_unit(Vec2::new(14.0, 10.5), 0.0, player());
        let mut auth = LocalAuthority::new(w);

        // 플레이어가 슬라임보다 빠르게 동쪽으로 달아난다.
        auth.submit(Intent::MoveTo {
            unit: p,
            target: Vec2::new(38.5, 10.5),
        });
        let mut evaded = false;
        for _ in 0..400 {
            if auth.tick(DT).contains(&Event::Evading { unit: s }) {
                evaded = true;
                break;
            }
        }
        assert!(evaded, "추격을 포기하지 않았다");
        assert!(unit(&auth, s).pos().distance(home) > 12.0 - 0.1);
        assert!(unit(&auth, s).is_returning());

        // 돌아가는 중에 맞아도 다시 싸우지 않는다.
        auth.submit(Intent::Attack {
            unit: p,
            target: s,
            skill: ARROW,
        });
        let events = run(&mut auth, 400);
        assert!(
            !events.iter().any(|e| matches!(e, Event::Engaged { .. })),
            "귀환 중에 싸움을 받았다: {events:?}"
        );
        assert_eq!(unit(&auth, s).pos(), home);
        assert!(!unit(&auth, s).is_returning());
        assert_eq!(unit(&auth, s).target(), None);
    }

    #[test]
    fn unreachable_target_is_given_up_without_retrying_every_tick() {
        // 플레이어를 벽으로 둘러싼다 (시야는 레벨만 보므로 보이긴 한다).
        let mut map = flat();
        let wall = Tile {
            walkable: false,
            ..Tile::default()
        };
        for x in 13..=16 {
            for y in 8..=12 {
                if x == 13 || x == 16 || y == 8 || y == 12 {
                    map.set(TileCoord::new(x, y), wall);
                }
            }
        }
        let mut w = world_with(map);
        let start = Vec2::new(10.5, 10.5);
        let s = w.spawn_unit(start, 0.0, slime());
        w.spawn_unit(Vec2::new(14.5, 10.5), 0.0, player());
        let mut auth = LocalAuthority::new(w);

        // 30 tick = 1.5초. 처음 한 번 + 1초 뒤 한 번만 다시 시도한다.
        let events = run(&mut auth, 30);
        assert_eq!(count(&events, |e| matches!(e, Event::Engaged { .. })), 2);
        assert!(!events.iter().any(|e| matches!(e, Event::Rejected { .. })));
        assert_eq!(unit(&auth, s).pos(), start);
    }

    // ── 싸움의 끝 ────────────────────────────────────────────────────────────

    #[test]
    fn it_stops_when_the_target_dies() {
        let mut w = world_with(flat());
        let killer = UnitDef {
            attack: 1000,
            ..slime()
        };
        let s = w.spawn_unit(Vec2::new(10.5, 10.5), 0.0, killer);
        let p = w.spawn_unit(Vec2::new(14.5, 10.5), 0.0, player());
        let mut auth = LocalAuthority::new(w);
        let events = run(&mut auth, 200);
        let deaths = count(
            &events,
            |e| matches!(e, Event::Died { unit, .. } if *unit == p),
        );
        assert_eq!(deaths, 1);
        assert_eq!(count(&events, |e| matches!(e, Event::Damaged { .. })), 1);
        assert_eq!(unit(&auth, s).target(), None);
        assert!(!unit(&auth, s).is_moving());
    }

    #[test]
    fn a_dead_ai_does_nothing() {
        let mut w = world_with(flat());
        let s = w.spawn_unit(Vec2::new(10.5, 10.5), 0.0, slime());
        let hero = UnitDef {
            attack: 1000,
            ..player()
        };
        let p = w.spawn_unit(Vec2::new(14.5, 10.5), 0.0, hero);
        let mut auth = LocalAuthority::new(w);
        auth.tick(DT); // 슬라임이 발견하고 쫓기 시작
        auth.submit(Intent::Attack {
            unit: p,
            target: s,
            skill: ARROW,
        });
        auth.tick(DT);
        assert!(!unit(&auth, s).is_alive());
        let at = unit(&auth, s).pos();
        let events = run(&mut auth, 40);
        assert!(events.is_empty(), "{events:?}");
        assert_eq!(unit(&auth, s).pos(), at);
    }
}
