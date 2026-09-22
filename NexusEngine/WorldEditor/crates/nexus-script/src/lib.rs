//! 액터 스크립트 — Rhai 로 구현한 [`ScriptHost`] (단계 2 P2).
//!
//! `nexus-sim` 은 스크립트 언어를 모르고 훅 트레이트만 갖는다. 이 크레이트가 그 트레이트를
//! Rhai 로 구현한다. **파일을 읽지 않는다** — 스크립트 본문은 호출한 쪽(앱)이 문자열로 넘긴다.
//!
//! # 스크립트가 할 수 있는 것
//!
//! 스크립트는 **함수만 정의한다.** 맨 위의 문장은 실행하지 않는다. 엔진이 이름으로 찾아 부르는
//! 훅은 다섯 개이고, 없는 훅은 부르지 않는다.
//!
//! ```text
//! fn on_spawn(me)                   처음 한 번 — 상태를 여기서 초기화한다
//! fn on_tick(me, dt)                매 tick (dt = 초, 20Hz 면 0.05)
//! fn on_attack(me, target, amount)  내 공격이 맞았다
//! fn on_damaged(me, attacker, amount) 내가 맞았다
//! fn on_death(me, killer)           내가 죽었다
//! ```
//!
//! **상태는 `this` 에 둔다** — 액터마다 따로인 맵이다 (`this.timer = 0.0;`).
//! Rhai 함수는 바깥 변수를 볼 수 없으므로 이것이 상태를 남기는 유일한 방법이다.
//!
//! 월드는 **읽기만** 한다. 하고 싶은 일은 [`Intent`] 로 나간다 — AI 와 같은 판정(사거리·쿨타임·
//! 진영)을 받는다. 자기 유닛만 조종할 수 있다.
//!
//! | 읽기 | |
//! |---|---|
//! | `u.x` `u.y` `u.hp` `u.max_hp` `u.alive` `u.moving` `u.id` | 유닛 속성 |
//! | `distance(a, b)` | 지면 거리 (m) |
//! | `nearest_enemy(me, 거리)` | 가장 가까운 **보이는 적대** 유닛, 없으면 `()` |
//! | `enemies(me, 거리)` | 그 목록 (가까운 순) |
//! | `can_see(me, u)` | 높이 레벨 시야 규칙 |
//! | `attack_range(me)` | 기본 공격 사거리 (없으면 0) |
//! | `time()` | 시뮬레이션 시각 (초) |
//! | `rand(n)` / `rand_float()` | 시드 있는 난수 — `[0, n)` / `[0, 1)` |
//!
//! | 행동 (Intent) | |
//! |---|---|
//! | `move_to(me, x, y)` / `move_to(me, u)` | 걸어가기 |
//! | `stop(me)` | 멈추기 |
//! | `attack(me, u)` / `attack(me, u, 스킬번호)` | 공격 (기본 공격 / 지정 스킬) |
//!
//! `print(...)` 는 호출한 쪽의 로그로 간다.
//!
//! # 안전·결정성
//!
//! - 훅 한 번의 실행 스텝에 상한이 있다 ([`MAX_OPERATIONS`]) — 무한 루프는 오류로 끊긴다.
//! - 오류가 나면 **그 스크립트만 끈다** (게임은 계속된다). 오류는 한 번만 알린다.
//! - 벽시계가 없고(`no_time`), 난수는 시드 있는 것뿐이다. 훅은 월드의 유닛 순서대로 돈다.
//!   같은 시드 = 같은 결과.

#![forbid(unsafe_code)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use nexus_core::{Entity, Vec2};
use nexus_sim::{Event, FactionId, Intent, Relation, ScriptHost, ScriptLog, SimWorld, SkillId};
use rhai::{AST, Array, CallFnOptions, Dynamic, Engine, EvalAltResult, FLOAT, INT, Map, Scope};

/// 훅 한 번이 쓸 수 있는 실행 스텝 수. 보통의 훅은 수백 스텝이다.
pub const MAX_OPERATIONS: u64 = 50_000;

/// 스크립트 번호 — [`RhaiHost::add_script`] 가 돌려준다.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ScriptId(usize);

/// 훅 이름과 인자 수. 인자 수가 다르면 컴파일할 때 알려 준다.
const HOOKS: [(&str, usize); 5] = [
    ("on_spawn", 1),
    ("on_tick", 2),
    ("on_attack", 3),
    ("on_damaged", 3),
    ("on_death", 2),
];

/// Rhai 로 액터 스크립트를 돌린다.
pub struct RhaiHost {
    engine: Engine,
    scripts: Vec<Script>,
    /// 스크립트가 붙은 유닛. 조회만 한다 — 순서는 항상 월드의 유닛 순서를 따른다.
    attached: HashMap<Entity, Attached>,
    ctx: Rc<RefCell<Ctx>>,
}

impl std::fmt::Debug for RhaiHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RhaiHost")
            .field("scripts", &self.scripts.len())
            .field("attached", &self.attached.len())
            .finish_non_exhaustive()
    }
}

struct Script {
    name: String,
    ast: AST,
    /// 정의된 훅 — [`HOOKS`] 순서.
    has: [bool; 5],
    /// 오류가 나서 꺼졌다.
    disabled: bool,
}

struct Attached {
    script: usize,
    /// 스크립트의 `this` — 액터마다 따로.
    state: Dynamic,
    spawned: bool,
}

/// 스크립트가 보는 한 tick 의 월드 요약 + 스크립트가 낸 것.
///
/// Rhai 에 등록한 함수는 `'static` 이어야 해서 월드를 빌려 갈 수 없다. 그래서 tick 마다
/// 필요한 것만 복사해 두고, 함수들은 이 공유 상자를 본다.
#[derive(Default)]
struct Ctx {
    units: Vec<UnitInfo>,
    index: HashMap<Entity, usize>,
    /// 진영 관계 표 — 이번 tick 에 있는 진영만.
    factions: Vec<FactionId>,
    relations: Vec<Relation>,
    now: FLOAT,
    /// 지금 훅을 돌리는 유닛과 스크립트 이름 — 자기 유닛만 조종하게, `print` 에 이름을 붙이게.
    current: Option<(Entity, String)>,
    out: Vec<Intent>,
    log: Vec<ScriptLog>,
    rng: u64,
}

#[derive(Clone, Copy)]
struct UnitInfo {
    entity: Entity,
    pos: Vec2,
    hp: u32,
    max_hp: u32,
    alive: bool,
    moving: bool,
    faction: FactionId,
    level: u8,
    basic_attack: Option<(SkillId, f32)>,
}

impl Ctx {
    fn unit(&self, e: Entity) -> Option<&UnitInfo> {
        self.index.get(&e).map(|&i| &self.units[i])
    }

    fn relation(&self, a: FactionId, b: FactionId) -> Relation {
        let at = |f| self.factions.iter().position(|&x| x == f);
        match (at(a), at(b)) {
            (Some(i), Some(j)) => self.relations[i * self.factions.len() + j],
            _ => Relation::Neutral,
        }
    }

    /// 보이는 적대 유닛, 가까운 순. 거리가 같으면 월드 순서 (결정적).
    fn enemies(&self, me: Entity, range: FLOAT) -> Vec<Entity> {
        let Some(me) = self.unit(me).copied() else {
            return Vec::new();
        };
        let mut found: Vec<(f32, usize)> = self
            .units
            .iter()
            .enumerate()
            .filter(|(_, u)| u.alive && u.entity != me.entity)
            .filter(|(_, u)| self.relation(me.faction, u.faction) == Relation::Hostile)
            .filter(|(_, u)| me.level >= u.level)
            .map(|(i, u)| (me.pos.distance(u.pos), i))
            .filter(|&(d, _)| d <= range)
            .collect();
        found.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        found
            .into_iter()
            .map(|(_, i)| self.units[i].entity)
            .collect()
    }

    /// 지금 훅을 돌리는 유닛인지 — 다른 유닛은 조종할 수 없다.
    fn check_self(&self, me: Entity) -> Result<(), Box<EvalAltResult>> {
        match &self.current {
            Some((e, _)) if *e == me => Ok(()),
            _ => Err("자기 유닛(me)만 조종할 수 있음".into()),
        }
    }

    /// SplitMix64 — 시뮬레이션의 드롭 난수와 같은 알고리즘, 따로인 흐름.
    fn next_u64(&mut self) -> u64 {
        self.rng = self.rng.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// 스크립트 안의 유닛 값. 핸들만 들고 있고, 속성은 그 tick 의 요약에서 읽는다.
#[derive(Clone, Copy, Debug, PartialEq)]
struct UnitRef(Entity);

impl RhaiHost {
    /// `seed` 는 스크립트 난수(`rand`)의 시드다 — 같은 시드면 같은 결과.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        let ctx = Rc::new(RefCell::new(Ctx {
            rng: seed,
            ..Ctx::default()
        }));
        Self {
            engine: build_engine(&ctx),
            scripts: Vec::new(),
            attached: HashMap::new(),
            ctx,
        }
    }

    /// 스크립트를 컴파일해 등록한다. 문법 오류·훅 인자 수 오류는 `이름: 위치 — 이유` 로 돌려준다.
    pub fn add_script(&mut self, name: &str, source: &str) -> Result<ScriptId, String> {
        let (ast, has) = compile(&self.engine, source).map_err(|d| format!("{name}: {d}"))?;
        self.scripts.push(Script {
            name: name.to_owned(),
            ast,
            has,
            disabled: false,
        });
        Ok(ScriptId(self.scripts.len() - 1))
    }

    /// 유닛에 스크립트를 붙인다. 다음 tick 에 `on_spawn` 이 불린다.
    pub fn attach(&mut self, unit: Entity, script: ScriptId) {
        self.attached.insert(
            unit,
            Attached {
                script: script.0,
                state: Dynamic::from_map(Map::new()),
                spawned: false,
            },
        );
    }

    /// 이 tick 에 스크립트가 볼 월드 요약을 만든다.
    fn snapshot(&self, world: &SimWorld) {
        let mut ctx = self.ctx.borrow_mut();
        ctx.units.clear();
        ctx.index.clear();
        for (entity, u) in world.units() {
            let def = u.def();
            let level = world
                .tiles()
                .get(world.tiles().world_to_tile(u.pos()))
                .map_or(0, |t| t.level);
            let basic_attack = def
                .basic_attack
                .and_then(|id| world.skill(id).map(|s| (id, s.range)));
            let i = ctx.units.len();
            ctx.index.insert(entity, i);
            ctx.units.push(UnitInfo {
                entity,
                pos: u.pos(),
                hp: u.hp(),
                max_hp: def.max_hp,
                alive: u.is_alive(),
                moving: u.is_moving(),
                faction: def.faction,
                level,
                basic_attack,
            });
        }
        let mut factions: Vec<FactionId> = ctx.units.iter().map(|u| u.faction).collect();
        factions.sort_by_key(|f| f.0);
        factions.dedup();
        ctx.relations = factions
            .iter()
            .flat_map(|&a| factions.iter().map(move |&b| (a, b)))
            .map(|(a, b)| world.relation(a, b))
            .collect();
        ctx.factions = factions;
        ctx.now = world.now().as_secs_f32();
    }

    /// 훅 하나를 부른다. 오류가 나면 스크립트를 끄고 한 번 알린다.
    fn call(&mut self, unit: Entity, hook: usize, args: Vec<Dynamic>) {
        let Some(attached) = self.attached.get_mut(&unit) else {
            return;
        };
        let script = &mut self.scripts[attached.script];
        if script.disabled || !script.has[hook] {
            return;
        }
        let name = HOOKS[hook].0;
        self.ctx.borrow_mut().current = Some((unit, script.name.clone()));
        let options = CallFnOptions::new()
            .eval_ast(false)
            .bind_this_ptr(&mut attached.state);
        let result = self.engine.call_fn_with_options::<Dynamic>(
            options,
            &mut Scope::new(),
            &script.ast,
            name,
            args,
        );
        let mut ctx = self.ctx.borrow_mut();
        ctx.current = None;
        if let Err(e) = result {
            script.disabled = true;
            let reason = match *e {
                EvalAltResult::ErrorTooManyOperations(pos) => {
                    format!("실행 스텝 상한({MAX_OPERATIONS}) 초과 — 무한 루프? ({pos})")
                }
                other => other.to_string(),
            };
            ctx.log.push(ScriptLog {
                error: true,
                message: format!("{}: {name} 오류로 스크립트를 껐음 — {reason}", script.name),
            });
        }
    }
}

impl ScriptHost for RhaiHost {
    fn run(&mut self, world: &SimWorld, dt: Duration, events: &[Event], out: &mut Vec<Intent>) {
        if self.attached.is_empty() {
            return;
        }
        self.snapshot(world);
        let unit = |e: Entity| Dynamic::from(UnitRef(e));
        let int = |n: u32| Dynamic::from(INT::from(n));

        // 이전 tick 의 일 — 일어난 순서대로.
        for event in events {
            match *event {
                Event::Damaged {
                    attacker,
                    target,
                    amount,
                    ..
                } => {
                    self.call(attacker, 2, vec![unit(attacker), unit(target), int(amount)]);
                    if world.unit(target).is_some_and(|u| u.is_alive()) {
                        self.call(target, 3, vec![unit(target), unit(attacker), int(amount)]);
                    }
                }
                Event::Died { unit: dead, killer } => {
                    self.call(dead, 4, vec![unit(dead), unit(killer)]);
                }
                _ => {}
            }
        }

        // 살아 있는 유닛마다, 월드 순서대로.
        let dt = Dynamic::from(dt.as_secs_f32() as FLOAT);
        let alive: Vec<Entity> = world
            .units()
            .filter(|(e, u)| u.is_alive() && self.attached.contains_key(e))
            .map(|(e, _)| e)
            .collect();
        for e in alive {
            let first = self
                .attached
                .get_mut(&e)
                .is_some_and(|a| !std::mem::replace(&mut a.spawned, true));
            if first {
                self.call(e, 0, vec![unit(e)]);
            }
            self.call(e, 1, vec![unit(e), dt.clone()]);
        }

        out.append(&mut self.ctx.borrow_mut().out);
    }

    fn drain_log(&mut self) -> Vec<ScriptLog> {
        std::mem::take(&mut self.ctx.borrow_mut().log)
    }
}

/// 컴파일 오류 하나 — 편집기가 해당 줄을 짚어 보여 줄 수 있게 위치를 따로 든다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// 1 부터. 위치를 모르는 오류(훅 인자 수 등)는 `None`.
    pub line: Option<usize>,
    /// 1 부터.
    pub column: Option<usize>,
    pub message: String,
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.line, self.column) {
            (Some(l), Some(c)) => write!(f, "{l}행 {c}칸 — {}", self.message),
            (Some(l), None) => write!(f, "{l}행 — {}", self.message),
            _ => f.write_str(&self.message),
        }
    }
}

/// 컴파일에 성공한 스크립트의 요약.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Compiled {
    /// 정의된 훅 이름 — 엔진이 부르는 순서(스폰·tick·공격·피격·사망).
    pub hooks: Vec<&'static str>,
}

/// 실행하지 않고 **컴파일만** 한다 — 에디터의 "컴파일" 버튼.
///
/// 실행 때와 같은 규칙으로 검사한다: 문법 · 선언하지 않은 변수(strict) · 식 깊이 · 훅 인자 수.
/// 없는 함수 이름(오타)은 실행할 때에야 알 수 있다 — Rhai 는 호출을 실행 시점에 찾는다.
pub fn check(source: &str) -> Result<Compiled, Diagnostic> {
    let (_, has) = compile(&base_engine(), source)?;
    Ok(Compiled {
        hooks: HOOKS
            .iter()
            .zip(has)
            .filter(|(_, on)| *on)
            .map(|((name, _), _)| *name)
            .collect(),
    })
}

/// 훅 이름과 인자 목록 — 편집기의 도움말·새 스크립트 틀에 쓴다.
#[must_use]
pub fn hook_signatures() -> Vec<String> {
    const PARAMS: [&str; 5] = [
        "me",
        "me, dt",
        "me, target, amount",
        "me, attacker, amount",
        "me, killer",
    ];
    HOOKS
        .iter()
        .zip(PARAMS)
        .map(|((name, _), params)| format!("fn {name}({params})"))
        .collect()
}

/// 컴파일과 훅 검사. 실행 엔진과 검사 엔진이 **같은 함수**를 거친다 — 둘이 다르게 판정하지 않게.
fn compile(engine: &Engine, source: &str) -> Result<(AST, [bool; 5]), Diagnostic> {
    let ast = engine.compile(source).map_err(|e| {
        let pos = e.position();
        Diagnostic {
            line: pos.line(),
            column: pos.position(),
            message: e.err_type().to_string(),
        }
    })?;
    let mut has = [false; 5];
    for f in ast.iter_functions() {
        if let Some(i) = HOOKS.iter().position(|(hook, _)| *hook == f.name) {
            let want = HOOKS[i].1;
            if f.params.len() != want {
                return Err(Diagnostic {
                    line: None,
                    column: None,
                    message: format!(
                        "{}({}) — 인자는 {want}개여야 함",
                        f.name,
                        f.params.join(", ")
                    ),
                });
            }
            has[i] = true;
        }
    }
    Ok((ast, has))
}

/// 제한·언어 옵션만 설정한 엔진 — 컴파일 판정에 영향을 주는 것은 전부 여기.
fn base_engine() -> Engine {
    let mut engine = Engine::new();
    engine
        .set_strict_variables(true)
        .set_max_operations(MAX_OPERATIONS)
        .set_max_call_levels(32)
        .set_max_expr_depths(64, 32)
        .set_max_string_size(4096)
        .set_max_array_size(1024)
        .set_max_map_size(256);
    engine
}

/// 스크립트가 쓸 함수를 등록한 엔진.
fn build_engine(ctx: &Rc<RefCell<Ctx>>) -> Engine {
    let mut engine = base_engine();

    {
        let ctx = ctx.clone();
        engine.on_print(move |text| {
            let mut c = ctx.borrow_mut();
            let who = c.current.as_ref().map_or(String::new(), |(_, n)| n.clone());
            c.log.push(ScriptLog {
                error: false,
                message: format!("{who}: {text}"),
            });
        });
    }

    engine.register_type_with_name::<UnitRef>("Unit");
    engine.register_fn("==", |a: UnitRef, b: UnitRef| a == b);
    engine.register_fn("!=", |a: UnitRef, b: UnitRef| a != b);
    engine.register_fn("to_string", |u: &mut UnitRef| {
        format!("유닛#{}", u.0.index())
    });

    // ── 유닛 속성 ────────────────────────────────────────────────────────
    macro_rules! getter {
        ($name:literal, $ty:ty, $default:expr, |$u:ident| $body:expr) => {{
            let ctx = ctx.clone();
            engine.register_get($name, move |r: &mut UnitRef| -> $ty {
                ctx.borrow().unit(r.0).map_or($default, |$u| $body)
            });
        }};
    }
    getter!("x", FLOAT, 0.0, |u| u.pos.x);
    getter!("y", FLOAT, 0.0, |u| u.pos.y);
    getter!("hp", INT, 0, |u| INT::from(u.hp));
    getter!("max_hp", INT, 0, |u| INT::from(u.max_hp));
    getter!("alive", bool, false, |u| u.alive);
    getter!("moving", bool, false, |u| u.moving);
    engine.register_get("id", |r: &mut UnitRef| INT::from(r.0.index()));

    // ── 월드 읽기 ────────────────────────────────────────────────────────
    {
        let ctx = ctx.clone();
        engine.register_fn("distance", move |a: UnitRef, b: UnitRef| -> FLOAT {
            let c = ctx.borrow();
            match (c.unit(a.0), c.unit(b.0)) {
                (Some(a), Some(b)) => a.pos.distance(b.pos),
                _ => FLOAT::INFINITY,
            }
        });
    }
    {
        let ctx = ctx.clone();
        engine.register_fn(
            "nearest_enemy",
            move |me: UnitRef, range: FLOAT| -> Dynamic {
                ctx.borrow()
                    .enemies(me.0, range)
                    .first()
                    .map_or(Dynamic::UNIT, |&e| Dynamic::from(UnitRef(e)))
            },
        );
    }
    {
        let ctx = ctx.clone();
        engine.register_fn("enemies", move |me: UnitRef, range: FLOAT| -> Array {
            ctx.borrow()
                .enemies(me.0, range)
                .into_iter()
                .map(|e| Dynamic::from(UnitRef(e)))
                .collect()
        });
    }
    {
        let ctx = ctx.clone();
        engine.register_fn("can_see", move |me: UnitRef, other: UnitRef| -> bool {
            let c = ctx.borrow();
            match (c.unit(me.0), c.unit(other.0)) {
                (Some(a), Some(b)) => a.level >= b.level,
                _ => false,
            }
        });
    }
    {
        let ctx = ctx.clone();
        engine.register_fn("attack_range", move |me: UnitRef| -> FLOAT {
            ctx.borrow()
                .unit(me.0)
                .and_then(|u| u.basic_attack)
                .map_or(0.0, |(_, range)| range)
        });
    }
    {
        let ctx = ctx.clone();
        engine.register_fn("time", move || -> FLOAT { ctx.borrow().now });
    }
    {
        let ctx = ctx.clone();
        engine.register_fn("rand", move |n: INT| -> Result<INT, Box<EvalAltResult>> {
            if n <= 0 {
                return Err(format!("rand({n}) — 1 이상이어야 함").into());
            }
            let r = ctx.borrow_mut().next_u64();
            // n 은 양수라 u64 로 바꿔도 값이 같고, 나머지는 n 보다 작아 INT 에 들어간다.
            Ok((r % n.unsigned_abs()) as INT)
        });
    }
    {
        let ctx = ctx.clone();
        engine.register_fn("rand_float", move || -> FLOAT {
            // 위 24비트 — f32 가수부에 정확히 들어간다.
            (ctx.borrow_mut().next_u64() >> 40) as FLOAT / (1u64 << 24) as FLOAT
        });
    }

    // ── 행동 (Intent) ────────────────────────────────────────────────────
    {
        let ctx = ctx.clone();
        engine.register_fn(
            "move_to",
            move |me: UnitRef, x: FLOAT, y: FLOAT| -> Result<(), Box<EvalAltResult>> {
                let mut c = ctx.borrow_mut();
                c.check_self(me.0)?;
                if !(x.is_finite() && y.is_finite()) {
                    return Err("move_to — 좌표가 수가 아님".into());
                }
                c.out.push(Intent::MoveTo {
                    unit: me.0,
                    target: Vec2::new(x, y),
                });
                Ok(())
            },
        );
    }
    {
        let ctx = ctx.clone();
        engine.register_fn(
            "move_to",
            move |me: UnitRef, to: UnitRef| -> Result<(), Box<EvalAltResult>> {
                let mut c = ctx.borrow_mut();
                c.check_self(me.0)?;
                if let Some(target) = c.unit(to.0).map(|u| u.pos) {
                    c.out.push(Intent::MoveTo { unit: me.0, target });
                }
                Ok(())
            },
        );
    }
    {
        let ctx = ctx.clone();
        engine.register_fn(
            "stop",
            move |me: UnitRef| -> Result<(), Box<EvalAltResult>> {
                let mut c = ctx.borrow_mut();
                c.check_self(me.0)?;
                c.out.push(Intent::Stop { unit: me.0 });
                Ok(())
            },
        );
    }
    {
        let ctx = ctx.clone();
        engine.register_fn(
            "attack",
            move |me: UnitRef, target: UnitRef| -> Result<bool, Box<EvalAltResult>> {
                let mut c = ctx.borrow_mut();
                c.check_self(me.0)?;
                // 기본 공격이 없는 액터는 칠 수 없다 — 오류가 아니라 false.
                let Some((skill, _)) = c.unit(me.0).and_then(|u| u.basic_attack) else {
                    return Ok(false);
                };
                c.out.push(Intent::Attack {
                    unit: me.0,
                    target: target.0,
                    skill,
                });
                Ok(true)
            },
        );
    }
    {
        let ctx = ctx.clone();
        engine.register_fn(
            "attack",
            move |me: UnitRef, target: UnitRef, skill: INT| -> Result<bool, Box<EvalAltResult>> {
                let mut c = ctx.borrow_mut();
                c.check_self(me.0)?;
                let skill = u32::try_from(skill)
                    .map_err(|_| format!("attack — 스킬 번호 {skill} 가 범위 밖"))?;
                c.out.push(Intent::Attack {
                    unit: me.0,
                    target: target.0,
                    skill: SkillId(skill),
                });
                Ok(true)
            },
        );
    }

    engine
}

#[cfg(test)]
mod tests;
