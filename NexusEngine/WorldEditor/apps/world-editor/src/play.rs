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
//! - 규칙 수치는 [`rules`] 에 있다 — **예시 데이터**이며 S7-2 에서 파일로 옮긴다.

use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use nexus_assets::{AnimState, SpriteAnimator, SpriteSheet};
use nexus_core::{Camera2d, Entity, Vec2, Vec3};
use nexus_render::{DEPTH_LAYER, RenderCommand, SpriteAnchor, TextureId, UvRect};
use nexus_sim::{
    Authority, BagKind, EquipSlot, Event, Intent, LocalAuthority, Rejection, Relation, SimWorld,
    Unit,
};

use crate::scene::{ItemKind, Scene};
use crate::sprites::{Look, MarkerSprites};

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
    tint: [f32; 4],
}

/// 인벤토리 패널에서 누른 것.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InventoryAction {
    Use(u16),
    Equip(u16),
    Unequip(EquipSlot),
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
    /// 시작 전 에디터 카메라 — 정지하면 되돌린다.
    editor_camera: Camera2d,
}

impl PlaySession {
    /// 씬에서 시뮬레이션을 만든다. **첫 플레이어 스폰**에 플레이어가 서고, 나머지 플레이어
    /// 스폰은 스폰 지점일 뿐이라 유닛을 만들지 않는다.
    ///
    /// # Errors
    /// 플레이어 스폰이 없으면 플레이할 수 없다.
    pub(crate) fn start(scene: &Scene, editor_camera: Camera2d) -> Result<Self, String> {
        let mut world = SimWorld::new(scene.tiles.clone());
        rules::install(&mut world);

        let mut labels = HashMap::new();
        let mut player = None;
        for item in &scene.items {
            let is_player = item.kind == ItemKind::PlayerSpawn;
            if is_player && player.is_some() {
                continue;
            }
            let unit = world.spawn_unit(item.pos, item.orientation, rules::unit_def(item.kind));
            let name = if is_player {
                String::from("플레이어")
            } else {
                item.name.clone()
            };
            labels.insert(
                unit,
                Label {
                    name,
                    tint: item.kind.color(),
                },
            );
            if is_player {
                rules::starting_kit(&mut world, unit);
                player = Some(unit);
            }
        }
        let player = player.ok_or("플레이어 스폰이 없어 플레이할 수 없습니다")?;

        let mut session = Self {
            auth: LocalAuthority::new(world),
            player,
            labels,
            animators: HashMap::new(),
            order: Order::Idle,
            chase_goal: None,
            log: VecDeque::new(),
            editor_camera,
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
    pub(crate) fn tick(&mut self, dt: Duration, sheet: Option<&SpriteSheet>) {
        for intent in self.follow_order() {
            self.auth.submit(intent);
        }
        for event in self.auth.tick(dt) {
            self.on_event(event);
        }

        let world = self.auth.world();
        for (unit, u) in world.units() {
            let animator = self.animators.entry(unit).or_default();
            if !u.is_alive() {
                continue; // 시체는 마지막 프레임에 멈춘다
            }
            animator.set_state(if u.is_moving() {
                AnimState::Walk
            } else {
                AnimState::Idle
            });
            if let Some(sheet) = sheet {
                animator.advance(sheet, dt);
            }
        }
    }

    /// 주문을 이번 tick 의 Intent 로 바꾼다. 다가가기는 여기서 — 규칙(사거리·줍기 거리)은 월드에 묻는다.
    fn follow_order(&mut self) -> Vec<Intent> {
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
                let range = world.skill(rules::PLAYER_ATTACK).map_or(0.0, |s| s.range);
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
                Order::Attack(target) if u.is_ready(rules::PLAYER_ATTACK, world.now()) => {
                    out.push(Intent::Attack {
                        unit: me,
                        target,
                        skill: rules::PLAYER_ATTACK,
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
                format!("{} ×{} 떨어짐", rules::item_name(stack.item), stack.count)
            }
            Event::PickedUp { stack, .. } => {
                format!("{} ×{} 획득", rules::item_name(stack.item), stack.count)
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
                    Some(item) => format!("{} 장착", rules::item_name(item)),
                    None => format!("{} 해제", rules::slot_name(slot)),
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
                format!("할 수 없음: {}", rules::rejection_text(reason))
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
                    rules::item_color(g.stack.item),
                ),
            ] {
                out.push(RenderCommand::DrawRect {
                    center: g.pos,
                    size: Vec2::splat(size + grow),
                    rotation: diamond,
                    z: 0.0,
                    depth_bias: bias,
                    color,
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
        sprites: &MarkerSprites,
        out: &mut Vec<RenderCommand>,
    ) {
        let fallback = SpriteAnimator::default();
        for (unit, u) in self.world().units() {
            let tint = if u.is_alive() {
                self.labels.get(&unit).map_or([1.0; 4], |l| l.tint)
            } else {
                CORPSE_TINT
            };
            let look = Look {
                heading: u.heading(),
                tint,
            };
            let animator = self.animators.get(&unit).unwrap_or(&fallback);
            sprites.push(u.render_pos(alpha), look, animator, px, BIAS_SPRITE, out);
        }
    }

    /// 표시 층 — 살아 있는 유닛 머리 위 HP 막대.
    ///
    /// 막대는 **빌보드**(흰 텍스처 스프라이트)다 — 화면을 향해 서므로 쿼터뷰에서도 눌리지 않는다.
    pub(crate) fn build_overlay(&self, alpha: f32, px: f32, out: &mut Vec<RenderCommand>) {
        const WIDTH_PX: f32 = 30.0;
        const HEIGHT_PX: f32 = 4.0;
        const GAP_PX: f32 = 6.0;

        let above = MarkerSprites::height(px) + GAP_PX * px;
        for (_, u) in self.world().units().filter(|(_, u)| u.is_alive()) {
            let at = u.render_pos(alpha);
            let ratio = u.hp() as f32 / u.def().max_hp as f32;
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
    pub(crate) name: &'static str,
    pub(crate) count: u32,
}

/// 플레이어 가방 내용 — UI 가 그대로 그린다.
pub(crate) fn bag_lines(unit: &Unit, kind: BagKind) -> Vec<BagLine> {
    unit.inventory()
        .bag(kind)
        .iter()
        .filter_map(|(slot, s)| {
            Some(BagLine {
                slot: u16::try_from(slot).ok()?,
                name: rules::item_name(s.item),
                count: s.count,
            })
        })
        .collect()
}

/// 장착 자리 이름과 그 자리의 아이템 이름.
pub(crate) fn equipped_lines(unit: &Unit) -> Vec<(EquipSlot, &'static str, Option<&'static str>)> {
    EquipSlot::ALL
        .into_iter()
        .map(|slot| {
            let item = unit.inventory().equipped(slot).map(rules::item_name);
            (slot, rules::slot_name(slot), item)
        })
        .collect()
}

/// 예시 게임 데이터 — **코드가 아니라 데이터가 될 자리다** (S7-2 에서 파일로).
///
/// 규칙(`nexus-sim`)은 수치를 모르고, 여기서 넘긴 수치로만 동작한다.
pub(crate) mod rules {
    use nexus_sim::{
        AiKind, EquipSlot, FactionId, ItemDef, ItemId, ItemKind as SimItem, LootEntry, LootTableId,
        Rejection, Relation, SimWorld, SkillDef, SkillId, UnitDef,
    };

    use crate::scene::ItemKind;

    const PLAYERS: FactionId = FactionId(1);
    const TOWN: FactionId = FactionId(100);
    const WILD: FactionId = FactionId(201);

    /// 플레이어 기본 공격 (근접).
    pub(crate) const PLAYER_ATTACK: SkillId = SkillId(1);
    const SLIME_BITE: SkillId = SkillId(2);
    const GUARD_STRIKE: SkillId = SkillId(3);

    const POTION: ItemId = ItemId(501);
    const JELLY: ItemId = ItemId(909);
    const SHORT_SWORD: ItemId = ItemId(1101);
    const CAP: ItemId = ItemId(2101);

    const SLIME_LOOT: LootTableId = LootTableId(1);

    pub(crate) fn install(world: &mut SimWorld) {
        world.set_seed(1);
        world.set_relation(PLAYERS, WILD, Relation::Hostile);

        let skill = |range, cooldown_ms, damage_mult| SkillDef {
            range,
            cooldown_ms,
            damage_mult,
        };
        world.define_skill(PLAYER_ATTACK, skill(2.0, 800, 1.0));
        world.define_skill(SLIME_BITE, skill(1.5, 1200, 1.0));
        world.define_skill(GUARD_STRIKE, skill(2.0, 1000, 1.2));

        let consumable = |heal, max_stack| ItemDef {
            kind: SimItem::Consumable { heal },
            max_stack,
        };
        let gear = |slot, attack, defense| ItemDef {
            kind: SimItem::Equipment {
                slot,
                attack,
                defense,
            },
            max_stack: 1,
        };
        world.define_item(POTION, consumable(60, 20));
        world.define_item(JELLY, consumable(15, 99));
        world.define_item(SHORT_SWORD, gear(EquipSlot::Weapon, 12, 0));
        world.define_item(CAP, gear(EquipSlot::Head, 0, 4));

        let entry = |item, count, chance_per_mille| LootEntry {
            item,
            count,
            chance_per_mille,
        };
        world.define_loot(
            SLIME_LOOT,
            vec![
                entry(JELLY, 1, 800),
                entry(POTION, 1, 250),
                entry(CAP, 1, 150),
            ],
        );
    }

    /// 마커 종류별 유닛 수치.
    pub(crate) fn unit_def(kind: ItemKind) -> UnitDef {
        match kind {
            ItemKind::PlayerSpawn => UnitDef {
                move_speed: 4.0,
                max_hp: 200,
                attack: 20,
                defense: 5,
                faction: PLAYERS,
                basic_attack: Some(PLAYER_ATTACK),
                ..UnitDef::default()
            },
            // 서버 NpcEntityData 기본값처럼 불사·방어형.
            ItemKind::Npc => UnitDef {
                move_speed: 1.5,
                max_hp: 300,
                attack: 25,
                defense: 10,
                immortal: true,
                faction: TOWN,
                ai: AiKind::Defensive,
                aggro_range: 6.0,
                leash_range: 10.0,
                basic_attack: Some(GUARD_STRIKE),
                ..UnitDef::default()
            },
            // 서버 MonsterEntityData 기본값처럼 공격형.
            ItemKind::Monster => UnitDef {
                move_speed: 2.0,
                max_hp: 80,
                attack: 18,
                defense: 2,
                faction: WILD,
                ai: AiKind::Aggressive,
                aggro_range: 6.0,
                leash_range: 14.0,
                basic_attack: Some(SLIME_BITE),
                loot: Some(SLIME_LOOT),
                ..UnitDef::default()
            },
        }
    }

    /// 플레이어 시작 소지품.
    pub(crate) fn starting_kit(world: &mut SimWorld, player: nexus_core::Entity) {
        world.give_item(player, POTION, 3);
        world.give_item(player, SHORT_SWORD, 1);
    }

    pub(crate) fn item_name(id: ItemId) -> &'static str {
        match id {
            POTION => "빨간 포션",
            JELLY => "젤리",
            SHORT_SWORD => "숏소드",
            CAP => "모자",
            _ => "알 수 없는 아이템",
        }
    }

    /// 땅에 떨어졌을 때의 색 (sRGB).
    pub(crate) fn item_color(id: ItemId) -> [f32; 4] {
        match id {
            POTION => [0.95, 0.30, 0.30, 1.0],
            JELLY => [0.55, 0.85, 0.95, 1.0],
            SHORT_SWORD | CAP => [0.95, 0.85, 0.40, 1.0],
            _ => [0.8, 0.8, 0.8, 1.0],
        }
    }

    pub(crate) fn slot_name(slot: EquipSlot) -> &'static str {
        match slot {
            EquipSlot::Weapon => "무기",
            EquipSlot::Head => "머리",
            EquipSlot::Body => "몸",
            EquipSlot::Hand => "손",
            EquipSlot::Shoes => "신발",
        }
    }

    pub(crate) fn rejection_text(reason: Rejection) -> &'static str {
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

#[cfg(test)]
mod tests {
    use nexus_sim::ItemId;

    use super::*;

    const DT: Duration = Duration::from_millis(50);

    fn session() -> PlaySession {
        PlaySession::start(&Scene::server_default(), Camera2d::default()).unwrap()
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
        let mut s = PlaySession::start(&scene, Camera2d::default()).unwrap();
        s.click(Vec2::new(5.0, 0.0), 0.01);
        for _ in 0..40 {
            s.tick(DT, None);
        }
        assert_eq!(scene.items, before);
    }

    #[test]
    fn no_player_spawn_means_no_play() {
        let mut scene = Scene::server_default();
        scene.items.retain(|i| i.kind != ItemKind::PlayerSpawn);
        assert!(PlaySession::start(&scene, Camera2d::default()).is_err());
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
