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

use nexus_assets::{AnimState, SpriteAnimator, SpriteSheet};
use nexus_core::{Camera2d, Entity, Vec2, Vec3};
use nexus_render::{DEPTH_LAYER, RenderCommand, SpriteAnchor, TextureId, UvRect};
use nexus_script::RhaiHost;
use nexus_sim::{
    Authority, BagKind, EquipSlot, Event, Intent, ItemStack, LocalAuthority, Progress, Rejection,
    Relation, SimWorld, SkillId, Unit,
};

use crate::game_data::{ActorLook, GameData};
use crate::save_file::SaveData;
use crate::scene::{ActorId, ItemKind, Scene};
use crate::sprites::{Look, NO_TINT, SpriteLibrary};

/// 쫓는 대상이 마지막 경로 지점에서 이만큼(m) 벗어나야 다시 경로를 잡는다 (AI 와 같은 값).
const REPATH_DISTANCE: f32 = 0.5;
/// 단축키 스킬을 대상 없이 눌렀을 때 적을 찾는 거리 (m).
const SKILL_SEARCH_RANGE: f32 = 10.0;
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
/// 맞은 유닛을 붉게 깜빡인다 — 피격 그림이 없는 시트에도 맞은 것이 보이게. 곱하는 색이다.
const HIT_TINT: [f32; 4] = [1.0, 0.35, 0.35, 1.0];
/// 깜빡임 길이 (tick). 3 tick = 150ms.
const HIT_FLASH_TICKS: u8 = 3;

/// 플레이어가 클릭으로 내린 주문. 매 tick 이것을 보고 Intent 를 낸다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Order {
    /// 할 일 없음 (이동은 `MoveTo` 한 번으로 끝나므로 주문으로 남기지 않는다).
    Idle,
    /// 다가가서 친다. 대상이 죽거나 거절되면 끝.
    ///
    /// `skill` 은 단축키로 예약한 스킬이다 — 그 스킬이 한 번 맞으면 비우고 기본 공격으로 이어 간다.
    Attack {
        target: Entity,
        skill: Option<SkillId>,
    },
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
    /// 맞은 유닛의 붉은 깜빡임 — 남은 tick.
    hit_flash: HashMap<Entity, u8>,
    order: Order,
    /// 단축키 스킬 — 조작하는 액터 타입의 `skills`. 1번 키가 맨 앞.
    hotbar: Vec<SkillId>,
    /// 플레이어가 기본 부활할 시각 (시뮬레이션 시계). 쓰러져 있는 동안만 있다.
    revive_at: Option<Duration>,
    /// 조작하는 액터의 스크립트가 `on_dead` 를 정의했다 — 부활은 스크립트가 맡고 기본 부활은 하지 않는다.
    scripted_revive: bool,
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
        let hotbar = labels
            .get(&player)
            .map(|l: &Label| data.actor_skills(l.actor).to_vec())
            .unwrap_or_default();
        let scripted_revive = labels
            .get(&player)
            .is_some_and(|l: &Label| data.scripted_revive(l.actor));

        let mut auth = LocalAuthority::new(world);
        auth.set_script_host(Box::new(scripts));

        let mut session = Self {
            auth,
            player,
            labels,
            animators: HashMap::new(),
            hit_flash: HashMap::new(),
            order: Order::Idle,
            hotbar,
            revive_at: None,
            scripted_revive,
            chase_goal: None,
            log: VecDeque::new(),
            alerts: Vec::new(),
            editor_camera,
            data,
        };
        session.note(String::from(
            "플레이 시작 — 클릭: 이동 · 적 클릭: 공격 · 아이템 클릭: 줍기 · 숫자키: 스킬",
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
            mp: u.map(Unit::mp),
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
            self.order = Order::Attack {
                target,
                skill: None,
            };
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

    /// 지금 치고 있는 대상.
    pub(crate) fn attack_target(&self) -> Option<Entity> {
        match self.order {
            Order::Attack { target, .. } => Some(target),
            _ => None,
        }
    }

    /// HUD 단축키 칸 — 키 번호·남은 쿨타임 비율·MP 충분 여부.
    pub(crate) fn skill_slots(&self) -> Vec<crate::screen::SkillSlot> {
        let world = self.world();
        let me = world.unit(self.player);
        let now = world.now();
        self.hotbar
            .iter()
            .enumerate()
            .map(|(i, &skill)| {
                let def = world.skill(skill);
                let total = def.map_or(0.0, |d| d.cooldown().as_secs_f32());
                let left = me.map_or(0.0, |u| u.cooldown_left(skill, now).as_secs_f32());
                crate::screen::SkillSlot {
                    key: u8::try_from(i + 1).unwrap_or(u8::MAX),
                    cooldown: if total > 0.0 { left / total } else { 0.0 },
                    usable: me
                        .is_some_and(|u| u.is_alive() && def.is_some_and(|d| u.has_mp(d.mp_cost))),
                }
            })
            .collect()
    }

    /// 숫자키 `key`(1부터)의 스킬을 쓴다 — 지금 치는 대상에게, 없으면 가까운 적에게.
    /// 사거리 밖이면 다가가서 쓴다. 한 번 맞으면 기본 공격으로 이어 간다.
    ///
    /// 쿨타임·MP 는 **누르는 순간** 확인해 이유를 기록에 남긴다 — 예약해 두고 조용히 기다리면
    /// 눌렀는데 왜 안 나가는지 알 수 없다. 이때 하던 주문은 그대로 둔다.
    pub(crate) fn use_skill(&mut self, key: usize) {
        match self.plan_skill(key) {
            Ok((target, skill)) => {
                let line = format!("{} → {}", self.data.skill_name(skill), self.name(target));
                self.order = Order::Attack {
                    target,
                    skill: Some(skill),
                };
                self.chase_goal = None;
                self.note(line);
            }
            Err(why) => self.note(why),
        }
    }

    fn plan_skill(&self, key: usize) -> Result<(Entity, SkillId), String> {
        let skill = *key
            .checked_sub(1)
            .and_then(|i| self.hotbar.get(i))
            .ok_or_else(|| format!("단축키 {key} 에 스킬이 없습니다"))?;
        let name = self.data.skill_name(skill);
        let world = self.auth.world();
        let me = world
            .unit(self.player)
            .filter(|u| u.is_alive())
            .ok_or_else(|| String::from("쓰러진 상태입니다"))?;
        let def = world
            .skill(skill)
            .ok_or_else(|| format!("{name}: 모르는 스킬입니다"))?;
        let now = world.now();
        if !me.is_ready(skill, now) {
            return Err(format!(
                "{name}: 아직 쓸 수 없습니다 ({:.1}초)",
                me.cooldown_left(skill, now).as_secs_f32()
            ));
        }
        if !me.has_mp(def.mp_cost) {
            return Err(format!(
                "{name}: MP 가 부족합니다 ({}/{})",
                me.mp(),
                def.mp_cost
            ));
        }
        let current = self
            .attack_target()
            .filter(|t| world.unit(*t).is_some_and(Unit::is_alive));
        let target = current
            .or_else(|| {
                // 클릭으로 고른 대상이 없으면 가까운 적 — 우호·중립·불사는 고르지 않는다.
                let faction = me.def().faction;
                world
                    .units()
                    .filter(|(e, u)| {
                        *e != self.player
                            && u.is_alive()
                            && !u.def().immortal
                            && world.relation(faction, u.def().faction) == Relation::Hostile
                    })
                    .map(|(e, u)| (e, u.pos().distance(me.pos())))
                    .filter(|&(_, d)| d <= SKILL_SEARCH_RANGE)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(e, _)| e)
            })
            .ok_or_else(|| format!("{name}: 주변에 적이 없습니다"))?;
        Ok((target, skill))
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
        // 기본 부활 — 시각이 되면 저장 지점(처음 선 자리, 이어 하기면 저장한 자리)에서 일으킨다.
        // 부활도 Intent 다: 단계 3 에서는 서버가 받아들일지 정한다.
        if let Some(at) = self.revive_at
            && self.world().now() >= at
        {
            self.revive_at = None;
            if let Some(u) = self.world().unit(self.player).filter(|u| !u.is_alive()) {
                let policy = self.data.revive();
                let intent = Intent::Revive {
                    unit: self.player,
                    at: u.spawn_pos(),
                    hp_per_mille: policy.hp_per_mille,
                    exp_loss_per_mille: policy.exp_loss_per_mille,
                };
                self.auth.submit(intent);
            }
        }
        for intent in self.follow_order() {
            self.auth.submit(intent);
        }
        let events = self.auth.tick(dt);
        let cues = AnimCue::collect(&events);
        for event in events {
            self.on_event(event);
        }
        // 깜빡임은 tick 으로 센다 — 렌더 프레임으로 세면 fps 에 따라 길이가 달라진다.
        self.hit_flash.retain(|_, left| {
            *left -= 1;
            *left > 0
        });
        for (unit, cue) in &cues {
            if cue.hit {
                self.hit_flash.insert(*unit, HIT_FLASH_TICKS);
            }
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
            next_anim(
                self.animators.entry(unit).or_default(),
                sprites.map(|s| s.sheet(actor).anim()),
                UnitPose {
                    alive: u.is_alive(),
                    moving: u.is_moving(),
                },
                cues.get(&unit).copied().unwrap_or_default(),
                dt,
            );
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
            Order::Attack { target, skill } => {
                let Some(t) = world.unit(target).filter(|t| t.is_alive()) else {
                    self.order = Order::Idle;
                    return Vec::new();
                };
                // 예약한 스킬이 있으면 그 스킬의 사거리까지 다가간다.
                let range = world
                    .skill(skill.unwrap_or(attack_skill))
                    .map_or(0.0, |s| s.range);
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
                Order::Attack { target, skill } => {
                    let skill = skill.unwrap_or(attack_skill);
                    if u.is_ready(skill, world.now()) {
                        out.push(Intent::Attack {
                            unit: me,
                            target,
                            skill,
                        });
                    }
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
            && self.attack_target() == Some(unit)
        {
            self.order = Order::Idle;
            self.chase_goal = None;
        }
        // 리스폰 — 핸들이 새로 나왔다. 이름표를 옮기고, 옛 핸들에 걸린 재생기·깜빡임은 버린다
        // (새 유닛은 대기 동작부터). 플레이어였다면 조작 대상도 옮긴다.
        if let Event::Respawned { unit, replaces } = event {
            if let Some(label) = self.labels.remove(&replaces) {
                self.labels.insert(unit, label);
            }
            self.animators.remove(&replaces);
            self.hit_flash.remove(&replaces);
            if self.player == replaces {
                self.player = unit;
            }
        }
        // 예약한 스킬이 맞았으면 비운다 — 이후로는 기본 공격으로 이어 간다.
        if let Event::Damaged {
            attacker, skill, ..
        } = event
            && attacker == self.player
            && let Order::Attack { skill: queued, .. } = &mut self.order
            && *queued == Some(skill)
        {
            *queued = None;
        }
        let line = match event {
            Event::Damaged {
                attacker,
                target,
                skill,
                amount,
                remaining_hp,
            } => {
                // 기본 공격이 아니면 스킬 이름을 붙인다 — 단축키 스킬·스크립트 스킬이 기록에서 보이게.
                let basic = if attacker == self.player {
                    Some(self.data.player_attack())
                } else {
                    self.world()
                        .unit(attacker)
                        .and_then(|u| u.def().basic_attack)
                };
                let what = if basic == Some(skill) {
                    String::new()
                } else {
                    format!(" {}", self.data.skill_name(skill))
                };
                format!(
                    "{} →{what} {} {amount} 피해 (HP {remaining_hp})",
                    self.name(attacker),
                    self.name(target)
                )
            }
            Event::Died { unit, .. } if unit == self.player => {
                self.order = Order::Idle;
                self.chase_goal = None;
                if self.scripted_revive {
                    String::from(
                        "플레이어가 쓰러졌습니다 — 부활은 액터 스크립트(on_dead)가 정합니다",
                    )
                } else {
                    let delay = self.data.revive().delay;
                    self.revive_at = Some(self.world().now() + delay);
                    format!(
                        "플레이어가 쓰러졌습니다 — {:.1}초 뒤 저장 지점에서 일어납니다",
                        delay.as_secs_f32()
                    )
                }
            }
            Event::Died { unit, .. } => format!("{} 쓰러짐", self.name(unit)),
            Event::Revived {
                unit, hp, exp_lost, ..
            } => {
                let mut line = format!("{} 부활 — HP {hp}", self.name(unit));
                if exp_lost > 0 {
                    line.push_str(&format!(" · 경험치 -{exp_lost}"));
                }
                line
            }
            Event::Respawned { unit, .. } => format!("{} 다시 나타남", self.name(unit)),
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
            Event::ManaRestored {
                amount,
                remaining_mp,
                ..
            } => format!("MP +{amount} (MP {remaining_mp})"),
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

        if let Order::Attack { target, .. } = self.order
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
                _ if self.hit_flash.contains_key(&unit) => (HIT_TINT, HIT_TINT),
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
/// 한 tick 동안 유닛에게 일어난 일 중 **동작을 바꾸는 것** — 전투 이벤트에서 모은다.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct AnimCue {
    /// 공격이 맞았다 (때린 쪽).
    attacked: bool,
    /// 공격에 맞았다.
    hit: bool,
    died: bool,
}

impl AnimCue {
    fn collect(events: &[Event]) -> HashMap<Entity, Self> {
        let mut cues: HashMap<Entity, Self> = HashMap::new();
        for event in events {
            match *event {
                Event::Damaged {
                    attacker, target, ..
                } => {
                    cues.entry(attacker).or_default().attacked = true;
                    cues.entry(target).or_default().hit = true;
                }
                Event::Died { unit, .. } => cues.entry(unit).or_default().died = true,
                _ => {}
            }
        }
        cues
    }
}

/// 동작을 고르는 데 필요한 유닛 상태.
#[derive(Clone, Copy, Debug)]
struct UnitPose {
    alive: bool,
    moving: bool,
}

/// 유닛 하나의 재생기를 한 tick 진행한다.
///
/// 우선순위: **사망 > 공격 > 피격 > 걷기·대기.** 공격·피격·사망은 한 번짜리 동작이라 끝날 때까지
/// 걷기·대기로 덮지 않는다. 다만 **시트에 그 클립이 있을 때만** 들어간다 — 없는 클립은 대기로
/// 대체되는데(`clip_or_fallback`), 대기는 루프라 끝나지 않아 유닛이 그 자리에 얼어붙는다.
/// 피격 그림이 없는 시트는 대신 붉게 깜빡인다 ([`HIT_TINT`]).
///
/// 시트가 없으면(`None` — 시험이나 로드 실패) 상태만 바꾸고 진행하지 않는다.
fn next_anim(
    animator: &mut SpriteAnimator,
    sheet: Option<&SpriteSheet>,
    pose: UnitPose,
    cue: AnimCue,
    dt: Duration,
) {
    let has = |state| sheet.is_some_and(|s| s.clip(state).is_some());
    if !pose.alive {
        if cue.died && has(AnimState::Die) {
            animator.play(AnimState::Die);
        }
        // 사망 동작이 없으면 시체는 쓰러진 순간의 프레임에 멈춘다.
        if animator.state() != AnimState::Die {
            return;
        }
    } else {
        if cue.attacked && has(AnimState::Attack) {
            // 연속 공격은 처음부터 다시 — 같은 상태여도 리셋한다.
            animator.play(AnimState::Attack);
        } else if cue.hit && has(AnimState::Hit) && animator.state() != AnimState::Attack {
            // 휘두르는 중에 맞아도 공격 동작은 끊지 않는다.
            animator.play(AnimState::Hit);
        }
        let busy =
            matches!(animator.state(), AnimState::Attack | AnimState::Hit) && !animator.finished();
        if !busy {
            animator.set_state(if pose.moving {
                AnimState::Walk
            } else {
                AnimState::Idle
            });
        }
    }
    if let Some(sheet) = sheet {
        animator.advance(sheet, dt);
    }
}

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
            Rejection::NotEnoughMp => "MP 가 부족합니다",
            Rejection::OutOfRange => "너무 멉니다",
            Rejection::InvalidTarget => "그 대상은 고를 수 없습니다",
            Rejection::Invulnerable => "공격할 수 없는 대상입니다",
            Rejection::Friendly => "우호 대상입니다",
            Rejection::EmptySlot => "빈 칸입니다",
            Rejection::InventoryFull => "가방이 가득 찼습니다",
            Rejection::NotUsable => "쓸 수 없는 아이템입니다",
            Rejection::NotDead => "쓰러진 상태가 아닙니다",
        }
    }
}

/// 저장 데이터를 스폰한 플레이어에 얹는다 (P3).
///
/// **수치는 되살리지 않는다** — HP·MP 상한·공격력은 `rules.ron` 의 지금 값에서 다시 계산된다.
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
    if let Some(mp) = save.mp {
        world.set_mp(player, mp);
    }
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
            // 쓰러지면 멈춘다 — 더 돌리면 리스폰해서(10초) 옛 핸들이 사라진다.
            if !s.world().unit(slime).unwrap().is_alive() {
                break;
            }
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

    // ── 부활 ─────────────────────────────────────────────────────────────────

    fn start_with(data: GameData) -> PlaySession {
        PlaySession::start(
            &Scene::server_default(),
            Camera2d::default(),
            data,
            PlayOptions::default(),
        )
        .unwrap()
    }

    /// 슬라임을 플레이어 옆으로 옮기고 HP 를 1 로 — 한 번 물리면 쓰러진다. 쓰러질 때까지 돌린다.
    fn knock_out(s: &mut PlaySession) {
        let me = s.player();
        let slime = find(s, "슬라임");
        let at = s.world().unit(me).unwrap().pos() + Vec2::new(1.0, 0.0);
        s.auth.world_mut().set_position(slime, at);
        s.auth.world_mut().set_hp(me, 1);
        for _ in 0..200 {
            s.tick(DT, None);
            if !s.world().unit(me).unwrap().is_alive() {
                return;
            }
        }
        panic!("쓰러지지 않았다: {:?}", s.log);
    }

    #[test]
    fn the_player_gets_up_at_the_save_point_after_the_default_delay() {
        // rules.ron: revive (delay_ms: 3000, hp_percent: 100, exp_loss_percent: 0).
        let mut s = session();
        let me = s.player();
        let save_point = s.world().unit(me).unwrap().spawn_pos();
        knock_out(&mut s);
        assert_eq!(
            last_log(&s),
            "플레이어가 쓰러졌습니다 — 3.0초 뒤 저장 지점에서 일어납니다"
        );
        for _ in 0..55 {
            s.tick(DT, None);
        }
        assert!(
            !s.world().unit(me).unwrap().is_alive(),
            "3초 전에는 쓰러져 있다"
        );
        for _ in 0..10 {
            s.tick(DT, None);
        }
        let u = s.world().unit(me).expect("같은 핸들 — 리스폰이 아니다");
        assert!(u.is_alive());
        assert_eq!(u.pos(), save_point);
        assert!(
            s.log.iter().any(|l| l.starts_with("플레이어 부활 — HP")),
            "{:?}",
            s.log
        );
        // 소지품은 그대로 — 새 유닛이 아니다.
        assert!(!s.bag_lines(BagKind::Consumable).is_empty());
    }

    #[test]
    fn the_default_revive_takes_its_numbers_from_the_rules() {
        let rules = GameData::embedded_rules().replacen(
            "revive: (delay_ms: 3000, hp_percent: 100, exp_loss_percent: 0)",
            "revive: (delay_ms: 500, hp_percent: 25, exp_loss_percent: 50)",
            1,
        );
        assert_ne!(rules, GameData::embedded_rules(), "시험 전제");
        let mut s = start_with(GameData::parse_with_scripts(&rules, &[]).unwrap());
        let me = s.player();
        s.auth.world_mut().set_progress(me, Progress::new(1, 80));
        knock_out(&mut s);
        for _ in 0..15 {
            s.tick(DT, None);
        }
        let u = s.world().unit(me).unwrap();
        assert!(u.is_alive(), "0.5초 뒤");
        assert_eq!(u.progress().exp(), 40, "경험치 절반");
        assert!(
            s.log.iter().any(|l| l.contains("경험치 -40")),
            "{:?}",
            s.log
        );
    }

    #[test]
    fn an_on_dead_script_replaces_the_default_revive() {
        // 플레이어 액터에 스크립트를 붙인다 — 1초 뒤 쓰러진 그 자리에서 HP 30% 로.
        const HERO: &str = r#"
            fn on_death(me, killer) { this.down = 0.0; }
            fn on_dead(me, dt) {
                this.down += dt;
                if this.down >= 1.0 { revive(me, me.x, me.y, 30); }
            }
        "#;
        let rules = GameData::embedded_rules().replacen(
            "            skills: [5, 6],\n",
            "            skills: [5, 6],\n            script: Some(\"scripts/hero.rhai\"),\n",
            1,
        );
        assert_ne!(rules, GameData::embedded_rules(), "시험 전제");
        let data = GameData::parse_with_scripts(&rules, &[("scripts/hero.rhai", HERO)]).unwrap();
        let mut s = start_with(data);
        let me = s.player();
        assert!(s.scripted_revive);
        knock_out(&mut s);
        let fell_at = s.world().unit(me).unwrap().pos();
        assert!(
            last_log(&s).contains("액터 스크립트(on_dead)"),
            "{}",
            last_log(&s)
        );
        assert_eq!(s.revive_at, None, "기본 부활은 예약하지 않는다");
        for _ in 0..30 {
            s.tick(DT, None);
        }
        let u = s.world().unit(me).unwrap();
        assert!(u.is_alive(), "{:?}", s.log);
        assert_eq!(u.pos(), fell_at, "그 자리에서");
        assert!(
            u.hp() <= u.max_hp() * 3 / 10,
            "HP 30% (그 뒤 맞았을 수도 있다)"
        );
    }

    // ── 리스폰 ───────────────────────────────────────────────────────────────

    #[test]
    fn a_slain_slime_respawns_at_its_marker_with_its_name() {
        // rules.ron: 슬라임 respawn_ms 10000.
        let mut s = session();
        let slime = find(&s, "슬라임");
        let spawn = s.world().unit(slime).unwrap().pos();
        s.click(spawn, 0.01);
        for _ in 0..900 {
            s.tick(DT, None);
            if !s.world().unit(slime).unwrap().is_alive() {
                break;
            }
        }
        assert!(!s.world().unit(slime).unwrap().is_alive(), "{:?}", s.log);
        // 플레이어를 멀리 치워 둔다 — 되살아난 슬라임이 곧바로 덤벼 죽지 않게.
        let me = s.player();
        s.auth.world_mut().set_position(me, Vec2::new(-30.0, 30.0));

        // 쓰러진 tick 의 시작 시각부터 10초 = 그 뒤 199 tick 째.
        for _ in 0..198 {
            s.tick(DT, None);
        }
        assert!(s.world().unit(slime).is_some(), "10초 전에는 시체로 남는다");
        s.tick(DT, None);
        assert!(s.world().unit(slime).is_none(), "시체는 치워졌다");
        let again = find(&s, "슬라임");
        let u = s.world().unit(again).unwrap();
        assert!(u.is_alive() && u.hp() == u.max_hp());
        assert_eq!(u.pos(), spawn, "마커 자리");
        assert!(
            s.log.iter().any(|l| l == "슬라임 다시 나타남"),
            "{:?}",
            s.log
        );
    }

    // ── 단축키 스킬 ──────────────────────────────────────────────────────────

    const SMASH: SkillId = SkillId(5);

    fn last_log(s: &PlaySession) -> String {
        s.log.back().cloned().unwrap_or_default()
    }

    #[test]
    fn the_hotbar_comes_from_the_controlled_actor() {
        let s = session();
        assert_eq!(
            s.hotbar,
            [SkillId(5), SkillId(6)],
            "rules.ron: actors.1.skills"
        );
        let slots = s.skill_slots();
        assert_eq!(slots.len(), 2);
        assert!(slots.iter().all(|x| x.usable && x.cooldown == 0.0));
    }

    #[test]
    fn a_skill_key_casts_once_on_the_target_then_basic_attacks_resume() {
        let mut s = session();
        let slime = find(&s, "슬라임");
        s.click(s.world().unit(slime).unwrap().pos(), 0.01);
        s.use_skill(1);
        assert_eq!(
            s.order,
            Order::Attack {
                target: slime,
                skill: Some(SMASH)
            }
        );
        let mut cast = false;
        for _ in 0..400 {
            s.tick(DT, None);
            if s.log.iter().any(|l| l.contains("강베기 슬라임")) {
                cast = true;
                break;
            }
        }
        assert!(cast, "{:?}", s.log);
        assert_eq!(
            s.order,
            Order::Attack {
                target: slime,
                skill: None
            },
            "한 번 맞으면 기본 공격으로"
        );
        let me = s.world().unit(s.player()).unwrap();
        assert!(me.mp() < 60, "MP 15 를 썼다 (지금 {})", me.mp());
        assert!(s.skill_slots()[0].cooldown > 0.0, "칸에 쿨타임이 보인다");

        // 쿨타임 중에 다시 누르면 이유를 알리고 주문은 그대로.
        s.use_skill(1);
        assert!(
            last_log(&s).contains("아직 쓸 수 없습니다"),
            "{}",
            last_log(&s)
        );
        assert_eq!(s.attack_target(), Some(slime));
    }

    #[test]
    fn a_skill_key_without_a_target_picks_the_nearest_enemy() {
        let mut s = session();
        let slime = find(&s, "슬라임");
        let near = s.world().unit(slime).unwrap().pos() + Vec2::new(3.0, 0.0);
        let me = s.player();
        s.auth.world_mut().set_position(me, near);
        s.use_skill(2);
        assert_eq!(s.attack_target(), Some(slime), "{:?}", s.log);
        assert_eq!(last_log(&s), "돌 던지기 → 슬라임");

        // 아무도 없으면 알린다.
        let mut far = session();
        let me = far.player();
        far.auth
            .world_mut()
            .set_position(me, Vec2::new(-30.0, 30.0));
        far.use_skill(2);
        assert!(
            last_log(&far).contains("주변에 적이 없습니다"),
            "{:?}",
            far.log
        );
    }

    #[test]
    fn short_mp_and_empty_keys_are_reported_at_once() {
        let mut s = session();
        let me = s.player();
        s.auth.world_mut().set_mp(me, 3);
        let slime = find(&s, "슬라임");
        s.click(s.world().unit(slime).unwrap().pos(), 0.01);
        s.use_skill(1);
        assert_eq!(last_log(&s), "강베기: MP 가 부족합니다 (3/15)");
        assert!(!s.skill_slots()[0].usable, "칸이 흐려진다");
        assert_eq!(
            s.order,
            Order::Attack {
                target: slime,
                skill: None
            },
            "하던 주문은 그대로"
        );
        s.use_skill(7);
        assert_eq!(last_log(&s), "단축키 7 에 스킬이 없습니다");
    }

    // ── 전투 동작 ────────────────────────────────────────────────────────────

    const PLAYER_SHEET: &str = "assets/third_party/zelda-like-armm1998/player.sheet.ron";
    const SLIME_SHEET: &str = "assets/third_party/slime-garakh/monster.sheet.ron";
    const STANDING: UnitPose = UnitPose {
        alive: true,
        moving: false,
    };
    const WALKING: UnitPose = UnitPose {
        alive: true,
        moving: true,
    };
    const DEAD: UnitPose = UnitPose {
        alive: false,
        moving: false,
    };
    const NONE: AnimCue = AnimCue {
        attacked: false,
        hit: false,
        died: false,
    };
    const ATTACKED: AnimCue = AnimCue {
        attacked: true,
        ..NONE
    };

    #[test]
    fn an_attack_plays_once_then_walking_resumes() {
        // 플레이어 시트의 공격 = 4프레임 × 90ms = 360ms → 8 tick 째에 끝난다.
        let sheet = crate::sprites::sheet_for_test(PLAYER_SHEET);
        let mut a = SpriteAnimator::default();
        next_anim(&mut a, Some(&sheet), WALKING, ATTACKED, DT);
        assert_eq!(a.state(), AnimState::Attack, "걷는 중이어도 휘두른다");
        for _ in 0..6 {
            next_anim(&mut a, Some(&sheet), WALKING, NONE, DT);
            assert_eq!(
                a.state(),
                AnimState::Attack,
                "끝날 때까지 걷기로 덮지 않는다"
            );
        }
        assert_eq!(a.frame(), 3, "마지막 프레임까지 보여 준다");
        next_anim(&mut a, Some(&sheet), WALKING, NONE, DT); // 400ms — 끝
        next_anim(&mut a, Some(&sheet), WALKING, NONE, DT);
        assert_eq!(a.state(), AnimState::Walk);
    }

    #[test]
    fn a_second_hit_restarts_the_swing() {
        let sheet = crate::sprites::sheet_for_test(PLAYER_SHEET);
        let mut a = SpriteAnimator::default();
        next_anim(&mut a, Some(&sheet), STANDING, ATTACKED, DT);
        for _ in 0..4 {
            next_anim(&mut a, Some(&sheet), STANDING, NONE, DT);
        }
        assert!(a.frame() > 0);
        next_anim(&mut a, Some(&sheet), STANDING, ATTACKED, DT);
        assert_eq!((a.state(), a.frame()), (AnimState::Attack, 0));
    }

    #[test]
    fn a_missing_clip_is_never_entered() {
        // 슬라임 시트에는 공격·피격 그림이 없다 — 들어가면 대기(루프)로 대체되어 끝나지 않는다.
        let sheet = crate::sprites::sheet_for_test(SLIME_SHEET);
        let mut a = SpriteAnimator::default();
        let both = AnimCue {
            attacked: true,
            hit: true,
            died: false,
        };
        next_anim(&mut a, Some(&sheet), WALKING, both, DT);
        assert_eq!(a.state(), AnimState::Walk);
    }

    #[test]
    fn death_plays_its_clip_and_stays_on_the_last_frame() {
        let sheet = crate::sprites::sheet_for_test(SLIME_SHEET);
        let mut a = SpriteAnimator::default();
        let died = AnimCue { died: true, ..NONE };
        next_anim(&mut a, Some(&sheet), DEAD, died, DT);
        assert_eq!(a.state(), AnimState::Die);
        for _ in 0..40 {
            next_anim(&mut a, Some(&sheet), DEAD, NONE, DT);
        }
        assert_eq!(
            (a.state(), a.frame(), a.finished()),
            (AnimState::Die, 2, true)
        );

        // 사망 그림이 없는 시트는 쓰러진 순간의 프레임에 멈춘다 — 대기로 돌아가지 않는다.
        let player = crate::sprites::sheet_for_test(PLAYER_SHEET);
        let mut b = SpriteAnimator::default();
        next_anim(&mut b, Some(&player), WALKING, NONE, DT * 3);
        let frozen = (b.state(), b.frame());
        for _ in 0..10 {
            next_anim(&mut b, Some(&player), DEAD, died, DT);
        }
        assert_eq!((b.state(), b.frame()), frozen);
    }

    #[test]
    fn hits_flash_for_a_few_ticks_in_play() {
        // 슬라임을 끝까지 잡는 판 — 맞은 쪽이 깜빡이고, 깜빡임은 tick 으로 끝난다.
        let mut s = session();
        let slime = find(&s, "슬라임");
        s.click(s.world().unit(slime).unwrap().pos(), 0.01);
        let mut flashed = false;
        for _ in 0..900 {
            s.tick(DT, None);
            flashed |= s.hit_flash.contains_key(&slime);
            if !s.world().unit(slime).unwrap().is_alive() {
                break;
            }
        }
        assert!(flashed, "맞은 슬라임이 깜빡였어야 한다");
        assert!(!s.world().unit(slime).unwrap().is_alive(), "{:?}", s.log);
        for _ in 0..HIT_FLASH_TICKS {
            s.tick(DT, None);
        }
        assert!(
            !s.hit_flash.contains_key(&slime),
            "더 맞지 않으면 깜빡임은 tick 으로 끝난다"
        );
    }

    #[test]
    fn the_player_has_mp_that_a_blue_potion_restores_and_a_save_keeps() {
        const BLUE_POTION: ItemId = ItemId(502);
        let mut s = session();
        let me = s.player();
        let u = s.world().unit(me).unwrap();
        assert_eq!((u.mp(), u.max_mp()), (60, 60), "rules.ron: max_mp 60");
        let slot = u
            .inventory()
            .bag(BagKind::Consumable)
            .iter()
            .find(|(_, st)| st.item == BLUE_POTION)
            .map(|(slot, _)| u16::try_from(slot).unwrap())
            .expect("시작 소지품에 파란 포션");

        s.auth.world_mut().set_mp(me, 5);
        s.inventory(InventoryAction::Use(slot));
        s.tick(DT, None);
        // 5 + 40. 초당 2 회복은 50ms 로는 아직 1 이 되지 않는다.
        assert_eq!(s.world().unit(me).unwrap().mp(), 45);
        assert!(s.log.iter().any(|l| l == "MP +40 (MP 45)"), "{:?}", s.log);

        let save = s.save_data(String::new());
        assert_eq!(save.mp, Some(45));
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
        assert_eq!(s.world().unit(s.player()).unwrap().mp(), 45, "이어 하기");
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
        assert_eq!(s.attack_target(), Some(slime));

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
