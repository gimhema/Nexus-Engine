use std::time::Duration;

use nexus_core::{Entity, Vec2};
use nexus_sim::{
    AiKind, Authority, Event, FactionId, LocalAuthority, Relation, ScriptHost, SimWorld, SkillDef,
    SkillId, Tile, TileMap, UnitDef,
};

use super::{RhaiHost, ScriptId};

const DT: Duration = Duration::from_millis(50);
const BITE: SkillId = SkillId(1);
const HEROES: FactionId = FactionId(1);
const GOBLINS: FactionId = FactionId(2);

fn fighter(faction: FactionId) -> UnitDef {
    UnitDef {
        move_speed: 3.0,
        max_hp: 100,
        attack: 20,
        defense: 0,
        faction,
        ai: AiKind::Passive,
        basic_attack: Some(BITE),
        ..UnitDef::default()
    }
}

/// 40×40m 평지, 스크립트 붙은 고블린 하나(원점)와 영웅 하나(`hero_at`).
fn arena(source: &str, seed: u64, hero_at: Vec2) -> (LocalAuthority, Entity, Entity) {
    let mut world = SimWorld::new(TileMap::new(
        40,
        40,
        1.0,
        Vec2::new(-20.0, -20.0),
        Tile::default(),
    ));
    world.define_skill(
        BITE,
        SkillDef {
            range: 1.5,
            cooldown_ms: 1000,
            damage_mult: 1.0,
        },
    );
    world.set_relation(HEROES, GOBLINS, Relation::Hostile);
    let goblin = world.spawn_unit(Vec2::ZERO, 0.0, fighter(GOBLINS));
    let hero = world.spawn_unit(hero_at, 0.0, fighter(HEROES));

    let mut host = RhaiHost::new(seed);
    let id = host.add_script("goblin.rhai", source).unwrap();
    host.attach(goblin, id);
    let mut auth = LocalAuthority::new(world);
    auth.set_script_host(Box::new(host));
    (auth, goblin, hero)
}

fn run(auth: &mut LocalAuthority, ticks: usize) -> Vec<Event> {
    (0..ticks).flat_map(|_| auth.tick(DT)).collect()
}

fn log(auth: &mut LocalAuthority) -> Vec<(bool, String)> {
    auth.script_host_mut()
        .unwrap()
        .drain_log()
        .into_iter()
        .map(|l| (l.error, l.message))
        .collect()
}

fn pos(auth: &LocalAuthority, e: Entity) -> Vec2 {
    auth.world().unit(e).unwrap().pos()
}

#[test]
fn spawn_runs_once_before_tick_and_state_survives_between_ticks() {
    let src = r#"
        fn on_spawn(me) { this.n = 0; print("spawn"); }
        fn on_tick(me, dt) {
            this.n += 1;
            if this.n == 3 { print(`tick ${this.n} dt ${dt}`); }
        }
    "#;
    let (mut auth, _, _) = arena(src, 1, Vec2::new(15.0, 0.0));
    run(&mut auth, 5);
    let lines: Vec<String> = log(&mut auth).into_iter().map(|(_, m)| m).collect();
    assert_eq!(
        lines,
        ["goblin.rhai: spawn", "goblin.rhai: tick 3 dt 0.05"],
        "on_spawn 은 한 번, this 는 tick 사이에 남는다"
    );
}

#[test]
fn a_goblin_script_looks_around_every_three_seconds_and_charges() {
    // P2 의 끝나는 조건 — "3초마다 주변을 둘러보고 적이 있으면 돌진".
    let src = r#"
        fn on_spawn(me) { this.timer = 0.0; this.target = (); }
        fn on_tick(me, dt) {
            this.timer += dt;
            if this.timer >= 3.0 {
                this.timer = 0.0;
                this.target = nearest_enemy(me, 10.0);
            }
            if this.target == () { return; }
            if distance(me, this.target) <= attack_range(me) {
                attack(me, this.target);
            } else {
                move_to(me, this.target);
            }
        }
    "#;
    let (mut auth, goblin, hero) = arena(src, 1, Vec2::new(8.0, 0.0));

    // 3초(60 tick) 전에는 둘러보지 않았으므로 제자리다.
    run(&mut auth, 59);
    assert_eq!(pos(&auth, goblin), Vec2::ZERO);

    // 둘러본 뒤로는 달려가 문다 — 판정은 AI 와 같은 규칙(사거리·쿨타임)이다.
    let events = run(&mut auth, 80);
    assert!(pos(&auth, goblin).distance(pos(&auth, hero)) <= 1.5);
    let bites = events
        .iter()
        .filter(|e| matches!(e, Event::Damaged { attacker, .. } if *attacker == goblin))
        .count();
    assert!(bites >= 2, "쿨타임(1초)마다 물어야 한다: {bites}");
    assert!(log(&mut auth).is_empty());
}

#[test]
fn nothing_in_range_means_nothing_found() {
    let src = r#"
        fn on_tick(me, dt) {
            if nearest_enemy(me, 5.0) == () { print("없음"); } else { print("있음"); }
            print(`${enemies(me, 100.0).len()}`);
        }
    "#;
    let (mut auth, _, _) = arena(src, 1, Vec2::new(8.0, 0.0));
    run(&mut auth, 1);
    let lines: Vec<String> = log(&mut auth).into_iter().map(|(_, m)| m).collect();
    assert_eq!(lines, ["goblin.rhai: 없음", "goblin.rhai: 1"]);
}

#[test]
fn damage_and_death_hooks_fire_for_the_right_unit() {
    let src = r#"
        fn on_attack(me, target, amount) { print(`때림 ${amount}`); }
        fn on_damaged(me, attacker, amount) { print(`맞음 ${amount} 남은 ${me.hp}`); }
        fn on_death(me, killer) { print("죽음"); }
    "#;
    let (mut auth, goblin, hero) = arena(src, 1, Vec2::new(1.0, 0.0));
    // 영웅이 고블린을 다섯 번 문다 (20 × 5 = 100).
    for _ in 0..5 {
        auth.submit(nexus_sim::Intent::Attack {
            unit: hero,
            target: goblin,
            skill: BITE,
        });
        run(&mut auth, 20);
    }
    let lines: Vec<String> = log(&mut auth).into_iter().map(|(_, m)| m).collect();
    assert_eq!(
        lines.first().map(String::as_str),
        Some("goblin.rhai: 맞음 20 남은 80")
    );
    assert_eq!(
        lines.iter().filter(|l| l.contains("맞음")).count(),
        4,
        "{lines:?}"
    );
    assert_eq!(lines.last().map(String::as_str), Some("goblin.rhai: 죽음"));
    assert!(!lines.iter().any(|l| l.contains("때림")));
}

#[test]
fn a_script_cannot_drive_another_unit_and_is_switched_off_once() {
    let src = r#"
        fn on_tick(me, dt) {
            let other = nearest_enemy(me, 100.0);
            move_to(other, 0.0, 0.0);
        }
    "#;
    let (mut auth, _, hero) = arena(src, 1, Vec2::new(5.0, 0.0));
    run(&mut auth, 10);
    let lines = log(&mut auth);
    assert_eq!(lines.len(), 1, "오류는 한 번만: {lines:?}");
    assert!(lines[0].0 && lines[0].1.contains("자기 유닛"), "{lines:?}");
    assert_eq!(pos(&auth, hero), Vec2::new(5.0, 0.0));
}

#[test]
fn an_endless_loop_is_cut_off_and_the_game_keeps_running() {
    let src = "fn on_tick(me, dt) { loop { } }";
    let (mut auth, _, hero) = arena(src, 1, Vec2::new(5.0, 0.0));
    auth.submit(nexus_sim::Intent::MoveTo {
        unit: hero,
        target: Vec2::new(10.0, 0.0),
    });
    run(&mut auth, 60);
    let lines = log(&mut auth);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].1.contains("실행 스텝 상한"), "{lines:?}");
    assert_eq!(pos(&auth, hero), Vec2::new(10.0, 0.0), "게임은 계속 돈다");
}

#[test]
fn same_seed_same_wandering_different_seed_different() {
    let src = r#"
        fn on_tick(me, dt) {
            if !me.moving { move_to(me, rand_float() * 20.0 - 10.0, (rand(20) - 10).to_float()); }
        }
    "#;
    let trail = |seed| {
        let (mut auth, goblin, _) = arena(src, seed, Vec2::new(19.0, 19.0));
        (0..200)
            .map(|_| {
                auth.tick(DT);
                pos(&auth, goblin)
            })
            .collect::<Vec<_>>()
            .into_iter()
            .chain(std::iter::once(Vec2::splat(log(&mut auth).len() as f32)))
            .collect::<Vec<_>>()
    };
    let a = trail(7);
    assert_eq!(
        *a.last().unwrap(),
        Vec2::ZERO,
        "스크립트 오류가 없어야 한다"
    );
    assert!(a[..200].iter().any(|p| *p != Vec2::ZERO), "움직여야 한다");
    assert_eq!(a, trail(7));
    assert_ne!(trail(7), trail(8));
}

#[test]
fn korean_identifiers_are_allowed() {
    let src = r#"
        fn on_spawn(me) { this.걸음 = 0; }
        fn on_tick(me, dt) {
            let 목표 = 3;
            this.걸음 += 1;
            if this.걸음 == 목표 { print("세 번째"); }
        }
    "#;
    let (mut auth, _, _) = arena(src, 1, Vec2::new(15.0, 0.0));
    run(&mut auth, 3);
    assert_eq!(
        log(&mut auth),
        [(false, String::from("goblin.rhai: 세 번째"))]
    );
}

#[test]
fn compile_errors_name_the_script_and_the_problem() {
    let mut host = RhaiHost::new(0);
    let err = host
        .add_script("bad.rhai", "fn on_tick(me, dt) { let = 1; }")
        .unwrap_err();
    assert!(err.starts_with("bad.rhai:"), "{err}");
    assert!(err.contains("line 1"), "줄 번호가 있어야 한다: {err}");

    let err = host
        .add_script("arity.rhai", "fn on_tick(me) { }")
        .unwrap_err();
    assert!(err.contains("인자는 2개"), "{err}");

    // 훅이 없어도 된다 — 아무것도 하지 않는 스크립트.
    assert_eq!(
        host.add_script("empty.rhai", "fn helper() { 1 }"),
        Ok(ScriptId(0))
    );
}

#[test]
fn no_wall_clock_is_available() {
    let mut host = RhaiHost::new(0);
    let id = host
        .add_script("clock.rhai", "fn on_tick(me, dt) { timestamp(); }")
        .unwrap();
    let mut world = SimWorld::new(TileMap::new(4, 4, 1.0, Vec2::ZERO, Tile::default()));
    let e = world.spawn_unit(Vec2::ONE, 0.0, UnitDef::default());
    host.attach(e, id);
    let mut out = Vec::new();
    host.run(&world, DT, &[], &mut out);
    let lines = host.drain_log();
    assert!(
        lines[0].error && lines[0].message.contains("timestamp"),
        "{lines:?}"
    );
}
