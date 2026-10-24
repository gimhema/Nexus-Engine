//! 시뮬레이션 월드 — 타일맵 + 유닛·땅의 아이템 저장소 + 이동·전투·아이템 규칙.
//!
//! # 컴포넌트 저장소는 직접 구현이다
//!
//! 핸들 발급은 [`nexus_core::World`] 가 하고, 이 모듈은 **엔티티 슬롯 번호로 색인하는
//! 벡터**에 유닛을 둔다. 컴포넌트 종류가 [`Unit`] 하나뿐인 지금은 범용 ECS(`hecs` 등)가
//! 줄 이득이 없다. 종류가 늘어 조회 패턴이 복잡해지면 그때 바꾸되, 바뀌는 범위는
//! 이 크레이트 안으로 한정된다.
//!
//! # 바꾸는 길은 Authority 뿐
//!
//! 게임플레이 변경 메서드는 `pub(crate)` 다. 밖에서는 [`Intent`](crate::Intent) 를
//! [`Authority`](crate::Authority) 에 넘기는 길만 있다. 예외는 스폰/디스폰 — 존을 읽어
//! 초기 상태를 만드는 **설정 작업**이라 공개한다.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use nexus_core::units::dir_to_heading;
use nexus_core::{Entity, Vec2, World};

use crate::authority::{Event, Rejection};
use crate::combat::{self, SkillDef, SkillId};
use crate::faction::{FactionId, FactionTable, Relation};
use crate::item::{
    BAG_SLOTS, BagKind, EquipSlot, GroundItem, ItemDef, ItemId, ItemKind, ItemStack,
};
use crate::loot::{self, LootEntry, LootTableId, Rng};
use crate::progress::Progress;
use crate::skills::{SkillRules, to_u32};
use crate::stats::{ClassDef, ClassId, EquipRules, EquipTotals, Formula, StatRules};
use crate::tilemap::TileMap;
use crate::trigger::{
    Condition, Effect, FlagId, FlagScope, Flags, Happening, MAX_ROUNDS, TriggerDef, TriggerId,
};
use crate::unit::{AiKind, AiState, Unit, UnitDef};

/// 도착 판정 거리 (m). 부동소수 오차로 경유점 바로 앞에서 멈추지 않게 한다.
const ARRIVE_EPSILON: f32 = 1e-4;

/// 줍기 사거리 기본값 (m). RO 처럼 거의 발밑까지 가야 한다. [`SimWorld::set_pickup_range`] 로 바꾼다.
pub const DEFAULT_PICKUP_RANGE: f32 = 1.5;

/// 시뮬레이션 상태 전체.
///
/// 유닛과 땅의 아이템은 **같은 핸들 공간**([`nexus_core::World`])을 쓴다 — 같은 `Entity` 값이
/// 두 가지를 가리키는 일이 없게. 저장소만 따로다.
#[derive(Debug)]
pub struct SimWorld {
    tiles: TileMap,
    entities: World,
    /// 엔티티 슬롯 번호로 색인. 핸들을 함께 두어 순회 시 되돌려 준다.
    units: Vec<Option<(Entity, Unit)>>,
    /// 땅에 떨어진 아이템. `units` 와 같은 방식으로 색인.
    ground: Vec<Option<(Entity, GroundItem)>>,
    /// 스킬 정적 데이터. 존을 읽을 때 채운다.
    skills: HashMap<SkillId, SkillDef>,
    items: HashMap<ItemId, ItemDef>,
    loot_tables: HashMap<LootTableId, Vec<LootEntry>>,
    factions: FactionTable,
    rng: Rng,
    pickup_range: f32,
    /// 땅의 아이템이 사라지기까지. `None` 이면 사라지지 않는다.
    ground_item_lifetime: Option<Duration>,
    /// 시뮬레이션 시계 — tick 마다 `dt` 만큼 정확히 늘어난다. 쿨타임의 기준.
    now: Duration,
    /// 월드 범위 진행 플래그 (R1).
    world_flags: Flags,
    /// 트리거 — 번호 순으로 판정한다 (R1).
    triggers: BTreeMap<TriggerId, TriggerDef>,
    /// 스탯 체계와 직업 (R2). 체계가 없으면 직업이 있는 유닛도 수치를 그대로 쓴다.
    stat_rules: Option<StatRules>,
    classes: HashMap<ClassId, ClassDef>,
    /// 장비의 기본 스탯 보너스·직업 제한. 없는 장비는 보너스도 제한도 없다.
    equip_rules: HashMap<ItemId, EquipRules>,
    /// 경험치 곡선 (R3) — 변수는 `level`. 없으면 서버와 같은 `level² × 100`.
    exp_curve: Option<Formula>,
    /// 스킬 부착 규칙·레벨 수식 (R3). 없는 스킬은 "누구나, 1레벨, 숫자 그대로".
    skill_rules: HashMap<SkillId, SkillRules>,
}

impl SimWorld {
    #[must_use]
    pub fn new(tiles: TileMap) -> Self {
        Self {
            tiles,
            entities: World::default(),
            units: Vec::new(),
            ground: Vec::new(),
            skills: HashMap::new(),
            items: HashMap::new(),
            loot_tables: HashMap::new(),
            factions: FactionTable::default(),
            rng: Rng::new(0),
            pickup_range: DEFAULT_PICKUP_RANGE,
            ground_item_lifetime: None,
            now: Duration::ZERO,
            world_flags: Flags::default(),
            triggers: BTreeMap::new(),
            stat_rules: None,
            classes: HashMap::new(),
            equip_rules: HashMap::new(),
            exp_curve: None,
            skill_rules: HashMap::new(),
        }
    }

    /// 스킬의 부착 규칙·레벨 수식을 등록한다 (설정 작업).
    pub fn define_skill_rules(&mut self, skill: SkillId, rules: SkillRules) {
        self.skill_rules.insert(skill, rules);
    }

    #[must_use]
    pub fn skill_rules(&self, skill: SkillId) -> Option<&SkillRules> {
        self.skill_rules.get(&skill)
    }

    /// 스킬 최대 레벨 (규칙이 없으면 1).
    #[must_use]
    pub fn skill_max_level(&self, skill: SkillId) -> u32 {
        self.skill_rules
            .get(&skill)
            .map_or(1, |r| r.max_level.max(1))
    }

    /// 스킬 레벨을 그대로 심는다 (설정 작업 — 직업의 시작 스킬, 저장 파일에서 되살릴 때).
    /// 조건은 보지 않는다. 최대 레벨로 자르고, 0 이면 뗀다. 없는 스킬은 무시한다.
    pub fn set_skill_level(&mut self, unit: Entity, skill: SkillId, level: u32) {
        if !self.skills.contains_key(&skill) {
            return;
        }
        let max = self.skill_max_level(skill);
        if let Some(u) = self.unit_mut(unit) {
            if level == 0 {
                u.skills.remove(&skill);
            } else {
                u.skills.insert(skill, level.min(max));
            }
        }
    }

    /// 이 유닛이 쓸 때의 스킬 — 레벨 수식을 그 유닛의 스탯·스킬 레벨로 계산해 덮은 값 (R3).
    /// 배우지 않은 스킬은 1레벨로 계산한다. UI 가 툴팁에 쓸 수 있다.
    #[must_use]
    pub fn effective_skill(&self, unit: Entity, skill: SkillId) -> Option<SkillDef> {
        let mut def = *self.skills.get(&skill)?;
        let Some(rules) = self.skill_rules.get(&skill) else {
            return Some(def);
        };
        let u = self.unit(unit)?;
        // 변수 순서는 skills::skill_variables 와 같다: 기본 스탯 · level · equip_attack · equip_defense · skill_level.
        let n = self.stat_rules.as_ref().map_or(0, |r| r.stats().len());
        let mut vals: Vec<i64> = (0..n)
            .map(|i| {
                u.sheet
                    .as_ref()
                    .and_then(|s| s.base.get(i).copied())
                    .unwrap_or(0)
            })
            .collect();
        vals.extend([
            i64::from(u.progress.level()),
            i64::from(u.bonus_attack),
            i64::from(u.bonus_defense),
            i64::from(u.skill_level(skill).max(1)),
        ]);
        if let Some(f) = &rules.power_per_mille {
            // 천분율 → 배율. 전투 공식은 서버와 같은 f32 배율을 그대로 쓴다.
            def.damage_mult = to_u32(f.eval(&vals)) as f32 / 1000.0;
        }
        if let Some(f) = &rules.cooldown_ms {
            def.cooldown_ms = to_u32(f.eval(&vals));
        }
        if let Some(f) = &rules.mp_cost {
            def.mp_cost = to_u32(f.eval(&vals));
        }
        if let Some(f) = &rules.heal {
            def.heal = to_u32(f.eval(&vals));
        }
        Some(def)
    }

    /// 스킬을 배운다 / 레벨을 올린다 — **모든 습득 경로가 지나는 문** (R3). 새 레벨을 돌려준다.
    pub(crate) fn learn(
        &mut self,
        unit: Entity,
        skill: SkillId,
        levels: u32,
        events: &mut Vec<Event>,
    ) -> Result<u32, Rejection> {
        if !self.skills.contains_key(&skill) {
            return Err(Rejection::UnknownSkill);
        }
        let u = self.unit(unit).ok_or(Rejection::UnknownEntity)?;
        let max = self.skill_max_level(skill);
        let have = u.skill_level(skill);
        if let Some(rules) = self.skill_rules.get(&skill)
            && (!rules.allows(u.def.class) || !self.check(&rules.requires, unit))
        {
            return Err(Rejection::SkillRequirement);
        }
        if have >= max {
            return Err(Rejection::SkillMaxed);
        }
        let level = have.saturating_add(levels.max(1)).min(max);
        if let Some(u) = self.unit_mut(unit) {
            u.skills.insert(skill, level);
        }
        events.push(Event::SkillLearned { unit, skill, level });
        Ok(level)
    }

    /// 스킬을 뗀다 (R3). 배우지 않았으면 아무 일도 없다.
    pub(crate) fn forget(&mut self, unit: Entity, skill: SkillId, events: &mut Vec<Event>) {
        if let Some(u) = self.unit_mut(unit)
            && u.skills.remove(&skill).is_some()
        {
            events.push(Event::SkillForgotten { unit, skill });
        }
    }

    /// 경험치 곡선을 정한다 (설정 작업 — 스폰보다 먼저). 수식의 변수는 `level` 하나다.
    pub fn set_exp_curve(&mut self, curve: Formula) {
        self.exp_curve = Some(curve);
    }

    /// `level` 에서 다음 레벨까지 필요한 경험치 (1 이상).
    #[must_use]
    pub fn exp_needed(&self, level: u32) -> u32 {
        exp_needed(self.exp_curve.as_ref(), level)
    }

    /// 스탯 체계를 정한다 (설정 작업 — **스폰보다 먼저**. 이미 선 유닛은 다시 계산하지 않는다).
    pub fn set_stat_rules(&mut self, rules: StatRules) {
        self.stat_rules = Some(rules);
    }

    #[must_use]
    pub fn stat_rules(&self) -> Option<&StatRules> {
        self.stat_rules.as_ref()
    }

    /// 직업을 등록한다 (설정 작업 — 스폰보다 먼저).
    pub fn define_class(&mut self, id: ClassId, def: ClassDef) {
        self.classes.insert(id, def);
    }

    #[must_use]
    pub fn class(&self, id: ClassId) -> Option<&ClassDef> {
        self.classes.get(&id)
    }

    /// 장비의 스탯 보너스·직업 제한을 등록한다 (설정 작업).
    pub fn define_equip_rules(&mut self, item: ItemId, rules: EquipRules) {
        self.equip_rules.insert(item, rules);
    }

    #[must_use]
    pub fn equip_rules(&self, item: ItemId) -> Option<&EquipRules> {
        self.equip_rules.get(&item)
    }

    /// 트리거를 등록한다 (설정 작업). 같은 번호면 갈아 끼운다.
    pub fn define_trigger(&mut self, id: TriggerId, def: TriggerDef) {
        self.triggers.insert(id, def);
    }

    /// 등록된 트리거 — 번호 순.
    pub fn triggers(&self) -> impl Iterator<Item = (TriggerId, &TriggerDef)> {
        self.triggers.iter().map(|(id, t)| (*id, t))
    }

    /// 월드 범위 플래그 하나 (없으면 0).
    #[must_use]
    pub fn world_flag(&self, flag: FlagId) -> i32 {
        self.world_flags.get(flag)
    }

    /// 월드 범위 플래그 전부 — 저장할 때.
    #[must_use]
    pub fn world_flags(&self) -> &Flags {
        &self.world_flags
    }

    /// 월드 플래그를 그대로 심는다 (설정 작업 — 저장 파일에서 되살릴 때).
    pub fn set_world_flag(&mut self, flag: FlagId, value: i32) {
        self.world_flags.set(flag, value);
    }

    /// 캐릭터 플래그를 그대로 심는다 (설정 작업 — 저장 파일에서 되살릴 때).
    pub fn set_unit_flag(&mut self, unit: Entity, flag: FlagId, value: i32) {
        if let Some(u) = self.unit_mut(unit) {
            u.flags.set(flag, value);
        }
    }

    /// 조건을 판정한다 — `subject` 는 대상 유닛. 상태를 바꾸지 않으므로 UI 도 쓸 수 있다
    /// (이 NPC 기능을 보여 줄지 등).
    ///
    /// 대상이 없으면(디스폰) **유닛을 보는 조건은 거짓**이다. 월드 플래그는 그대로 판정한다.
    #[must_use]
    pub fn check(&self, condition: &Condition, subject: Entity) -> bool {
        let unit = self.unit(subject);
        match condition {
            Condition::Always => true,
            Condition::Flag {
                scope,
                flag,
                cmp,
                value,
            } => {
                let have = match scope {
                    FlagScope::Character => match unit {
                        Some(u) => u.flags.get(*flag),
                        None => return false,
                    },
                    FlagScope::World => self.world_flags.get(*flag),
                };
                cmp.test(i64::from(have), i64::from(*value))
            }
            Condition::Level { cmp, value } => {
                unit.is_some_and(|u| cmp.test(i64::from(u.progress.level()), i64::from(*value)))
            }
            Condition::HasItem { item, count } => unit.is_some_and(|u| {
                let inv = &u.inventory;
                inv.bag(BagKind::Consumable).count_of(*item)
                    + inv.bag(BagKind::Equipment).count_of(*item)
                    >= *count
            }),
            Condition::Actor(actor) => unit.is_some_and(|u| u.def.actor == *actor),
            Condition::Faction(faction) => unit.is_some_and(|u| u.def.faction == *faction),
            Condition::All(all) => all.iter().all(|c| self.check(c, subject)),
            Condition::Any(any) => any.iter().any(|c| self.check(c, subject)),
            Condition::Not(c) => !self.check(c, subject),
            Condition::SkillLevel { skill, cmp, value } => {
                unit.is_some_and(|u| cmp.test(i64::from(u.skill_level(*skill)), i64::from(*value)))
            }
        }
    }

    /// 난수 시드 (설정 작업). 같은 시드·같은 입력이면 드롭까지 같은 결과가 나온다.
    pub fn set_seed(&mut self, seed: u64) {
        self.rng = Rng::new(seed);
    }

    /// 아이템을 등록한다 (설정 작업).
    pub fn define_item(&mut self, id: ItemId, def: ItemDef) {
        self.items.insert(id, def);
    }

    #[must_use]
    pub fn item_def(&self, id: ItemId) -> Option<&ItemDef> {
        self.items.get(&id)
    }

    /// 드롭 테이블을 등록한다 (설정 작업). `UnitDef::loot` 가 이 번호를 가리킨다.
    pub fn define_loot(&mut self, id: LootTableId, entries: Vec<LootEntry>) {
        self.loot_tables.insert(id, entries);
    }

    /// 레벨·경험치를 그대로 심는다 (설정 작업 — 저장 파일에서 되살릴 때).
    /// 성장치만큼 최대 HP 가 달라지므로 현재 HP 도 상한으로 자른다.
    pub fn set_progress(&mut self, unit: Entity, progress: Progress) {
        let curve = self.exp_curve.clone();
        if let Some(u) = self.unit_mut(unit) {
            u.progress = Progress::with_curve(progress.level(), progress.exp(), |lv| {
                exp_needed(curve.as_ref(), lv)
            });
        }
        self.refresh_stats(unit);
        if let Some(u) = self.unit_mut(unit) {
            let cap = u.max_hp();
            u.hp = u.hp.min(cap);
            u.mp = u.mp.min(u.max_mp());
        }
    }

    /// 위치를 그대로 심는다 (설정 작업 — 저장한 자리에서 이어 하기).
    /// 보간이 튀지 않게 직전 위치도 맞추고, AI 의 귀환 기준(집)도 여기로 옮긴다.
    pub fn set_position(&mut self, unit: Entity, pos: Vec2) {
        if let Some(u) = self.unit_mut(unit) {
            u.pos = pos;
            u.prev_pos = pos;
            u.home = pos;
            u.spawn_pos = pos;
            u.waypoints.clear();
        }
    }

    /// 현재 HP 를 그대로 심는다 (설정 작업). 0 은 시체를 만드므로 최소 1.
    pub fn set_hp(&mut self, unit: Entity, hp: u32) {
        if let Some(u) = self.unit_mut(unit) {
            let cap = u.max_hp();
            u.hp = hp.clamp(1, cap);
        }
    }

    /// 현재 MP 를 그대로 심는다 (설정 작업). 상한으로 자른다.
    pub fn set_mp(&mut self, unit: Entity, mp: u32) {
        if let Some(u) = self.unit_mut(unit) {
            u.mp = mp.min(u.max_mp());
            u.mp_carry = 0;
        }
    }

    /// 가방의 **그 칸에** 아이템을 놓는다 (설정 작업 — 저장한 칸 번호를 그대로 되살린다).
    /// 정의되지 않은 아이템이나 범위 밖 칸은 무시한다.
    pub fn place_item(&mut self, unit: Entity, slot: u16, stack: ItemStack) -> bool {
        let Some(def) = self.items.get(&stack.item).copied() else {
            return false;
        };
        let slot = usize::from(slot);
        if slot >= BAG_SLOTS || stack.count == 0 || stack.count > def.stack_limit() {
            return false;
        }
        let Some(u) = self.unit_mut(unit) else {
            return false;
        };
        u.inventory.bag_mut(def.bag()).put(slot, Some(stack));
        true
    }

    /// 장비를 가방을 거치지 않고 바로 장착한다 (설정 작업). 장비가 아니면 무시한다.
    pub fn force_equip(&mut self, unit: Entity, item: ItemId) -> bool {
        let Some(def) = self.items.get(&item).copied() else {
            return false;
        };
        let ItemKind::Equipment { slot, .. } = def.kind else {
            return false;
        };
        let Some(u) = self.unit_mut(unit) else {
            return false;
        };
        u.inventory.set_equipped(slot, Some(item));
        self.refresh_stats(unit);
        true
    }

    pub fn set_pickup_range(&mut self, meters: f32) {
        self.pickup_range = if meters.is_finite() {
            meters.max(0.0)
        } else {
            0.0
        };
    }

    /// 땅의 아이템이 사라지기까지의 시간 (설정 작업). `None`·0 이면 영영 남는다 (기본).
    /// 떨어진 뒤 이 시간이 지나면 그 tick 에 치워지고 [`Event::ItemDespawned`] 가 난다.
    pub fn set_ground_item_lifetime(&mut self, lifetime: Option<Duration>) {
        self.ground_item_lifetime = lifetime.filter(|d| !d.is_zero());
    }

    /// 땅 아이템이 사라지기까지 남은 시간. 소멸 시간이 없으면 `None`.
    #[must_use]
    pub fn ground_item_time_left(&self, item: &GroundItem) -> Option<Duration> {
        self.ground_item_lifetime
            .map(|life| (item.dropped_at + life).saturating_sub(self.now))
    }

    #[must_use]
    pub fn pickup_range(&self) -> f32 {
        self.pickup_range
    }

    /// 스킬을 등록한다 (설정 작업). 같은 번호는 덮어쓴다.
    pub fn define_skill(&mut self, id: SkillId, def: SkillDef) {
        self.skills.insert(id, def);
    }

    #[must_use]
    pub fn skill(&self, id: SkillId) -> Option<&SkillDef> {
        self.skills.get(&id)
    }

    /// 시뮬레이션 시작 후 흐른 시간.
    #[must_use]
    pub fn now(&self) -> Duration {
        self.now
    }

    /// 두 진영의 관계를 정한다 (설정 작업). 대칭이다.
    pub fn set_relation(&mut self, a: FactionId, b: FactionId, relation: Relation) {
        self.factions.set(a, b, relation);
    }

    #[must_use]
    pub fn relation(&self, a: FactionId, b: FactionId) -> Relation {
        self.factions.get(a, b)
    }

    #[must_use]
    pub fn tiles(&self) -> &TileMap {
        &self.tiles
    }

    /// 타일맵 수정 — 에디터가 칠한 결과를 반영할 때.
    /// 이동 중인 유닛은 다음 tick 에 막힌 걸음을 발견하면 [`Event::Blocked`] 로 멈춘다.
    pub fn tiles_mut(&mut self) -> &mut TileMap {
        &mut self.tiles
    }

    /// 유닛을 만든다. 위치는 타일맵 밖이어도 되지만, 그러면 움직일 수 없다.
    pub fn spawn_unit(&mut self, pos: Vec2, heading: f32, def: UnitDef) -> Entity {
        let entity = self.entities.spawn();
        let slot = entity.index() as usize;
        if self.units.len() <= slot {
            self.units.resize_with(slot + 1, || None);
        }
        let mut unit = Unit::new(pos, heading, def);
        unit.progress = Progress::with_curve(1, 0, |lv| self.exp_needed(lv));
        self.units[slot] = Some((entity, unit));
        // 직업이 있으면 수식으로 계산한 값으로 가득 채운 채 선다.
        self.refresh_stats(entity);
        if let Some(u) = self.unit_mut(entity) {
            u.hp = u.max_hp();
            u.mp = u.max_mp();
        }
        entity
    }

    /// 유닛이나 땅의 아이템을 제거한다. 이미 없거나 낡은 핸들이면 `false`.
    pub fn despawn(&mut self, entity: Entity) -> bool {
        if !self.entities.despawn(entity) {
            return false;
        }
        let slot = entity.index() as usize;
        if let Some(s) = self.units.get_mut(slot) {
            *s = None;
        }
        if let Some(s) = self.ground.get_mut(slot) {
            *s = None;
        }
        true
    }

    /// 땅에 아이템을 놓는다 (설정 작업 — 게임 중에는 드롭·버리기가 놓는다).
    pub fn spawn_ground_item(&mut self, pos: Vec2, stack: ItemStack) -> Entity {
        let entity = self.entities.spawn();
        let slot = entity.index() as usize;
        if self.ground.len() <= slot {
            self.ground.resize_with(slot + 1, || None);
        }
        let dropped_at = self.now;
        self.ground[slot] = Some((
            entity,
            GroundItem {
                pos,
                stack,
                dropped_at,
            },
        ));
        entity
    }

    #[must_use]
    pub fn ground_item(&self, entity: Entity) -> Option<&GroundItem> {
        if !self.entities.is_alive(entity) {
            return None;
        }
        self.ground
            .get(entity.index() as usize)?
            .as_ref()
            .map(|(_, g)| g)
    }

    /// 땅의 아이템 전부 (슬롯 순서).
    pub fn ground_items(&self) -> impl Iterator<Item = (Entity, &GroundItem)> {
        self.ground.iter().flatten().map(|(e, g)| (*e, g))
    }

    /// 유닛 가방에 아이템을 넣는다 (설정 작업 — 시작 소지품). 다 들어가지 않으면 아무것도 넣지 않고 `false`.
    pub fn give_item(&mut self, unit: Entity, item: ItemId, count: u32) -> bool {
        let Some(def) = self.items.get(&item).copied() else {
            return false;
        };
        let Some(u) = self.unit_mut(unit) else {
            return false;
        };
        let bag = u.inventory.bag_mut(def.bag());
        if count == 0 || !bag.fits(item, count, def.stack_limit()) {
            return false;
        }
        bag.add(item, count, def.stack_limit());
        true
    }

    #[must_use]
    pub fn unit(&self, entity: Entity) -> Option<&Unit> {
        if !self.entities.is_alive(entity) {
            return None;
        }
        self.units
            .get(entity.index() as usize)?
            .as_ref()
            .map(|(_, unit)| unit)
    }

    pub(crate) fn unit_mut(&mut self, entity: Entity) -> Option<&mut Unit> {
        if !self.entities.is_alive(entity) {
            return None;
        }
        self.units
            .get_mut(entity.index() as usize)?
            .as_mut()
            .map(|(_, unit)| unit)
    }

    /// 살아 있는 유닛 전부 (슬롯 순서).
    pub fn units(&self) -> impl Iterator<Item = (Entity, &Unit)> {
        self.units.iter().flatten().map(|(e, u)| (*e, u))
    }

    /// 유닛 수 (시체 포함, 땅의 아이템 제외).
    #[must_use]
    pub fn unit_count(&self) -> u32 {
        self.units.iter().flatten().count() as u32
    }

    /// `target` 까지 경로를 잡는다. 이전 경로는 버린다.
    ///
    /// 경유점은 경로 타일의 중심이고 마지막만 `target` 그 자체다. 출발 타일 중심은 넣지
    /// 않는다 — 돌아갔다 오는 걸음이 생긴다. 직교 이웃이면 두 칸의 합집합이, 대각
    /// 이웃이면 (양옆까지 통과 가능하다는 [`TileMap::can_step`] 조건 덕에) 2×2 칸이
    /// 볼록하므로 경유점 사이 직선이 금지된 칸을 지나지 않는다.
    pub(crate) fn plan_move(&mut self, entity: Entity, target: Vec2) -> Result<(), Rejection> {
        let unit = self.unit(entity).ok_or(Rejection::UnknownEntity)?;
        if !unit.is_alive() {
            return Err(Rejection::Dead);
        }
        if unit.def.move_speed <= 0.0 {
            // 경로를 잡아 두면 영원히 "이동 중" 으로 남는다.
            return Err(Rejection::Immobile);
        }
        let from = self.tiles.world_to_tile(unit.pos);
        let to = self.tiles.world_to_tile(target);
        let path = self.tiles.find_path(from, to).ok_or(Rejection::NoPath)?;

        let mut waypoints: std::collections::VecDeque<Vec2> = path
            .iter()
            .skip(1)
            .map(|&c| self.tiles.tile_center(c))
            .collect();
        waypoints.pop_back();
        waypoints.push_back(target);

        let unit = self.unit_mut(entity).ok_or(Rejection::UnknownEntity)?;
        unit.waypoints = waypoints;
        Ok(())
    }

    pub(crate) fn stop(&mut self, entity: Entity) -> Result<(), Rejection> {
        let unit = self.unit_mut(entity).ok_or(Rejection::UnknownEntity)?;
        unit.waypoints.clear();
        Ok(())
    }

    /// `attacker` 가 `skill` 로 `target` 을 친다. 판정 순서는 서버와 같다 ([`combat`] 모듈).
    ///
    /// 성공하면 [`Event::Damaged`] 를, 그 공격으로 죽으면 [`Event::Died`] 를 더 낸다.
    /// 공격해도 이동은 멈추지 않는다 — 멈출지는 Intent 를 내는 쪽이 정한다.
    pub(crate) fn attack(
        &mut self,
        attacker: Entity,
        target: Entity,
        skill: SkillId,
        events: &mut Vec<Event>,
    ) -> Result<(), Rejection> {
        let def = self
            .effective_skill(attacker, skill)
            .ok_or(Rejection::UnknownSkill)?;
        // 회복은 자신에게도 쓴다. 피해는 자신에게 쓸 수 없다.
        if attacker == target && !def.is_heal() {
            return Err(Rejection::InvalidTarget);
        }
        let a = self.unit(attacker).ok_or(Rejection::UnknownEntity)?;
        let t = self.unit(target).ok_or(Rejection::UnknownEntity)?;

        // 서버 CombatProcessor 와 같은 순서.
        if !a.is_alive() {
            return Err(Rejection::Dead);
        }
        if !t.is_alive() {
            return Err(Rejection::TargetDead);
        }
        if !a.is_ready(skill, self.now) {
            return Err(Rejection::OnCooldown);
        }
        if !a.has_mp(def.mp_cost) {
            return Err(Rejection::NotEnoughMp);
        }
        if a.pos.distance_squared(t.pos) > def.range * def.range {
            return Err(Rejection::OutOfRange);
        }
        let relation = self.factions.get(a.def.faction, t.def.faction);
        if def.is_heal() {
            if attacker != target && relation == Relation::Hostile {
                return Err(Rejection::InvalidTarget);
            }
        } else {
            if t.def.immortal || t.is_protected(self.now) {
                return Err(Rejection::Invulnerable);
            }
            if relation == Relation::Friendly {
                return Err(Rejection::Friendly);
            }
        }
        // 서버와 겹치는 판정 뒤에 붙인 것 (R3) — 직업이 있는 유닛은 배운 스킬과 기본 공격만 쓴다.
        if a.def.class.is_some() && a.def.basic_attack != Some(skill) && a.skill_level(skill) == 0 {
            return Err(Rejection::SkillNotLearned);
        }

        // 맞는 유닛 — 대상이 먼저, 그다음 범위 안의 다른 유닛을 슬롯 순서대로 (결정적).
        let mut hits = vec![target];
        if def.radius > 0.0 {
            let (center, now) = (t.pos, self.now);
            let faction = a.def.faction;
            let r2 = def.radius * def.radius;
            for (e, u) in self.units() {
                if e == target || !u.is_alive() || u.pos.distance_squared(center) > r2 {
                    continue;
                }
                let rel = self.factions.get(faction, u.def.faction);
                let ok = if def.is_heal() {
                    e == attacker || rel == Relation::Friendly
                } else {
                    e != attacker
                        && rel == Relation::Hostile
                        && !u.def.immortal
                        && !u.is_protected(now)
                };
                if ok {
                    hits.push(e);
                }
            }
        }

        let facing = t.pos - a.pos;
        let now = self.now;
        let a = self.unit_mut(attacker).ok_or(Rejection::UnknownEntity)?;
        a.cooldowns.insert(skill, now + def.cooldown());
        a.mp -= def.mp_cost;
        if facing.length_squared() > ARRIVE_EPSILON * ARRIVE_EPSILON {
            a.heading = dir_to_heading(facing);
        }

        for target in hits {
            if def.is_heal() {
                if let Some(t) = self.unit_mut(target) {
                    let before = t.hp;
                    t.hp = t.hp.saturating_add(def.heal).min(t.max_hp());
                    events.push(Event::Healed {
                        unit: target,
                        amount: t.hp - before,
                        remaining_hp: t.hp,
                    });
                }
            } else {
                self.strike(attacker, target, skill, def, events);
            }
        }
        Ok(())
    }

    /// 피해 한 번 — 판정을 마친 뒤에 부른다. 범위 스킬이면 맞는 유닛마다 한 번씩.
    fn strike(
        &mut self,
        attacker: Entity,
        target: Entity,
        skill: SkillId,
        def: SkillDef,
        events: &mut Vec<Event>,
    ) {
        let now = self.now;
        // 장비가 더해진 수치로 계산한다.
        let Some(attack) = self.unit(attacker).map(Unit::attack) else {
            return;
        };
        let Some(t) = self.unit_mut(target) else {
            return;
        };
        let amount = combat::damage(attack, def.damage_mult, t.defense());
        t.hp = t.hp.saturating_sub(amount);
        let remaining_hp = t.hp;
        events.push(Event::Damaged {
            attacker,
            target,
            skill,
            amount,
            remaining_hp,
        });
        if remaining_hp == 0 {
            t.waypoints.clear();
            t.ai = AiState::default();
            t.died_at = Some(now);
            let (corpse, table, reward) = (t.pos, t.def.loot, t.def.exp_reward);
            events.push(Event::Died {
                unit: target,
                killer: attacker,
            });
            self.drop_loot(table, corpse, events);
            self.grant_exp(attacker, reward, events);
        } else if t.def.ai != AiKind::Passive && t.ai.target.is_none() && !t.ai.returning {
            // 반격 — 싸우는 상대가 없을 때만 갈아탄다. 귀환 중에는 받지 않는다.
            t.ai.target = Some(attacker);
            events.push(Event::Engaged {
                unit: target,
                target: attacker,
            });
        }
    }

    /// 죽인 쪽에 경험치를 준다 (P3). 레벨이 오르면 **HP·MP 가 가득 찬다** — 상한이 올라가므로.
    fn grant_exp(&mut self, unit: Entity, amount: u32, events: &mut Vec<Event>) {
        if amount == 0 {
            return;
        }
        let Some(from) = self
            .unit(unit)
            .filter(|u| u.is_alive())
            .map(|u| u.progress.level())
        else {
            return;
        };
        let curve = self.exp_curve.clone();
        let Some(u) = self.unit_mut(unit) else {
            return;
        };
        let levels = u
            .progress
            .add_with(amount, |lv| exp_needed(curve.as_ref(), lv));
        let progress = u.progress;
        events.push(Event::ExperienceGained {
            unit,
            amount,
            progress,
        });
        if levels > 0 {
            self.refresh_stats(unit);
            let Some(u) = self.unit_mut(unit) else {
                return;
            };
            let full = u.max_hp();
            u.hp = full;
            u.mp = u.max_mp();
            u.mp_carry = 0;
            events.push(Event::LeveledUp {
                unit,
                from,
                level: progress.level(),
            });
        }
    }

    /// 드롭 테이블을 굴려 시체 자리에 떨어뜨린다.
    fn drop_loot(&mut self, table: Option<LootTableId>, at: Vec2, events: &mut Vec<Event>) {
        let Some(entries) = table.and_then(|id| self.loot_tables.get(&id)) else {
            return;
        };
        for (item, count) in loot::roll(entries, &mut self.rng) {
            let stack = ItemStack { item, count };
            let spawned = self.spawn_ground_item(at, stack);
            events.push(Event::ItemSpawned {
                item: spawned,
                stack,
                pos: at,
            });
        }
    }

    // ── 아이템 Intent ────────────────────────────────────────────────────────

    /// 살아 있는 유닛을 찾는다 — 아이템 Intent 공통 전제.
    fn living(&self, unit: Entity) -> Result<&Unit, Rejection> {
        let u = self.unit(unit).ok_or(Rejection::UnknownEntity)?;
        if u.is_alive() {
            Ok(u)
        } else {
            Err(Rejection::Dead)
        }
    }

    pub(crate) fn pick_up(
        &mut self,
        unit: Entity,
        item: Entity,
        events: &mut Vec<Event>,
    ) -> Result<(), Rejection> {
        let u = self.living(unit)?;
        let ground = *self.ground_item(item).ok_or(Rejection::UnknownItem)?;
        let def = *self
            .items
            .get(&ground.stack.item)
            .ok_or(Rejection::UnknownItem)?;
        if u.pos.distance_squared(ground.pos) > self.pickup_range * self.pickup_range {
            return Err(Rejection::OutOfRange);
        }
        let bag = u.inventory.bag(def.bag());
        if !bag.fits(ground.stack.item, ground.stack.count, def.stack_limit()) {
            // 일부만 줍지 않는다 — 땅의 더미가 쪼개지면 서버와 맞추기 어렵다.
            return Err(Rejection::InventoryFull);
        }

        self.despawn(item);
        let u = self.unit_mut(unit).ok_or(Rejection::UnknownEntity)?;
        u.inventory.bag_mut(def.bag()).add(
            ground.stack.item,
            ground.stack.count,
            def.stack_limit(),
        );
        events.push(Event::PickedUp {
            unit,
            item,
            stack: ground.stack,
        });
        Ok(())
    }

    /// 가방 한 칸을 통째로 발밑에 버린다.
    pub(crate) fn drop_item(
        &mut self,
        unit: Entity,
        bag: BagKind,
        slot: u16,
        events: &mut Vec<Event>,
    ) -> Result<(), Rejection> {
        let u = self.living(unit)?;
        let slot = usize::from(slot);
        let stack = u.inventory.bag(bag).get(slot).ok_or(Rejection::EmptySlot)?;
        let pos = u.pos;

        let u = self.unit_mut(unit).ok_or(Rejection::UnknownEntity)?;
        u.inventory.bag_mut(bag).put(slot, None);
        let spawned = self.spawn_ground_item(pos, stack);
        events.push(Event::ItemSpawned {
            item: spawned,
            stack,
            pos,
        });
        Ok(())
    }

    /// 소모품 가방의 `slot` 에서 하나를 쓴다. HP·MP 가 가득이어도 쓰인다 (RO 와 같다).
    ///
    /// 회복하는 쪽마다 이벤트를 낸다 — MP 만 채우는 물약은 [`Event::ManaRestored`] 만,
    /// 아무것도 채우지 않는 소모품은 [`Event::Healed`] (양 0) 를 낸다.
    pub(crate) fn use_item(
        &mut self,
        unit: Entity,
        slot: u16,
        events: &mut Vec<Event>,
    ) -> Result<(), Rejection> {
        let u = self.living(unit)?;
        let slot = usize::from(slot);
        let stack = u
            .inventory
            .consumables
            .get(slot)
            .ok_or(Rejection::EmptySlot)?;
        let def = self.items.get(&stack.item).ok_or(Rejection::UnknownItem)?;
        let (heal, mana) = match def.kind {
            ItemKind::Consumable { heal, mana } => (heal, mana),
            // 스킬북 (R3) — 배울 수 있을 때만 사라진다.
            ItemKind::SkillBook { skill } => {
                self.learn(unit, skill, 1, events)?;
                if let Some(u) = self.unit_mut(unit) {
                    u.inventory.consumables.take_one(slot);
                }
                return Ok(());
            }
            ItemKind::Equipment { .. } => return Err(Rejection::NotUsable),
        };

        let u = self.unit_mut(unit).ok_or(Rejection::UnknownEntity)?;
        u.inventory.consumables.take_one(slot);
        if heal > 0 || mana == 0 {
            let before = u.hp;
            let cap = u.max_hp();
            u.hp = u.hp.saturating_add(heal).min(cap);
            events.push(Event::Healed {
                unit,
                amount: u.hp - before,
                remaining_hp: u.hp,
            });
        }
        if mana > 0 {
            let before = u.mp;
            let cap = u.max_mp();
            u.mp = u.mp.saturating_add(mana).min(cap);
            events.push(Event::ManaRestored {
                unit,
                amount: u.mp - before,
                remaining_mp: u.mp,
            });
        }
        Ok(())
    }

    /// 장비 가방의 `slot` 을 장착한다. 그 자리에 있던 장비는 **같은 칸**으로 돌아온다 (맞바꿈).
    pub(crate) fn equip(
        &mut self,
        unit: Entity,
        slot: u16,
        events: &mut Vec<Event>,
    ) -> Result<(), Rejection> {
        let u = self.living(unit)?;
        let bag_slot = usize::from(slot);
        let stack = u
            .inventory
            .equipment
            .get(bag_slot)
            .ok_or(Rejection::EmptySlot)?;
        let def = self.items.get(&stack.item).ok_or(Rejection::UnknownItem)?;
        let ItemKind::Equipment { slot: place, .. } = def.kind else {
            return Err(Rejection::NotUsable);
        };
        if self
            .equip_rules
            .get(&stack.item)
            .is_some_and(|r| !r.allows(u.def.class))
        {
            return Err(Rejection::ClassRestricted);
        }

        let u = self.unit_mut(unit).ok_or(Rejection::UnknownEntity)?;
        let previous = u.inventory.equipped(place);
        u.inventory.set_equipped(place, Some(stack.item));
        u.inventory
            .equipment
            .put(bag_slot, previous.map(|item| ItemStack { item, count: 1 }));
        self.refresh_stats(unit);
        events.push(Event::EquipmentChanged { unit, slot: place });
        Ok(())
    }

    /// `place` 의 장비를 벗어 장비 가방 빈칸에 넣는다.
    pub(crate) fn unequip(
        &mut self,
        unit: Entity,
        place: EquipSlot,
        events: &mut Vec<Event>,
    ) -> Result<(), Rejection> {
        let u = self.living(unit)?;
        let item = u.inventory.equipped(place).ok_or(Rejection::EmptySlot)?;
        if !u.inventory.equipment.fits(item, 1, 1) {
            return Err(Rejection::InventoryFull);
        }

        let u = self.unit_mut(unit).ok_or(Rejection::UnknownEntity)?;
        u.inventory.set_equipped(place, None);
        u.inventory.equipment.add(item, 1, 1);
        self.refresh_stats(unit);
        events.push(Event::EquipmentChanged { unit, slot: place });
        Ok(())
    }

    /// 장착 장비의 수치 합을 유닛에 다시 적는다. 전투는 매번 정의를 찾지 않고 이 값을 쓴다.
    /// 장비 합계와 (직업이 있으면) 스탯 시트를 다시 계산한다 — 스폰·레벨·장착이 바뀔 때.
    /// 현재 HP·MP 는 건드리지 않는다 (상한을 넘으면 부른 쪽이 자른다).
    fn refresh_stats(&mut self, unit: Entity) {
        let Some(u) = self.unit(unit) else { return };
        let mut equip = EquipTotals::default();
        for place in EquipSlot::ALL {
            let Some(item) = u.inventory.equipped(place) else {
                continue;
            };
            if let Some(ItemKind::Equipment {
                attack: a,
                defense: d,
                ..
            }) = self.items.get(&item).map(|d| d.kind)
            {
                equip.attack = equip.attack.saturating_add(a);
                equip.defense = equip.defense.saturating_add(d);
            }
            for &(stat, add) in self.equip_rules.get(&item).map_or(&[][..], |r| &r.stats) {
                if equip.stats.len() <= stat {
                    equip.stats.resize(stat + 1, 0);
                }
                equip.stats[stat] = equip.stats[stat].saturating_add(add);
            }
        }
        let sheet = match (&self.stat_rules, u.def.class) {
            (Some(rules), Some(class)) => self
                .classes
                .get(&class)
                .map(|c| rules.sheet(c, u.progress.level(), &equip)),
            _ => None,
        };
        if let Some(u) = self.unit_mut(unit) {
            u.bonus_attack = equip.attack;
            u.bonus_defense = equip.defense;
            u.sheet = sheet;
        }
    }

    /// 소멸 시간이 지난 땅의 아이템을 치운다 — 슬롯 순서대로 (결정적).
    fn expire_ground_items(&mut self, events: &mut Vec<Event>) {
        let Some(life) = self.ground_item_lifetime else {
            return;
        };
        let now = self.now;
        let due: Vec<Entity> = self
            .ground_items()
            .filter(|(_, g)| now >= g.dropped_at + life)
            .map(|(e, _)| e)
            .collect();
        for item in due {
            self.despawn(item);
            events.push(Event::ItemDespawned { item });
        }
    }

    /// 쓰러진 유닛을 `at` 에서 일으킨다 ([`Intent::Revive`](crate::Intent::Revive)).
    ///
    /// 핸들은 그대로다(리스폰은 새 핸들) — 레벨·소지품·장비가 남는다. HP·MP 는 최대치의 천분율,
    /// 최소 1. 걷던 길·AI 상대·쿨타임·리스폰 예약은 지운다. 자리는 검사하지 않는다 —
    /// 막힌 칸이면 움직일 때 [`Event::Blocked`] 로 멈출 뿐이다 (스크립트의 책임).
    pub(crate) fn revive(
        &mut self,
        unit: Entity,
        at: Vec2,
        hp_per_mille: u32,
        exp_loss_per_mille: u32,
        protect: Duration,
        events: &mut Vec<Event>,
    ) -> Result<(), Rejection> {
        let now = self.now;
        if !at.is_finite() {
            return Err(Rejection::NoPath);
        }
        let u = self.unit_mut(unit).ok_or(Rejection::UnknownEntity)?;
        if u.is_alive() {
            return Err(Rejection::NotDead);
        }
        let ratio = u64::from(hp_per_mille.min(1000));
        let part = |max: u32| u32::try_from(u64::from(max) * ratio / 1000).unwrap_or(max);
        u.hp = part(u.max_hp()).max(1);
        u.mp = part(u.max_mp());
        u.mp_carry = 0;
        u.pos = at;
        u.prev_pos = at;
        u.home = at;
        u.waypoints.clear();
        u.ai = AiState::default();
        u.cooldowns.clear();
        u.died_at = None;
        u.protected_until = (!protect.is_zero()).then(|| now + protect);
        let exp_lost = u.progress.lose(exp_loss_per_mille);
        let hp = u.hp;
        events.push(Event::Revived {
            unit,
            pos: at,
            hp,
            exp_lost,
        });
        Ok(())
    }

    /// 리스폰 시각이 된 시체를 치우고 스폰 지점에 새로 세운다 — 슬롯 순서대로 (결정적).
    ///
    /// 새 유닛은 **처음 스폰과 같은 수치**(마커별 덮어쓰기가 얹힌 `UnitDef`)로 가득 찬 채 나타나고,
    /// 레벨·소지품·쿨타임은 없다. 핸들이 새로 나오므로 [`Event::Respawned`] 로 알린다.
    fn respawn_due(&mut self, events: &mut Vec<Event>) {
        let now = self.now;
        let due: Vec<(Entity, UnitDef, Vec2, f32)> = self
            .units()
            .filter(|(_, u)| {
                !u.is_alive()
                    && u.def.respawn_ms > 0
                    && u.died_at.is_some_and(|t| {
                        now >= t + Duration::from_millis(u64::from(u.def.respawn_ms))
                    })
            })
            .map(|(e, u)| (e, u.def, u.spawn_pos, u.spawn_heading))
            .collect();
        for (old, def, pos, heading) in due {
            self.despawn(old);
            let unit = self.spawn_unit(pos, heading, def);
            events.push(Event::Respawned {
                unit,
                replaces: old,
            });
        }
    }

    /// 이번 tick 의 일에 트리거를 맞춰 본다 (R1) — `step` 뒤에 부른다. 판정 순서는 `trigger` 모듈 doc.
    ///
    /// 첫 라운드는 이 tick 에 움직인 유닛(들어옴 판정) → 이벤트 순이고, 효과가 낸 이벤트는
    /// 다음 라운드에서 본다 ([`MAX_ROUNDS`] 까지).
    pub(crate) fn run_triggers(&mut self, events: &mut Vec<Event>) {
        if self.triggers.is_empty() {
            return;
        }
        let ids: Vec<TriggerId> = self.triggers.keys().copied().collect();
        let mut happenings: Vec<Happening> = self
            .units()
            .filter(|(_, u)| u.prev_pos != u.pos)
            .map(|(unit, u)| Happening::Moved {
                unit,
                from: u.prev_pos,
                to: u.pos,
            })
            .collect();
        let mut seen = 0;
        for _ in 0..MAX_ROUNDS {
            let end = events.len();
            happenings.extend(
                events[seen..end]
                    .iter()
                    .filter_map(|e| self.happening(e, &events[..end])),
            );
            seen = end;
            if happenings.is_empty() {
                break;
            }
            for happened in std::mem::take(&mut happenings) {
                for &id in &ids {
                    let Some(def) = self.triggers.get(&id) else {
                        continue;
                    };
                    let Some(subject) = happened.matches(def.on) else {
                        continue;
                    };
                    if !self.check(&def.when, subject) {
                        continue;
                    }
                    let effects = def.then.clone();
                    events.push(Event::TriggerFired {
                        trigger: id,
                        subject,
                    });
                    for effect in effects {
                        self.apply_effect(effect, subject, events);
                    }
                }
            }
        }
    }

    /// 트리거가 반응할 수 있는 이벤트면 그 일. `batch` 는 같은 tick 의 이벤트 — 쓰러진 쪽이 같은 tick 에
    /// 리스폰해 핸들이 바뀌었으면 거기서 새 유닛을 찾아 타입을 읽는다.
    fn happening(&self, event: &Event, batch: &[Event]) -> Option<Happening> {
        match *event {
            Event::Died { unit, killer } => {
                let victim = self
                    .unit(unit)
                    .or_else(|| {
                        batch.iter().find_map(|e| match *e {
                            Event::Respawned {
                                unit: new,
                                replaces,
                            } if replaces == unit => self.unit(new),
                            _ => None,
                        })
                    })
                    .map(|u| u.def.actor);
                Some(Happening::Killed { killer, victim })
            }
            Event::LeveledUp { unit, from, level } => Some(Happening::Leveled {
                unit,
                from,
                to: level,
            }),
            Event::PickedUp { unit, stack, .. } => Some(Happening::PickedUp {
                unit,
                item: stack.item,
            }),
            _ => None,
        }
    }

    /// 효과 하나를 적용한다. 바뀐 것만 이벤트로 알린다 (같은 값으로 `SetFlag` 하면 조용하다).
    fn apply_effect(&mut self, effect: Effect, subject: Entity, events: &mut Vec<Event>) {
        match effect {
            Effect::SetFlag { scope, flag, value } => {
                self.write_flag(scope, flag, subject, |_| value, events);
            }
            Effect::AddFlag { scope, flag, delta } => {
                self.write_flag(scope, flag, subject, |v| v.saturating_add(delta), events);
            }
            Effect::GiveItem { item, count } => {
                let Some(def) = self.items.get(&item).copied() else {
                    return;
                };
                let Some(u) = self.unit_mut(subject).filter(|_| count > 0) else {
                    return;
                };
                let stack = ItemStack { item, count };
                let bag = u.inventory.bag_mut(def.bag());
                if bag.fits(item, count, def.stack_limit()) {
                    bag.add(item, count, def.stack_limit());
                    events.push(Event::ItemReceived {
                        unit: subject,
                        stack,
                    });
                } else {
                    // 가방이 차도 보상을 잃지 않는다 — 발밑에 떨어뜨려 비운 뒤 줍게 한다.
                    let pos = u.pos;
                    let ground = self.spawn_ground_item(pos, stack);
                    events.push(Event::ItemSpawned {
                        item: ground,
                        stack,
                        pos,
                    });
                }
            }
            Effect::TakeItem { item, count } => {
                let Some(def) = self.items.get(&item).copied() else {
                    return;
                };
                let Some(u) = self.unit_mut(subject) else {
                    return;
                };
                let taken = u.inventory.bag_mut(def.bag()).remove(item, count);
                if taken > 0 {
                    events.push(Event::ItemTaken {
                        unit: subject,
                        item,
                        count: taken,
                    });
                }
            }
            Effect::GiveExp(amount) => self.grant_exp(subject, amount, events),
            Effect::GrantSkill { skill, levels } => {
                let _ = self.learn(subject, skill, levels, events);
            }
            Effect::RemoveSkill { skill } => self.forget(subject, skill, events),
        }
    }

    fn write_flag(
        &mut self,
        scope: FlagScope,
        flag: FlagId,
        subject: Entity,
        f: impl FnOnce(i32) -> i32,
        events: &mut Vec<Event>,
    ) {
        let (flags, unit) = match scope {
            FlagScope::Character => match self.unit_mut(subject) {
                Some(u) => (&mut u.flags, Some(subject)),
                None => return,
            },
            FlagScope::World => (&mut self.world_flags, None),
        };
        let old = flags.get(flag);
        let value = f(old);
        if value != old {
            flags.set(flag, value);
            events.push(Event::FlagChanged { unit, flag, value });
        }
    }

    /// 한 tick 진행.
    pub(crate) fn step(&mut self, dt: Duration, events: &mut Vec<Event>) {
        self.now += dt;
        self.respawn_due(events);
        self.expire_ground_items(events);
        let tick = dt;
        let dt = dt.as_secs_f32();
        let tiles = &self.tiles;
        for (entity, unit) in self.units.iter_mut().flatten() {
            unit.prev_pos = unit.pos;
            if unit.is_alive() {
                unit.regen_mp(tick);
            }
            if unit.waypoints.is_empty() || !unit.is_alive() {
                continue;
            }
            match advance(tiles, unit, unit.def.move_speed * dt) {
                Step::Moving => {}
                Step::Arrived => events.push(Event::Arrived { unit: *entity }),
                Step::Blocked => events.push(Event::Blocked { unit: *entity }),
            }
        }
    }
}

/// 곡선(없으면 기본 곡선)으로 `level` 의 필요 경험치. 결과는 1 이상.
fn exp_needed(curve: Option<&Formula>, level: u32) -> u32 {
    curve.map_or_else(
        || crate::progress::exp_to_next(level),
        |f| u32::try_from(f.eval(&[i64::from(level)]).max(1)).unwrap_or(u32::MAX),
    )
}

enum Step {
    Moving,
    Arrived,
    Blocked,
}

/// 유닛을 경유점을 따라 `budget` 미터만큼 움직인다.
///
/// 한 tick 에 경유점 여러 개를 지날 수 있다 — 남은 거리를 다음 구간으로 넘긴다.
/// 다른 타일로 넘어가는 구간은 **매번 규칙을 다시 확인한다.** 경로를 잡은 뒤에
/// 타일이 바뀌었을 수 있기 때문이다.
fn advance(tiles: &TileMap, unit: &mut Unit, mut budget: f32) -> Step {
    while let Some(&next) = unit.waypoints.front() {
        let here = tiles.world_to_tile(unit.pos);
        let there = tiles.world_to_tile(next);
        if here != there && !tiles.can_step(here, there) {
            unit.waypoints.clear();
            return Step::Blocked;
        }

        let delta = next - unit.pos;
        let dist = delta.length();
        if dist > ARRIVE_EPSILON {
            unit.heading = dir_to_heading(delta);
        }
        if dist <= budget + ARRIVE_EPSILON {
            unit.pos = next;
            budget = (budget - dist).max(0.0);
            unit.waypoints.pop_front();
        } else {
            unit.pos += delta / dist * budget;
            return Step::Moving;
        }
    }
    Step::Arrived
}
