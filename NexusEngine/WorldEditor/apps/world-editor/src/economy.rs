//! 재화 매니저 — 공급·소비 표와 헤드리스 경제 시뮬레이터 (R7).
//!
//! ```text
//! ┌ 재화 매니저 ───────────────────────────────────────────────┐
//! │ [공급·소비 표] [시뮬레이션]                                  │
//! │  들어오는 곳: 처치 보상 · 드롭(기대 판매가) · 퀘스트 · 트리거 · 던전 클리어 · 시작 재화
//! │  나가는 곳:   상점 · 제작 수수료 · 텔레포트 · 치료 · 트리거 차감
//! │  경험치:      처치 · 퀘스트 · 트리거 · 던전 클리어
//! │ ─ 시뮬레이션 ─  [10 분] [실행]                                │
//! │  레벨 | 처치 | 쓰러짐 | 골드/시간 | 경험치/시간 | 드롭 가치/시간 | 끝 레벨
//! └────────────────────────────────────────────────────────────┘
//! ```
//!
//! - **표는 데이터를 읽기만 한다** — `data/rules.ron`·`display.ron`·`levels/` 를 창을 열 때·다시 읽기 때 읽는다.
//! - **시뮬레이션은 게임과 같은 경로로 돈다** — 레벨의 존 파일로 [`PlaySession`] 을 만들고(새 게임, 기본 직업),
//!   봇이 사람처럼 **클릭 주문**을 낸다(가까운 땅의 아이템 → 가까운 적). 그래서 판정·드롭·리스폰·부활이
//!   전부 실제 규칙 그대로다. 시드가 같으면 결과도 같다(결정적 시뮬레이션).
//! - 창이 도는 동안 멈춘다 — 레벨 하나 10분이 디버그 빌드에서 몇 초 걸린다. 스레드로 빼지 않은 이유는
//!   스크립트 호스트(Rhai)가 스레드를 넘지 못하기 때문이다.
//! - 봇은 상점·퀘스트·스킬을 쓰지 않는다 — "사냥만 했을 때의 시간당 수입" 이다. 소비는 표로 비교한다.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Duration;

use nexus_core::{Camera2d, Entity};
use nexus_render_wgpu::egui;
use nexus_sim::{
    BagKind, CurrencyId, Effect, ItemId, ItemKind as SimItemKind, LootTableId, Price, Relation,
    Service,
};

use crate::game_data::GameData;
use crate::level::Levels;
use crate::play::{LevelLinks, PlayOptions, PlaySession};

/// 시뮬레이션 tick — 게임과 같은 20Hz.
const DT: Duration = Duration::from_millis(50);
/// 봇이 아이템을 주우러 가는 거리 (m). 더 멀면 사냥을 먼저 한다.
const PICKUP_REACH: f32 = 12.0;
/// 봇이 포션을 마시는 HP 비율 (%).
const DRINK_BELOW: u32 = 35;
/// 같은 대상을 이만큼 연달아 골랐는데도 끝나지 않으면(갈 수 없음·가방 가득) 그 대상은 버린다.
const MAX_RETRIES: u32 = 3;

/// 표 한 줄.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LedgerLine {
    /// 무엇 — "처치 보상", "상점" …
    pub(crate) kind: &'static str,
    /// 어디서 — "슬라임 (#100)", "퀘스트 #1 슬라임 소탕" …
    pub(crate) source: String,
    /// 양 — "15 골드", "기대 4.5 골드", "60 경험치" …
    pub(crate) amount: String,
}

/// 공급·소비 표.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Ledger {
    pub(crate) income: Vec<LedgerLine>,
    pub(crate) spending: Vec<LedgerLine>,
    pub(crate) exp: Vec<LedgerLine>,
}

/// 상점들이 사들이는 가장 좋은 비율 (천분율) — 드롭의 "판매 가치" 기준.
fn best_buy_rate(data: &GameData) -> u32 {
    data.shops()
        .iter()
        .map(|(_, s)| s.buy_rate_per_mille)
        .max()
        .unwrap_or(0)
}

/// 아이템 하나를 상점에 팔면 받는 값 (기본 재화가 아니면 0).
fn sell_value(data: &GameData, item: ItemId, rate: u32) -> u64 {
    data.item_price(item)
        .filter(|p| Some(p.currency) == data.default_currency())
        .map_or(0, |p| u64::from(p.amount) * u64::from(rate) / 1000)
}

/// 드롭 표 한 번의 기대 판매가 (기본 재화, 소수).
fn loot_expectation(data: &GameData, table: LootTableId, rate: u32) -> f64 {
    data.loot_table(table).map_or(0.0, |rows| {
        rows.iter()
            .map(|r| {
                f64::from(r.chance_per_mille.min(1000)) / 1000.0
                    * f64::from(r.count)
                    * sell_value(data, r.item, rate) as f64
            })
            .sum()
    })
}

impl Ledger {
    /// 데이터에서 표를 모은다. `levels` 의 던전 클리어 보상도 넣는다.
    pub(crate) fn collect(data: &GameData, levels: &Levels) -> Self {
        let mut l = Self::default();
        let rate = best_buy_rate(data);
        let currency = data.default_currency();
        let money = |amount: f64| match currency {
            Some(c) => format!("{amount:.1} {}", data.currency_name(c)),
            None => format!("{amount:.1}"),
        };
        if let Some(p) = data.starting_currency() {
            l.income.push(LedgerLine {
                kind: "시작 재화",
                source: String::from("새 게임"),
                amount: data.price_text(p),
            });
        }
        for (actor, name, def) in data.actor_catalog() {
            let who = format!("{name} (#{})", actor.raw());
            if let Some(p) = def.bounty {
                l.income.push(LedgerLine {
                    kind: "처치 보상",
                    source: who.clone(),
                    amount: data.price_text(p),
                });
            }
            if let Some(t) = def.loot {
                let e = loot_expectation(data, t, rate);
                if e > 0.0 {
                    l.income.push(LedgerLine {
                        kind: "드롭 (기대 판매가)",
                        source: who.clone(),
                        amount: money(e),
                    });
                }
            }
            if def.exp_reward > 0 {
                l.exp.push(LedgerLine {
                    kind: "처치",
                    source: who.clone(),
                    amount: format!("{} 경험치", def.exp_reward),
                });
            }
            // NPC 기능 — 치료 값.
            for s in data
                .services()
                .get(&actor)
                .map(Vec::as_slice)
                .unwrap_or_default()
            {
                if let Service::Heal { price } = s
                    && price.amount > 0
                {
                    l.spending.push(LedgerLine {
                        kind: "치료",
                        source: who.clone(),
                        amount: data.price_text(*price),
                    });
                }
            }
        }
        for (id, shop) in data.shops() {
            for g in &shop.goods {
                l.spending.push(LedgerLine {
                    kind: "상점",
                    source: format!("상점 #{} — {}", id.0, data.item_name(g.item)),
                    amount: data.price_text(g.price),
                });
            }
            if shop.buy_rate_per_mille > 0 {
                l.income.push(LedgerLine {
                    kind: "아이템 판매",
                    source: format!("상점 #{}", id.0),
                    amount: format!("기본 값의 {}%", shop.buy_rate_per_mille / 10),
                });
            }
        }
        for (id, craft) in data.crafts() {
            for (i, r) in craft.recipes.iter().enumerate() {
                if let Some(fee) = r.fee {
                    l.spending.push(LedgerLine {
                        kind: "제작 수수료",
                        source: format!("제작 #{} 레시피 {i} — {}", id.0, data.item_name(r.output)),
                        amount: data.price_text(fee),
                    });
                }
            }
        }
        for (id, tp) in data.teleports() {
            for (i, d) in tp.destinations.iter().enumerate() {
                if let Some(p) = d.price {
                    l.spending.push(LedgerLine {
                        kind: "텔레포트",
                        source: data.destination_name(*id, i),
                        amount: data.price_text(p),
                    });
                }
            }
        }
        let effects = |source: String, list: &[Effect], l: &mut Self| {
            for e in list {
                match *e {
                    Effect::GiveCurrency { currency, amount } => l.income.push(LedgerLine {
                        kind: "보상",
                        source: source.clone(),
                        amount: data.price_text(Price { currency, amount }),
                    }),
                    Effect::TakeCurrency { currency, amount } => l.spending.push(LedgerLine {
                        kind: "차감",
                        source: source.clone(),
                        amount: data.price_text(Price { currency, amount }),
                    }),
                    Effect::GiveLoot(t) => l.income.push(LedgerLine {
                        kind: "보상 드롭 (기대 판매가)",
                        source: source.clone(),
                        amount: money(loot_expectation(data, t, rate)),
                    }),
                    Effect::GiveExp(n) => l.exp.push(LedgerLine {
                        kind: "보상",
                        source: source.clone(),
                        amount: format!("{n} 경험치"),
                    }),
                    _ => {}
                }
            }
        };
        for (id, q) in data.quests() {
            let source = format!("퀘스트 #{} {}", id.0, data.quest_title(*id));
            effects(source.clone(), &q.rewards, &mut l);
            for stage in &q.stages {
                effects(source.clone(), &stage.then, &mut l);
            }
        }
        for (id, t) in data.triggers() {
            effects(format!("트리거 #{}", id.0), &t.then, &mut l);
        }
        for (id, name) in levels.list() {
            if let Some(clear) = levels.clear_of(&id) {
                effects(format!("던전 클리어 — {name}"), &clear.rewards, &mut l);
            }
        }
        l
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 헤드리스 시뮬레이션
// ─────────────────────────────────────────────────────────────────────────────

/// 레벨 하나를 돌린 결과.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RunReport {
    pub(crate) level: String,
    pub(crate) name: String,
    pub(crate) minutes: u32,
    /// 쓰러진 적대 유닛 수 (누가 쓰러뜨렸든).
    pub(crate) kills: u32,
    /// 플레이어가 쓰러진 횟수.
    pub(crate) deaths: u32,
    /// 지갑의 기본 재화 증감 — 처치 보상·퀘스트 등.
    pub(crate) gold: i64,
    /// 얻은 경험치 합 (레벨업에 쓴 것까지).
    pub(crate) exp: u64,
    /// 가방에 늘어난 아이템을 상점에 팔면 받을 값 (기본 재화).
    pub(crate) loot_value: u64,
    pub(crate) end_level: u32,
}

impl RunReport {
    /// 시간당으로 — `value × 60 / 분`.
    pub(crate) fn per_hour(&self, value: i64) -> i64 {
        if self.minutes == 0 {
            0
        } else {
            value * 60 / i64::from(self.minutes)
        }
    }
}

/// 레벨 하나를 `minutes` 분 돌린다. 존이 없는 레벨·읽지 못한 존 파일은 이유를 돌려준다.
///
/// 존 파일은 `root`(보통 작업 디렉터리) 기준으로 읽는다.
pub(crate) fn simulate_level(
    root: &Path,
    levels: &Levels,
    id: &str,
    data: GameData,
    minutes: u32,
) -> Result<RunReport, String> {
    let level = levels
        .level(id)
        .ok_or_else(|| format!("레벨 '{id}' 이 없음"))?;
    let zone = level.zone.as_deref().ok_or("존이 없는 레벨")?;
    let text = std::fs::read_to_string(root.join(zone)).map_err(|e| format!("{zone}: {e}"))?;
    let scene = crate::zone_file::from_ron(&text).map_err(|e| format!("{zone}: {e}"))?;
    let rate = best_buy_rate(&data);
    let currency = data.default_currency();
    let mut s = PlaySession::start(
        &scene,
        Camera2d::default(),
        data,
        PlayOptions {
            links: LevelLinks {
                here: id.to_owned(),
                requires: levels.entry_conditions(),
                clear: levels.clear_of(id),
            },
            ..PlayOptions::default()
        },
    )?;

    let me = s.player();
    let start_gold = gold_of(&s, currency);
    let start_exp = total_exp(&s);
    let start_loot = carried_value(&s, rate);
    let mut alive: HashMap<Entity, bool> = HashMap::new();
    let mut was_alive = true;
    let (mut kills, mut deaths) = (0, 0);
    let mut bot = Bot::default();
    let ticks = u64::from(minutes) * 60 * 1000 / DT.as_millis() as u64;
    for _ in 0..ticks {
        bot.act(&mut s, rate);
        s.tick(DT, None);
        // 다른 레벨로 가는 문을 밟아도 레벨을 열지 않는다 — 이 존에서만 잰다.
        let _ = s.take_travel();
        let world = s.world();
        let Some(player) = world.unit(me) else { break };
        if was_alive && !player.is_alive() {
            deaths += 1;
        }
        was_alive = player.is_alive();
        let faction = player.def().faction;
        for (e, u) in world.units() {
            if world.relation(faction, u.def().faction) != Relation::Hostile {
                continue;
            }
            let now = u.is_alive();
            if alive.insert(e, now) == Some(true) && !now {
                kills += 1;
            }
        }
    }
    Ok(RunReport {
        level: id.to_owned(),
        name: level.name.clone(),
        minutes,
        kills,
        deaths,
        gold: gold_of(&s, currency) as i64 - start_gold as i64,
        exp: total_exp(&s).saturating_sub(start_exp),
        loot_value: carried_value(&s, rate).saturating_sub(start_loot),
        end_level: s.world().unit(me).map_or(0, |u| u.progress().level()),
    })
}

fn gold_of(s: &PlaySession, currency: Option<CurrencyId>) -> u64 {
    let (Some(c), Some(u)) = (currency, s.world().unit(s.player())) else {
        return 0;
    };
    u.wallet().get(c)
}

/// 지금까지 모은 경험치 합 — 지난 레벨들의 필요량 + 지금 레벨에서 모은 것.
fn total_exp(s: &PlaySession) -> u64 {
    let world = s.world();
    let Some(u) = world.unit(s.player()) else {
        return 0;
    };
    let p = u.progress();
    (1..p.level())
        .map(|l| u64::from(world.exp_needed(l)))
        .sum::<u64>()
        + u64::from(p.exp())
}

/// 두 가방의 아이템을 다 팔면 받을 값.
fn carried_value(s: &PlaySession, rate: u32) -> u64 {
    let world = s.world();
    let Some(u) = world.unit(s.player()) else {
        return 0;
    };
    let inv = u.inventory();
    [BagKind::Consumable, BagKind::Equipment]
        .into_iter()
        .flat_map(|b| inv.bag(b).iter())
        .map(|(_, stack)| {
            let price = world
                .item_price(stack.item)
                .map_or(0, |p| u64::from(p.amount));
            price * u64::from(rate) / 1000 * u64::from(stack.count)
        })
        .sum()
}

/// 사냥만 하는 봇 — 사람처럼 클릭 주문을 낸다.
#[derive(Debug, Default)]
struct Bot {
    last: Option<Entity>,
    tries: u32,
    /// 갈 수 없거나 주울 수 없어 버린 대상.
    skip: HashSet<Entity>,
}

impl Bot {
    fn act(&mut self, s: &mut PlaySession, rate: u32) {
        let world = s.world();
        let me = s.player();
        let Some(u) = world.unit(me) else { return };
        if !u.is_alive() {
            return; // 부활은 게임이 한다 (기본 부활).
        }
        // 다치면 회복 물약부터 — 가진 것 중 회복량이 가장 큰 것.
        if u.hp() * 100 < u.max_hp() * DRINK_BELOW {
            let potion = s
                .consumable_counts()
                .into_iter()
                .filter_map(|(item, _)| match world.item_def(ItemId(item))?.kind {
                    SimItemKind::Consumable { heal, .. } if heal > 0 => Some((heal, item)),
                    _ => None,
                })
                .max();
            if let Some((_, item)) = potion {
                s.use_item_by_id(item);
                return;
            }
        }
        if s.busy() {
            return;
        }
        let pos = u.pos();
        let faction = u.def().faction;
        let skip = &self.skip;
        // 가까운 땅의 아이템 — 팔 수 있는 것만 (값이 없는 것은 사냥을 막지 않게).
        let item = world
            .ground_items()
            .filter(|(e, g)| {
                !skip.contains(e)
                    && g.pos.distance(pos) <= PICKUP_REACH
                    && (rate == 0 || world.item_price(g.stack.item).is_some())
            })
            .min_by(|a, b| a.1.pos.distance(pos).total_cmp(&b.1.pos.distance(pos)))
            .map(|(e, g)| (e, g.pos));
        let enemy = || {
            world
                .units()
                .filter(|(e, t)| {
                    !skip.contains(e)
                        && t.is_alive()
                        && !t.def().immortal
                        && world.relation(faction, t.def().faction) == Relation::Hostile
                })
                .min_by(|a, b| a.1.pos().distance(pos).total_cmp(&b.1.pos().distance(pos)))
                .map(|(e, t)| (e, t.pos()))
        };
        let Some((target, at)) = item.or_else(enemy) else {
            return;
        };
        if self.last == Some(target) {
            self.tries += 1;
            if self.tries > MAX_RETRIES {
                self.skip.insert(target);
                self.last = None;
                return;
            }
        } else {
            self.last = Some(target);
            self.tries = 1;
        }
        s.click(at, 0.01);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 창
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    Ledger,
    Simulate,
}

/// 재화 매니저 창.
#[derive(Debug)]
pub(crate) struct EconomyManager {
    open: bool,
    tab: Tab,
    ledger: Option<Ledger>,
    minutes: u32,
    reports: Vec<Result<RunReport, (String, String)>>,
    status: Option<(bool, String)>,
}

impl Default for EconomyManager {
    fn default() -> Self {
        Self {
            open: false,
            tab: Tab::Ledger,
            ledger: None,
            minutes: 10,
            reports: Vec::new(),
            status: None,
        }
    }
}

impl EconomyManager {
    pub(crate) fn open(&mut self) {
        self.open = true;
        self.reload();
    }

    /// 표를 다시 모은다.
    fn reload(&mut self) {
        match GameData::load() {
            Ok(data) => {
                let (levels, _) = Levels::load();
                self.ledger = Some(Ledger::collect(&data, &levels));
                self.status = None;
            }
            Err(e) => self.status = Some((true, format!("게임 데이터를 읽을 수 없음 — {e}"))),
        }
    }

    /// 존이 있는 레벨을 전부 돌린다 (스크립트 `economy run` 도 이것을 쓴다).
    pub(crate) fn run_all(&mut self) {
        self.open = true;
        self.tab = Tab::Simulate;
        let (levels, _) = Levels::load();
        self.reports.clear();
        for (id, name) in levels.list() {
            if levels.level(&id).and_then(|l| l.zone.as_ref()).is_none() {
                continue;
            }
            let result = GameData::load()
                .and_then(|data| simulate_level(Path::new("."), &levels, &id, data, self.minutes));
            self.reports
                .push(result.map_err(|e| (format!("{id} ({name})"), e)));
        }
        self.status = Some((
            false,
            format!(
                "{}개 레벨을 {}분씩 돌렸습니다 (게임 시간, 시드는 rules.ron 의 seed)",
                self.reports.len(),
                self.minutes
            ),
        ));
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui) {
        let mut open = self.open;
        egui::Window::new("재화 매니저")
            .open(&mut open)
            .default_size([620.0, 460.0])
            .resizable(true)
            .show(ui.ctx(), |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.tab, Tab::Ledger, "공급·소비 표");
                    ui.selectable_value(&mut self.tab, Tab::Simulate, "시뮬레이션");
                    if ui.button("다시 읽기").clicked() {
                        self.reload();
                    }
                });
                ui.separator();
                match self.tab {
                    Tab::Ledger => self.ledger_ui(ui),
                    Tab::Simulate => self.simulate_ui(ui),
                }
                if let Some((error, message)) = &self.status {
                    if *error {
                        ui.colored_label(egui::Color32::from_rgb(240, 110, 100), message);
                    } else {
                        ui.weak(message);
                    }
                }
            });
        self.open = open;
    }

    fn ledger_ui(&self, ui: &mut egui::Ui) {
        let Some(ledger) = &self.ledger else {
            return;
        };
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (title, lines, id) in [
                ("들어오는 곳 (재화)", &ledger.income, "ledger-in"),
                ("나가는 곳 (재화)", &ledger.spending, "ledger-out"),
                ("경험치", &ledger.exp, "ledger-exp"),
            ] {
                ui.strong(title);
                if lines.is_empty() {
                    ui.weak("없음");
                }
                egui::Grid::new(id)
                    .striped(true)
                    .num_columns(3)
                    .show(ui, |ui| {
                        for line in lines {
                            ui.label(line.kind);
                            ui.label(&line.source);
                            ui.label(&line.amount);
                            ui.end_row();
                        }
                    });
                ui.add_space(6.0);
            }
            ui.weak("드롭의 판매가 = 아이템 기본 값 × 상점이 사들이는 가장 좋은 비율 × 확률.");
        });
    }

    fn simulate_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("레벨마다");
            ui.add(
                egui::DragValue::new(&mut self.minutes)
                    .range(1..=120)
                    .suffix(" 분"),
            );
            if ui.button("실행").clicked() {
                self.run_all();
            }
        });
        ui.weak(
            "봇이 새 게임(기본 직업)으로 사냥만 한다 — 가까운 아이템을 줍고 가까운 적을 친다, HP 가 낮으면 물약. \
             상점·퀘스트·스킬은 쓰지 않는다. 도는 동안 창이 멈춘다.",
        );
        if self.reports.is_empty() {
            return;
        }
        egui::Grid::new("economy-runs")
            .striped(true)
            .num_columns(8)
            .show(ui, |ui| {
                for h in [
                    "레벨",
                    "처치",
                    "쓰러짐",
                    "골드/시간",
                    "경험치/시간",
                    "드롭 가치/시간",
                    "끝 레벨",
                    "",
                ] {
                    ui.strong(h);
                }
                ui.end_row();
                for r in &self.reports {
                    match r {
                        Ok(r) => {
                            ui.label(format!("{} ({})", r.level, r.name));
                            ui.label(r.kills.to_string());
                            ui.label(r.deaths.to_string());
                            ui.label(r.per_hour(r.gold).to_string());
                            ui.label(r.per_hour(r.exp as i64).to_string());
                            ui.label(r.per_hour(r.loot_value as i64).to_string());
                            ui.label(r.end_level.to_string());
                            ui.label("");
                        }
                        Err((who, e)) => {
                            ui.label(who);
                            ui.colored_label(egui::Color32::from_rgb(240, 110, 100), e);
                        }
                    }
                    ui.end_row();
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> GameData {
        GameData::embedded()
    }

    #[test]
    fn the_ledger_lists_sources_and_sinks_from_the_data() {
        let (levels, _) = Levels::load();
        let l = Ledger::collect(&data(), &levels);
        let has = |lines: &[LedgerLine], kind: &str, source: &str| {
            lines
                .iter()
                .any(|x| x.kind == kind && x.source.contains(source))
        };
        assert!(
            has(&l.income, "처치 보상", "슬라임 (#100)"),
            "{:#?}",
            l.income
        );
        assert!(has(&l.income, "드롭 (기대 판매가)", "슬라임 (#100)"));
        assert!(has(&l.income, "보상", "퀘스트 #1"));
        assert!(has(&l.income, "보상", "던전 클리어"), "R6-2 의 던전 보상");
        assert!(has(&l.income, "시작 재화", "새 게임"));
        assert!(has(&l.spending, "상점", "상점 #1"), "{:#?}", l.spending);
        assert!(has(&l.spending, "제작 수수료", "제작 #1"));
        assert!(has(&l.spending, "텔레포트", ""));
        assert!(has(&l.exp, "처치", "슬라임 (#100)"));
        assert!(has(&l.exp, "보상", "던전 클리어"));
    }

    /// R7 끝나는 조건 — 레벨의 시간당 골드·경험치가 나오고, 같은 시드면 같은 결과다.
    #[test]
    fn a_village_hunt_earns_gold_and_exp_deterministically() {
        let (levels, _) = Levels::load();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let run = || simulate_level(&root, &levels, "village", data(), 2).unwrap();
        let a = run();
        assert!(a.kills > 0, "{a:?}");
        assert!(a.gold > 0 && a.exp > 0, "{a:?}");
        assert_eq!(a.per_hour(a.gold), a.gold * 30, "2분 → 시간당 ×30");
        assert_eq!(run(), a, "결정적 — 같은 데이터·시드면 같은 결과");
        // 존이 없는 레벨은 이유를 말한다.
        assert!(
            simulate_level(&root, &levels, "main", data(), 1)
                .unwrap_err()
                .contains("존이 없는")
        );
    }
}
