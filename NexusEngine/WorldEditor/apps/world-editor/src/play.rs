//! 플레이 모드 — 편집 중인 씬을 **그 자리에서** 시뮬레이션으로 돌린다 (S7-1).
//!
//! ```text
//! 씬(편집 데이터) ──시작──▶ SimWorld (복사본) ──LocalAuthority.tick (20Hz)──▶ 화면
//!        ▲                                                   │
//!        └───────────── 정지하면 버린다 (씬은 그대로) ────────┘
//! ```
//!
//! - **씬을 바꾸지 않는다.** 시작할 때 씬을 복사해 시뮬레이션을 만들고, 정지하면 버린다.
//!   그래서 플레이 중 일어난 일(몬스터 사망 등)은 언두 기록과 무관하다.
//! - 입력은 Intent 로만 들어간다. 클릭한 대상에 **다가가는 것은 입력 쪽의 일**이다 (S6 설계) —
//!   [`PlaySession`] 의 주문(`Order`)이 매 tick `MoveTo`/`Attack`/`PickUp` 을 낸다.
//! - 에디터 `Scene` 과 `SimWorld` 는 핸들 공간이 다르다. 이름·색은 시작할 때 만든 대응표로 찾는다.
//! - 규칙 수치·아이템 이름은 [`GameData`] (`data/rules.ron` · `data/display.ron`) 에서 온다 — 코드에 수치가 없다.

use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use nexus_assets::{AnimState, SpriteAnimator};
use nexus_core::{Camera2d, Entity, Vec2, Vec3};
use nexus_render::{DEPTH_LAYER, RenderCommand, SpriteAnchor, TextureId, UvRect};
use nexus_script::RhaiHost;
use nexus_sim::{
    Authority, BagKind, EquipSlot, Event, Intent, ItemStack, LocalAuthority, Progress, Rejection,
    Relation, SimWorld, Unit,
};

use crate::game_data::{ActorLook, GameData};
use crate::save_file::SaveData;
use crate::scene::{ActorId, ItemKind, Scene};
use crate::sprites::{Look, NO_TINT, SpriteLibrary};

/// 쫓는 대상이 마지막 경로 지점에서 이만큼(m) 벗어나야 다시 경로를 잡는다 (AI 와 같은 값).
const REPATH_DISTANCE: f32 = 0.5;
/// 이벤트 기록을 몇 줄까지 보관하나.
const LOG_LINES: usize = 10;

// 겹침 순서 (깊이 편향). 에디터 표시와 같은 규칙 — 지면 표시는 z = 0 + 편향.
const BIAS_PATH: f32 = 2.0 * DEPTH_LAYER;
const BIAS_ITEM: f32 = 3.0 * DEPTH_LAYER;
const BIAS_TARGET: f32 = 4.0 * DEPTH_LAYER;
const BIAS_SPRITE: f32 = 3.0 * DEPTH_LAYER;
const BIAS_BAR: f32 = 2.0 * DEPTH_LAYER;

const PATH_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 0.55];
const TARGET_COLOR: [f32; 4] = [1.0, 0.35, 0.30, 1.0];
const BAR_BACK: [f32; 4] = [0.05, 0.05, 0.07, 1.0];
const BAR_HP: [f32; 4] = [0.30, 0.85, 0.40, 1.0];
const BAR_HP_LOW: [f32; 4] = [0.95, 0.30, 0.25, 1.0];
const CORPSE_TINT: [f32; 4] = [0.35, 0.35, 0.38, 1.0];

/// 플레이어가 클릭으로 내린 주문. 매 tick 이것을 보고 Intent 를 낸다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Order {
    /// 할 일 없음 (이동은 `MoveTo` 한 번으로 끝나므로 주문으로 남기지 않는다).
    Idle,
    /// 다가가서 친다. 대상이 죽거나 거절되면 끝.
    Attack(Entity),
    /// 다가가서 줍는다.
    PickUp(Entity),
}

/// 시뮬레이션 유닛의 이름과 색 — 씬 마커에서 가져온다.
#[derive(Clone, Debug)]
struct Label {
    name: String,
    /// 이 유닛의 **액터 타입** — 스프라이트 시트를 고르는 기준이다.
    actor: ActorId,
    /// 액터 타입이 명시한 색 (없으면 `NO_TINT`).
    tint: [f32; 4],
    /// 무채색 시트에 쓸 마커 종류 색.
    fallback: [f32; 4],
}

/// 인벤토리 패널에서 누른 것.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InventoryAction {
    Use(u16),
    Equip(u16),
    Unequip(EquipSlot),
}

/// 플레이를 시작할 때 주는 설정.
#[derive(Debug, Default)]
pub(crate) struct PlayOptions {
    /// 여기(씬 마커)에서 시작한다 — 없으면 첫 플레이어 스폰 (P3-1).
    pub(crate) spawn: Option<Entity>,
    /// 이어서 할 저장 데이터 (P3). 없으면 새로 시작한다.
    pub(crate) save: Option<SaveData>,
}

/// 진행 중인 플레이 한 판.
#[derive(Debug)]
pub(crate) struct PlaySession {
    auth: LocalAuthority,
    player: Entity,
    labels: HashMap<Entity, Label>,
    /// 유닛마다 따로 — 걷는 유닛과 서 있는 유닛의 위상이 달라야 한다.
    animators: HashMap<Entity, SpriteAnimator>,
    order: Order,
    /// 주문을 위해 마지막으로 경로를 잡은 지점.
    chase_goal: Option<Vec2>,
    log: VecDeque<String>,
    /// 상태 바에 띄울 경고 (스크립트 오류) — 편집기가 꺼내 간다.
    alerts: Vec<String>,
    /// 시작 전 에디터 카메라 — 정지하면 되돌린다.
    editor_camera: Camera2d,
    /// 이번 판의 게임 데이터 (규칙 수치 + 이름·색). 시작할 때 파일에서 읽는다.
    data: GameData,
}

impl PlaySession {
    /// 씬에서 시뮬레이션을 만든다. **첫 플레이어 스폰**에 플레이어가 서고, 나머지 플레이어
    /// 스폰은 스폰 지점일 뿐이라 유닛을 만들지 않는다.
    ///
    /// # Errors
    /// 플레이어 스폰이 없거나, 액터 스크립트에 문법 오류가 있으면 플레이할 수 없다.
    pub(crate) fn start(
        scene: &Scene,
        editor_camera: Camera2d,
        data: GameData,
        options: PlayOptions,
    ) -> Result<Self, String> {
        let mut world = SimWorld::new(scene.tiles.clone());
        data.install(&mut world);

        // 액터 스크립트 (P2) — 플레이 시작마다 새로 컴파일한다. 문법 오류면 플레이를 막는다
        // (고친 것이 적용되지 않은 채 모르고 지나가지 않게 — 데이터 파일과 같은 규칙).
        let mut scripts = RhaiHost::new(data.seed());
        let mut compiled = HashMap::new();
        for (path, source) in data.script_sources() {
            let id = scripts.add_script(path, source)?;
            compiled.insert(path.to_owned(), id);
        }

        let mut labels = HashMap::new();
        let mut player = None;
        // 조작할 스폰 — 고른 마커가 있으면 그것, 없으면 첫 플레이어 스폰 (P3-1).
        let chosen = options.spawn.filter(|e| {
            scene
                .items
                .iter()
                .any(|i| i.entity == *e && i.kind == ItemKind::PlayerSpawn)
        });
        for item in &scene.items {
            let is_player =
                item.kind == ItemKind::PlayerSpawn && chosen.is_none_or(|e| e == item.entity);
            if is_player && player.is_some() {
                continue;
            }
            if item.kind == ItemKind::PlayerSpawn && !is_player {
                continue; // 나머지 플레이어 스폰은 지점일 뿐이다
            }
            // 마커가 가리키는 액터 타입에서 수치·그림이 나오고, 마커별 덮어쓰기가 그 위에 얹힌다 (P1, P1-4).
            let actor = data.resolve_actor(item.actor, item.kind);
            let unit = world.spawn_unit(
                item.pos,
                item.orientation,
                item.overrides.apply(data.unit_def(actor)),
            );
            let name = if is_player {
                String::from("플레이어")
            } else {
                item.name.clone()
            };
            labels.insert(
                unit,
                Label {
                    name,
                    actor,
                    // 두 색을 따로 들고 간다 — 어느 쪽을 쓸지는 시트가 무채색인지로 정해진다.
                    tint: data.actor_look(actor).map_or(NO_TINT, ActorLook::tint),
                    fallback: item.kind.color(),
                },
            );
            if let Some(id) = data.actor_script(actor).and_then(|p| compiled.get(p)) {
                scripts.attach(unit, *id);
            }
            if is_player {
                // 이어 하기면 저장한 소지품이 들어오므로 시작 소지품은 주지 않는다.
                match options.save.as_ref() {
                    Some(save) => restore(&mut world, unit, save),
                    None => data.give_starting_kit(&mut world, unit),
                }
                player = Some(unit);
            }
        }
        let player = player.ok_or("플레이어 스폰이 없어 플레이할 수 없습니다")?;

        let mut auth = LocalAuthority::new(world);
        auth.set_script_host(Box::new(scripts));

        let mut session = Self {
            auth,
            player,
            labels,
            animators: HashMap::new(),
            order: Order::Idle,
            chase_goal: None,
            log: VecDeque::new(),
            alerts: Vec::new(),
            editor_camera,
            data,
        };
        session.note(String::from(
            "플레이 시작 — 클릭: 이동 · 적 클릭: 공격 · 아이템 클릭: 줍기",
        ));
        Ok(session)
    }

    /// 시작 전 에디터 카메라.
    pub(crate) fn editor_camera(&self) -> &Camera2d {
        &self.editor_camera
    }

    pub(crate) fn world(&self) -> &SimWorld {
        self.auth.world()
    }

    pub(crate) fn player(&self) -> Entity {
        self.player
    }

    pub(crate) fn ticks(&self) -> u64 {
        self.auth.ticks()
    }

    /// 유닛 이름. 모르면 `"?"`.
    pub(crate) fn name(&self, unit: Entity) -> &str {
        self.labels.get(&unit).map_or("?", |l| l.name.as_str())
    }

    /// 지금 상태를 저장 데이터로 (P3). `zone` 은 편집기가 열어 둔 존 파일이다.
    pub(crate) fn save_data(&self, zone: String) -> SaveData {
        let u = self.world().unit(self.player);
        let progress = u.map(|u| u.progress()).unwrap_or_default();
        let inventory = u.map(Unit::inventory);
        let bags = [BagKind::Consumable, BagKind::Equipment]
            .into_iter()
            .filter_map(|kind| inventory.map(|i| i.bag(kind)))
            .flat_map(|bag| {
                bag.iter()
                    .map(|(slot, s)| (slot as u16, s.item, s.count))
                    .collect::<Vec<_>>()
            })
            .collect();
        SaveData {
            actor: self
                .labels
                .get(&self.player)
                .map_or(ActorId::DEFAULT, |l| l.actor),
            level: progress.level(),
            exp: progress.exp(),
            // 쓰러진 채로 저장하면 이어 할 수 없다 — 최소 1 로 둔다.
            hp: u.map_or(1, |u| u.hp().max(1)),
            zone,
            pos: u.map(Unit::pos),
            bags,
            equipped: EquipSlot::ALL
                .into_iter()
                .filter_map(|slot| inventory?.equipped(slot))
                .collect(),
        }
    }

    /// 상태 바에 띄울 경고를 꺼낸다.
    pub(crate) fn take_alerts(&mut self) -> Vec<String> {
        std::mem::take(&mut self.alerts)
    }

    /// 최근 이벤트 (오래된 것부터).
    pub(crate) fn log(&self) -> impl Iterator<Item = &str> {
        self.log.iter().map(String::as_str)
    }

    /// 플레이어가 두 tick 사이 어디쯤 그려지는가 — 카메라가 따라간다.
    pub(crate) fn player_render_pos(&self, alpha: f32) -> Option<Vec2> {
        self.world().unit(self.player).map(|u| u.render_pos(alpha))
    }

    // ── 입력 ─────────────────────────────────────────────────────────────────

    /// 뷰포트 클릭. 적이면 공격, 땅의 아이템이면 줍기, 아니면 그 자리로 이동.
    ///
    /// `px` 는 화면 1픽셀의 월드 길이 — 멀리서 봐도 작은 대상을 누를 수 있게 한다.
    pub(crate) fn click(&mut self, at: Vec2, px: f32) {
        let world = self.auth.world();
        let Some(me) = world.unit(self.player).filter(|u| u.is_alive()) else {
            return;
        };
        let my_faction = me.def().faction;

        let nearest_unit = world
            .units()
            .filter(|(e, u)| *e != self.player && u.is_alive())
            .map(|(e, u)| (e, u.pos().distance(at)))
            .filter(|&(_, d)| d <= (14.0 * px).max(0.6))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let nearest_item = world
            .ground_items()
            .map(|(e, g)| (e, g.pos.distance(at)))
            .filter(|&(_, d)| d <= (10.0 * px).max(0.5))
            .min_by(|a, b| a.1.total_cmp(&b.1));

        self.chase_goal = None;
        if let Some((target, _)) = nearest_unit
            && world
                .unit(target)
                .is_some_and(|t| world.relation(my_faction, t.def().faction) != Relation::Friendly)
        {
            self.order = Order::Attack(target);
        } else if let Some((item, _)) = nearest_item {
            self.order = Order::PickUp(item);
        } else {
            self.order = Order::Idle;
            self.auth.submit(Intent::MoveTo {
                unit: self.player,
                target: at,
            });
        }
    }

    /// 인벤토리 패널 조작.
    pub(crate) fn inventory(&mut self, action: InventoryAction) {
        let unit = self.player;
        self.auth.submit(match action {
            InventoryAction::Use(slot) => Intent::UseItem { unit, slot },
            InventoryAction::Equip(slot) => Intent::Equip { unit, slot },
            InventoryAction::Unequip(slot) => Intent::Unequip { unit, slot },
        });
    }

    // ── 진행 ─────────────────────────────────────────────────────────────────

    /// 한 tick. **고정 timestep 에서만** 부른다 — 애니메이션도 여기서 진행한다.
    pub(crate) fn tick(&mut self, dt: Duration, sprites: Option<&SpriteLibrary>) {
        for intent in self.follow_order() {
            self.auth.submit(intent);
        }
        for event in self.auth.tick(dt) {
            self.on_event(event);
        }
        // 스크립트의 print 는 이벤트 기록으로, 오류는 상태 바로도 (그 스크립트는 꺼졌다).
        let lines = self
            .auth
            .script_host_mut()
            .map(|host| host.drain_log())
            .unwrap_or_default();
        for line in lines {
            if line.error {
                self.alerts
                    .push(format!("스크립트 오류 — {}", line.message));
            }
            self.note(line.message);
        }

        // 유닛마다 자기 액터 타입의 시트로 진행한다 — 시트마다 클립 길이가 다르다.
        let actors: Vec<(Entity, ActorId)> = self
            .labels
            .iter()
            .map(|(unit, label)| (*unit, label.actor))
            .collect();
        let world = self.auth.world();
        for (unit, actor) in actors {
            let Some(u) = world.unit(unit) else { continue };
            let animator = self.animators.entry(unit).or_default();
            if !u.is_alive() {
                continue; // 시체는 마지막 프레임에 멈춘다
            }
            animator.set_state(if u.is_moving() {
                AnimState::Walk
            } else {
                AnimState::Idle
            });
            if let Some(sprites) = sprites {
                animator.advance(sprites.sheet(actor).anim(), dt);
            }
        }
    }

    /// 주문을 이번 tick 의 Intent 로 바꾼다. 다가가기는 여기서 — 규칙(사거리·줍기 거리)은 월드에 묻는다.
    fn follow_order(&mut self) -> Vec<Intent> {
        let attack_skill = self.data.player_attack();
        let world = self.auth.world();
        let me = self.player;
        let Some(u) = world.unit(me).filter(|u| u.is_alive()) else {
            self.order = Order::Idle;
            return Vec::new();
        };

        let (goal, reach) = match self.order {
            Order::Idle => return Vec::new(),
            Order::Attack(target) => {
                let Some(t) = world.unit(target).filter(|t| t.is_alive()) else {
                    self.order = Order::Idle;
                    return Vec::new();
                };
                let range = world.skill(attack_skill).map_or(0.0, |s| s.range);
                (t.pos(), range)
            }
            Order::PickUp(item) => {
                let Some(g) = world.ground_item(item) else {
                    self.order = Order::Idle;
                    return Vec::new();
                };
                (g.pos, world.pickup_range())
            }
        };

        let mut out = Vec::new();
        if u.pos().distance(goal) <= reach {
            if u.is_moving() {
                out.push(Intent::Stop { unit: me });
            }
            self.chase_goal = None;
            match self.order {
                Order::Attack(target) if u.is_ready(attack_skill, world.now()) => {
                    out.push(Intent::Attack {
                        unit: me,
                        target,
                        skill: attack_skill,
                    });
                }
                Order::PickUp(item) => {
                    out.push(Intent::PickUp { unit: me, item });
                    self.order = Order::Idle;
                }
                _ => {}
            }
        } else if self
            .chase_goal
            .is_none_or(|g| g.distance(goal) > REPATH_DISTANCE)
            || !u.is_moving()
        {
            self.chase_goal = Some(goal);
            out.push(Intent::MoveTo {
                unit: me,
                target: goal,
            });
        }
        out
    }

    fn on_event(&mut self, event: Event) {
        // 노리던 대상이 쓰러지면 그 tick 안에 주문을 끝낸다 — 다음 tick 까지 남겨 두면
        // 쓰러진 대상 쪽으로 한 걸음 더 내디딘다.
        if let Event::Died { unit, .. } = event
            && self.order == Order::Attack(unit)
        {
            self.order = Order::Idle;
            self.chase_goal = None;
        }
        let line = match event {
            Event::Damaged {
                attacker,
                target,
                amount,
                remaining_hp,
                ..
            } => format!(
                "{} → {} {amount} 피해 (HP {remaining_hp})",
                self.name(attacker),
                self.name(target)
            ),
            Event::Died { unit, .. } if unit == self.player => {
                String::from("플레이어가 쓰러졌습니다")
            }
            Event::Died { unit, .. } => format!("{} 쓰러짐", self.name(unit)),
            Event::ItemSpawned { stack, .. } => {
                format!(
                    "{} ×{} 떨어짐",
                    self.data.item_name(stack.item),
                    stack.count
                )
            }
            Event::PickedUp { stack, .. } => {
                format!("{} ×{} 획득", self.data.item_name(stack.item), stack.count)
            }
            Event::ExperienceGained {
                unit,
                amount,
                progress,
            } if unit == self.player => format!(
                "경험치 +{amount} (레벨 {} · {}/{})",
                progress.level(),
                progress.exp(),
                progress.exp_to_next()
            ),
            Event::LeveledUp { unit, level } if unit == self.player => {
                format!("레벨 {level} 달성 — HP 가 가득 찼다")
            }
            Event::Healed {
                amount,
                remaining_hp,
                ..
            } => {
                format!("HP +{amount} (HP {remaining_hp})")
            }
            Event::EquipmentChanged { unit, slot } => {
                let now = self
                    .world()
                    .unit(unit)
                    .and_then(|u| u.inventory().equipped(slot));
                match now {
                    Some(item) => format!("{} 장착", self.data.item_name(item)),
                    None => format!("{} 해제", text::slot_name(slot)),
                }
            }
            Event::Engaged { unit, target } => {
                format!(
                    "{} 이(가) {} 을(를) 노린다",
                    self.name(unit),
                    self.name(target)
                )
            }
            Event::Evading { unit } => format!("{} 추격 포기", self.name(unit)),
            Event::Blocked { unit } if unit == self.player => String::from("길이 막혔습니다"),
            Event::Rejected { intent, reason } if intent.unit() == self.player => {
                // 쿨타임은 주문이 계속 기다리면 되는 일이다. 그 밖의 거절은 주문을 끝낸다
                // — 그대로 두면 매 tick 같은 거절이 반복된다.
                if reason == Rejection::OnCooldown {
                    return;
                }
                self.order = Order::Idle;
                self.chase_goal = None;
                format!("할 수 없음: {}", text::rejection(reason))
            }
            _ => return,
        };
        self.note(line);
    }

    fn note(&mut self, line: String) {
        if self.log.len() == LOG_LINES {
            self.log.pop_front();
        }
        self.log.push_back(line);
    }

    // ── 패널 ─────────────────────────────────────────────────────────────────

    /// 플레이어 가방 내용 — UI 가 그대로 그린다.
    pub(crate) fn bag_lines(&self, kind: BagKind) -> Vec<BagLine> {
        let Some(me) = self.world().unit(self.player) else {
            return Vec::new();
        };
        me.inventory()
            .bag(kind)
            .iter()
            .filter_map(|(slot, s)| {
                Some(BagLine {
                    slot: u16::try_from(slot).ok()?,
                    name: self.data.item_name(s.item),
                    count: s.count,
                })
            })
            .collect()
    }

    /// HUD 인벤토리 칸 — `(아이템 색, 개수)`, 소모품 가방 다음 장비 가방 (P5).
    ///
    /// HUD 는 글자 대신 색으로 아이템을 보여 준다 — 폰트에 한글이 없기 때문이다.
    pub(crate) fn hud_slots(&self) -> Vec<([f32; 4], u32)> {
        let Some(me) = self.world().unit(self.player) else {
            return Vec::new();
        };
        [BagKind::Consumable, BagKind::Equipment]
            .into_iter()
            .flat_map(|kind| {
                me.inventory()
                    .bag(kind)
                    .iter()
                    .map(|(_, s)| (self.data.item_color(s.item), s.count))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// 장착 자리마다 (자리, 자리 이름, 장착한 아이템 이름).
    pub(crate) fn equipped_lines(&self) -> Vec<(EquipSlot, &'static str, Option<String>)> {
        let me = self.world().unit(self.player);
        EquipSlot::ALL
            .into_iter()
            .map(|slot| {
                let item = me
                    .and_then(|u| u.inventory().equipped(slot))
                    .map(|i| self.data.item_name(i));
                (slot, text::slot_name(slot), item)
            })
            .collect()
    }

    // ── 그리기 ───────────────────────────────────────────────────────────────

    /// 지면 층 — 땅의 아이템, 이동 경로, 공격 대상 표시.
    pub(crate) fn build_ground(
        &self,
        camera: &Camera2d,
        alpha: f32,
        px: f32,
        out: &mut Vec<RenderCommand>,
    ) {
        let world = self.world();

        for (_, g) in world.ground_items() {
            let size = (10.0 * px).max(0.4);
            let diamond = std::f32::consts::FRAC_PI_4;
            for (grow, bias, color) in [
                (2.0 * px, BIAS_ITEM, BAR_BACK),
                (
                    0.0,
                    BIAS_ITEM + DEPTH_LAYER,
                    self.data.item_color(g.stack.item),
                ),
            ] {
                out.push(RenderCommand::DrawRect {
                    center: g.pos,
                    size: Vec2::splat(size + grow),
                    rotation: diamond,
                    z: 0.0,
                    depth_bias: bias,
                    color,
                    uv: UvRect::FULL,
                    texture: TextureId::WHITE,
                });
            }
        }

        if let Some(me) = world.unit(self.player) {
            // 남은 경로 — 지금 그려지는 위치에서 시작한다.
            let mut from = me.render_pos(alpha);
            for to in me.waypoints() {
                crate::push_segment(from, to, 2.0 * px, BIAS_PATH, PATH_COLOR, out);
                from = to;
            }
        }

        if let Order::Attack(target) = self.order
            && let Some(t) = world.unit(target)
        {
            let half = Vec2::splat((12.0 * px).max(0.5));
            let at = t.render_pos(alpha);
            crate::grid::build_outline(
                camera,
                at - half,
                at + half,
                2.0,
                BIAS_TARGET,
                TARGET_COLOR,
                out,
            );
        }
    }

    /// 오브젝트 층 — 유닛 스프라이트.
    pub(crate) fn build_objects(
        &self,
        alpha: f32,
        px: f32,
        sprites: &SpriteLibrary,
        out: &mut Vec<RenderCommand>,
    ) {
        let idle_animator = SpriteAnimator::default();
        for (unit, u) in self.world().units() {
            let actor = self.actor_of(unit);
            // 시체는 어떤 시트든 회색 — 명시한 색으로 넘겨 컬러 아트에도 곱해지게 한다.
            let (tint, fallback) = match self.labels.get(&unit) {
                _ if !u.is_alive() => (CORPSE_TINT, CORPSE_TINT),
                Some(l) => (l.tint, l.fallback),
                None => (NO_TINT, NO_TINT),
            };
            let look = Look {
                heading: u.heading(),
                tint,
                fallback,
            };
            let animator = self.animators.get(&unit).unwrap_or(&idle_animator);
            sprites
                .sheet(actor)
                .push(u.render_pos(alpha), look, animator, px, BIAS_SPRITE, out);
        }
    }

    /// 이 유닛의 액터 타입. 모르면 기본값 — 내장 시트로 그려진다.
    fn actor_of(&self, unit: Entity) -> ActorId {
        self.labels.get(&unit).map_or(ActorId::DEFAULT, |l| l.actor)
    }

    /// 표시 층 — 살아 있는 유닛 머리 위 HP 막대.
    ///
    /// 막대는 **빌보드**(흰 텍스처 스프라이트)다 — 화면을 향해 서므로 쿼터뷰에서도 눌리지 않는다.
    /// 막대가 뜨는 높이는 **유닛이 쓰는 시트**의 스프라이트 높이다 — 종류마다 칸 크기가 다르다.
    pub(crate) fn build_overlay(
        &self,
        alpha: f32,
        px: f32,
        sprites: Option<&SpriteLibrary>,
        out: &mut Vec<RenderCommand>,
    ) {
        const WIDTH_PX: f32 = 30.0;
        const HEIGHT_PX: f32 = 4.0;
        const GAP_PX: f32 = 6.0;
        /// 시트가 없을 때(로드 실패) 쓰는 머리 높이 (m).
        const NO_SHEET_HEIGHT: f32 = 1.5;

        for (unit, u) in self.world().units().filter(|(_, u)| u.is_alive()) {
            let height =
                sprites.map_or(NO_SHEET_HEIGHT, |s| s.sheet(self.actor_of(unit)).height(px));
            let above = height + GAP_PX * px;
            let at = u.render_pos(alpha);
            let ratio = u.hp() as f32 / u.max_hp() as f32;
            let full = WIDTH_PX * px;
            let color = if ratio <= 0.3 { BAR_HP_LOW } else { BAR_HP };

            push_bar(
                Vec3::new(at.x, at.y, above),
                full + 2.0 * px,
                (HEIGHT_PX + 2.0) * px,
                0.0,
                BAR_BACK,
                out,
            );
            // 채움은 왼쪽 정렬 — 가운데에서 모자란 만큼 왼쪽으로 민다 (카메라 right 가 +X).
            let fill = full * ratio;
            let shift = (full - fill) * 0.5;
            push_bar(
                Vec3::new(at.x - shift, at.y, above),
                fill,
                HEIGHT_PX * px,
                DEPTH_LAYER,
                color,
                out,
            );
        }
    }
}

/// 흰 텍스처 빌보드 하나 — 막대의 한 층.
fn push_bar(
    center: Vec3,
    width: f32,
    height: f32,
    bias: f32,
    color: [f32; 4],
    out: &mut Vec<RenderCommand>,
) {
    if width <= 0.0 {
        return;
    }
    out.push(RenderCommand::DrawSprite {
        pos: center,
        size: Vec2::new(width, height),
        anchor: SpriteAnchor::Center,
        depth_bias: BIAS_BAR + bias,
        uv: UvRect::FULL,
        texture: TextureId::WHITE,
        tint: color,
    });
}

/// 인벤토리 패널이 보여 줄 한 줄.
#[derive(Clone, Debug)]
pub(crate) struct BagLine {
    pub(crate) slot: u16,
    pub(crate) name: String,
    pub(crate) count: u32,
}

/// 화면 문구 — 데이터가 아니라 UI 의 일부라 코드에 둔다.
mod text {
    use nexus_sim::{EquipSlot, Rejection};

    pub(super) fn slot_name(slot: EquipSlot) -> &'static str {
        match slot {
            EquipSlot::Weapon => "무기",
            EquipSlot::Head => "머리",
            EquipSlot::Body => "몸",
            EquipSlot::Hand => "손",
            EquipSlot::Shoes => "신발",
        }
    }

    pub(super) fn rejection(reason: Rejection) -> &'static str {
        match reason {
            Rejection::UnknownEntity | Rejection::UnknownItem => "대상이 없습니다",
            Rejection::NoPath => "갈 수 없는 곳입니다",
            Rejection::Immobile => "움직일 수 없습니다",
            Rejection::Dead => "쓰러진 상태입니다",
            Rejection::TargetDead => "대상이 이미 쓰러졌습니다",
            Rejection::UnknownSkill => "모르는 스킬입니다",
            Rejection::OnCooldown => "아직 쓸 수 없습니다",
            Rejection::OutOfRange => "너무 멉니다",
            Rejection::InvalidTarget => "그 대상은 고를 수 없습니다",
            Rejection::Invulnerable => "공격할 수 없는 대상입니다",
            Rejection::Friendly => "우호 대상입니다",
            Rejection::EmptySlot => "빈 칸입니다",
            Rejection::InventoryFull => "가방이 가득 찼습니다",
            Rejection::NotUsable => "쓸 수 없는 아이템입니다",
        }
    }
}

/// 저장 데이터를 스폰한 플레이어에 얹는다 (P3).
///
/// **수치는 되살리지 않는다** — HP 상한·공격력은 `rules.ron` 의 지금 값에서 다시 계산된다.
/// 데이터에서 사라진 아이템은 조용히 버린다 (규칙이 바뀌었다고 이어 하기가 막히면 곤란하다).
fn restore(world: &mut SimWorld, player: Entity, save: &SaveData) {
    world.set_progress(player, Progress::new(save.level, save.exp));
    for &(slot, item, count) in &save.bags {
        world.place_item(player, slot, ItemStack { item, count });
    }
    for &item in &save.equipped {
        world.force_equip(player, item);
    }
    // 위치는 같은 존일 때만 남아 있다 — 다른 존이면 스폰 지점 그대로다.
    if let Some(pos) = save.pos {
        world.set_position(player, pos);
    }
    world.set_hp(player, save.hp);
}
#[cfg(test)]
mod tests {
    use nexus_sim::ItemId;

    use super::*;

    const DT: Duration = Duration::from_millis(50);

    fn session() -> PlaySession {
        PlaySession::start(
            &Scene::server_default(),
            Camera2d::default(),
            GameData::embedded(),
            PlayOptions::default(),
        )
        .unwrap()
    }

    fn find(s: &PlaySession, name: &str) -> Entity {
        s.labels
            .iter()
            .find(|(_, l)| l.name == name)
            .map(|(e, _)| *e)
            .unwrap()
    }

    #[test]
    fn scene_becomes_one_player_and_all_npcs_and_monsters() {
        let s = session();
        // 기본 씬: 플레이어 스폰 2 · NPC 2 · 몬스터 1 → 플레이어 1명 + 3
        assert_eq!(s.world().unit_count(), 4);
        assert_eq!(s.name(s.player()), "플레이어");
        let me = s.world().unit(s.player()).unwrap();
        assert_eq!(me.pos(), Vec2::ZERO, "첫 플레이어 스폰에 선다");
        assert_eq!(
            me.inventory()
                .bag(BagKind::Consumable)
                .count_of(ItemId(501)),
            3
        );
    }

    #[test]
    fn starting_does_not_touch_the_scene() {
        let scene = Scene::server_default();
        let before = scene.items.clone();
        let mut s = PlaySession::start(
            &scene,
            Camera2d::default(),
            GameData::embedded(),
            PlayOptions::default(),
        )
        .unwrap();
        s.click(Vec2::new(5.0, 0.0), 0.01);
        for _ in 0..40 {
            s.tick(DT, None);
        }
        assert_eq!(scene.items, before);
    }

    #[test]
    fn a_scripted_goblin_spots_the_player_and_charges() {
        // P2: 행동은 전부 data/scripts/goblin.rhai 가 정한다 (AI 는 Passive).
        let mut scene = Scene::server_default();
        let slime = scene.items.iter_mut().find(|i| i.name == "슬라임").unwrap();
        slime.actor = ActorId::new(102);
        slime.pos = Vec2::new(6.0, 0.0);
        let mut s = PlaySession::start(
            &scene,
            Camera2d::default(),
            GameData::embedded(),
            PlayOptions::default(),
        )
        .unwrap();
        let goblin = find(&s, "슬라임");
        let full = s.world().unit(s.player()).unwrap().hp();

        for _ in 0..80 {
            s.tick(DT, None);
        }
        assert!(
            s.log().any(|l| l == "scripts/goblin.rhai: 적 발견 — 돌진!"),
            "{:?}",
            s.log
        );
        let me = s.world().unit(s.player()).unwrap();
        assert!(me.hp() < full, "고블린이 물었어야 한다");
        assert!(s.world().unit(goblin).unwrap().pos().distance(me.pos()) <= 1.5);
        assert!(s.take_alerts().is_empty());
    }

    // ── 이어 하기 (P3) ──────────────────────────────────────────────────────

    #[test]
    fn the_chosen_player_spawn_is_the_one_that_walks() {
        // P3-1: 여러 플레이어 스폰 중 고른 마커에서 시작한다.
        let scene = Scene::server_default();
        let spawns: Vec<_> = scene
            .items
            .iter()
            .filter(|i| i.kind == ItemKind::PlayerSpawn)
            .collect();
        assert!(spawns.len() >= 2, "시험 전제: 플레이어 스폰이 둘 이상");
        let (second, at) = (spawns[1].entity, spawns[1].pos);

        let s = PlaySession::start(
            &scene,
            Camera2d::default(),
            GameData::embedded(),
            PlayOptions {
                spawn: Some(second),
                save: None,
            },
        )
        .unwrap();
        assert_eq!(s.world().unit(s.player()).unwrap().pos(), at);
        // 스폰 지점은 여전히 하나만 유닛이 된다.
        assert_eq!(s.world().unit_count(), 4);
    }

    #[test]
    fn a_saved_character_comes_back_with_level_items_and_place() {
        let mut s = session();
        // 슬라임을 잡아 경험치를 얻고, 떨어진 것을 줍는다.
        let slime = find(&s, "슬라임");
        s.click(s.world().unit(slime).unwrap().pos(), 0.01);
        for _ in 0..900 {
            s.tick(DT, None);
        }
        assert!(!s.world().unit(slime).unwrap().is_alive(), "{:?}", s.log);
        let before = s.world().unit(s.player()).unwrap().progress();
        assert!(
            before.level() >= 1 && before.exp() > 0,
            "경험치를 받았어야 한다"
        );

        let save = s.save_data(String::from("zones/sample.zone.ron"));
        assert_eq!((save.level, save.exp), (before.level(), before.exp()));
        assert!(!save.bags.is_empty(), "시작 소지품이 저장에 담긴다");

        // 같은 존에서 이어 하기 — 레벨·소지품·위치가 그대로다.
        let again = PlaySession::start(
            &Scene::server_default(),
            Camera2d::default(),
            GameData::embedded(),
            PlayOptions {
                spawn: None,
                save: Some(save.clone()),
            },
        )
        .unwrap();
        let me = again.world().unit(again.player()).unwrap();
        assert_eq!(me.progress(), before);
        assert_eq!(me.pos(), save.pos.unwrap(), "저장한 자리에서 이어 한다");
        assert_eq!(me.hp(), save.hp);
        assert_eq!(
            me.inventory()
                .bag(BagKind::Consumable)
                .count_of(ItemId(501)),
            save.bags
                .iter()
                .filter(|(_, item, _)| *item == ItemId(501))
                .map(|(_, _, n)| n)
                .sum::<u32>()
        );
    }

    #[test]
    fn continuing_in_another_zone_keeps_the_character_but_not_the_place() {
        let mut save = session().save_data(String::from("zones/other.zone.ron"));
        save.level = 4;
        save.hp = 7;
        save.pos = None; // 편집기가 다른 존이라 지운 상태
        let s = PlaySession::start(
            &Scene::server_default(),
            Camera2d::default(),
            GameData::embedded(),
            PlayOptions {
                spawn: None,
                save: Some(save),
            },
        )
        .unwrap();
        let me = s.world().unit(s.player()).unwrap();
        assert_eq!(me.progress().level(), 4);
        assert_eq!(me.hp(), 7);
        assert_eq!(me.pos(), Vec2::ZERO, "스폰 지점에서 시작한다");
        // 레벨 4 = 성장치 ×3 (rules.ron: HP +40/레벨).
        assert_eq!(me.max_hp(), 200 + 40 * 3);
    }

    #[test]
    fn items_that_no_longer_exist_are_dropped_instead_of_blocking_play() {
        let mut save = session().save_data(String::new());
        save.bags.push((5, ItemId(9_999), 1));
        save.equipped.push(ItemId(9_998));
        let s = PlaySession::start(
            &Scene::server_default(),
            Camera2d::default(),
            GameData::embedded(),
            PlayOptions {
                spawn: None,
                save: Some(save),
            },
        )
        .expect("없는 아이템 때문에 플레이가 막히면 안 된다");
        let inv = s.world().unit(s.player()).unwrap().inventory();
        assert_eq!(inv.bag(BagKind::Equipment).get(5), None);
    }

    #[test]
    fn marker_overrides_reach_the_spawned_unit() {
        // P1-4: 같은 타입이라도 이 마커의 슬라임만 HP 가 다르다. 나머지 수치는 타입 그대로.
        let mut scene = Scene::server_default();
        let slime = scene.items.iter_mut().find(|i| i.name == "슬라임").unwrap();
        slime.overrides.max_hp = Some(333);
        let s = PlaySession::start(
            &scene,
            Camera2d::default(),
            GameData::embedded(),
            PlayOptions::default(),
        )
        .unwrap();
        let plain = PlaySession::start(
            &Scene::server_default(),
            Camera2d::default(),
            GameData::embedded(),
            PlayOptions::default(),
        )
        .unwrap();

        let unit = s.world().unit(find(&s, "슬라임")).unwrap();
        let base = plain.world().unit(find(&plain, "슬라임")).unwrap();
        assert_eq!((unit.hp(), unit.def().max_hp), (333, 333));
        assert_eq!(unit.def().attack, base.def().attack);
        assert_ne!(base.def().max_hp, 333);
    }

    #[test]
    fn no_player_spawn_means_no_play() {
        let mut scene = Scene::server_default();
        scene.items.retain(|i| i.kind != ItemKind::PlayerSpawn);
        assert!(
            PlaySession::start(
                &scene,
                Camera2d::default(),
                GameData::embedded(),
                PlayOptions::default()
            )
            .is_err()
        );
    }

    #[test]
    fn clicking_the_ground_walks_there() {
        let mut s = session();
        let goal = Vec2::new(-4.0, 3.0);
        s.click(goal, 0.01);
        for _ in 0..60 {
            s.tick(DT, None);
        }
        assert_eq!(s.world().unit(s.player()).unwrap().pos(), goal);
    }

    #[test]
    fn clicking_a_monster_walks_up_and_fights_it_to_the_death() {
        let mut s = session();
        let slime = find(&s, "슬라임");
        let at = s.world().unit(slime).unwrap().pos();
        s.click(at, 0.01);
        assert_eq!(s.order, Order::Attack(slime));

        let mut dead = false;
        for _ in 0..(20 * 60) {
            s.tick(DT, None);
            if !s.world().unit(slime).unwrap().is_alive() {
                dead = true;
                break;
            }
        }
        assert!(dead, "슬라임을 쓰러뜨리지 못했다: {:?}", s.log);
        assert!(s.world().unit(s.player()).unwrap().is_alive());
        assert_eq!(s.order, Order::Idle, "대상이 죽으면 주문이 끝난다");
    }

    #[test]
    fn loot_can_be_clicked_and_picked_up() {
        let mut s = session();
        let slime = find(&s, "슬라임");
        let at = s.world().unit(slime).unwrap().pos();
        s.click(at, 0.01);
        for _ in 0..(20 * 60) {
            s.tick(DT, None);
            if s.world().ground_items().count() > 0 {
                break;
            }
        }
        let (_, drop) = s
            .world()
            .ground_items()
            .next()
            .expect("시드 1 이면 젤리가 나온다");
        let (pos, stack) = (drop.pos, drop.stack);

        s.click(pos, 0.01);
        for _ in 0..100 {
            s.tick(DT, None);
        }
        assert_eq!(
            s.world().ground_items().count(),
            0,
            "줍지 않았다: {:?}",
            s.log
        );
        let me = s.world().unit(s.player()).unwrap();
        assert!(me.inventory().bag(BagKind::Consumable).count_of(stack.item) >= stack.count);
    }

    #[test]
    fn clicking_an_immortal_npc_is_refused_once_not_every_tick() {
        let mut s = session();
        let guard = find(&s, "마을 경비병");
        // 경비병 옆으로 붙인다 — 사거리 안에서 거절이 나야 한다.
        let at = s.world().unit(guard).unwrap().pos();
        s.click(at + Vec2::new(1.0, 0.0), 0.01);
        for _ in 0..200 {
            s.tick(DT, None);
        }
        s.click(at, 0.01);
        for _ in 0..40 {
            s.tick(DT, None);
        }
        let refusals = s.log().filter(|l| l.starts_with("할 수 없음")).count();
        assert_eq!(refusals, 1, "{:?}", s.log);
        assert_eq!(s.order, Order::Idle);
    }

    #[test]
    fn inventory_panel_equips_the_starting_sword() {
        let mut s = session();
        let base = s.world().unit(s.player()).unwrap().attack();
        s.inventory(InventoryAction::Equip(0));
        s.tick(DT, None);
        assert_eq!(s.world().unit(s.player()).unwrap().attack(), base + 12);
        assert!(s.log().any(|l| l == "숏소드 장착"));
    }
}
