//! 시뮬레이션 월드 — 타일맵 + 유닛 저장소 + 이동.
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

use std::collections::HashMap;
use std::time::Duration;

use nexus_core::units::dir_to_heading;
use nexus_core::{Entity, Vec2, World};

use crate::authority::{Event, Rejection};
use crate::combat::{self, SkillDef, SkillId};
use crate::faction::{FactionId, FactionTable, Relation};
use crate::tilemap::TileMap;
use crate::unit::{AiKind, AiState, Unit, UnitDef};

/// 도착 판정 거리 (m). 부동소수 오차로 경유점 바로 앞에서 멈추지 않게 한다.
const ARRIVE_EPSILON: f32 = 1e-4;

/// 시뮬레이션 상태 전체.
#[derive(Debug)]
pub struct SimWorld {
    tiles: TileMap,
    entities: World,
    /// 엔티티 슬롯 번호로 색인. 핸들을 함께 두어 순회 시 되돌려 준다.
    units: Vec<Option<(Entity, Unit)>>,
    /// 스킬 정적 데이터. 존을 읽을 때 채운다.
    skills: HashMap<SkillId, SkillDef>,
    factions: FactionTable,
    /// 시뮬레이션 시계 — tick 마다 `dt` 만큼 정확히 늘어난다. 쿨타임의 기준.
    now: Duration,
}

impl SimWorld {
    #[must_use]
    pub fn new(tiles: TileMap) -> Self {
        Self {
            tiles,
            entities: World::default(),
            units: Vec::new(),
            skills: HashMap::new(),
            factions: FactionTable::default(),
            now: Duration::ZERO,
        }
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
        self.units[slot] = Some((entity, Unit::new(pos, heading, def)));
        entity
    }

    /// 유닛을 제거한다. 이미 없거나 낡은 핸들이면 `false`.
    pub fn despawn(&mut self, entity: Entity) -> bool {
        if !self.entities.despawn(entity) {
            return false;
        }
        self.units[entity.index() as usize] = None;
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

    #[must_use]
    pub fn unit_count(&self) -> u32 {
        self.entities.entity_count()
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
        let def = *self.skills.get(&skill).ok_or(Rejection::UnknownSkill)?;
        if attacker == target {
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
        if a.pos.distance_squared(t.pos) > def.range * def.range {
            return Err(Rejection::OutOfRange);
        }
        if t.def.immortal {
            return Err(Rejection::Invulnerable);
        }
        if self.factions.get(a.def.faction, t.def.faction) == Relation::Friendly {
            return Err(Rejection::Friendly);
        }

        let amount = combat::damage(a.def.attack, def.damage_mult, t.def.defense);
        let facing = t.pos - a.pos;

        let now = self.now;
        let a = self.unit_mut(attacker).ok_or(Rejection::UnknownEntity)?;
        a.cooldowns.insert(skill, now + def.cooldown());
        if facing.length_squared() > ARRIVE_EPSILON * ARRIVE_EPSILON {
            a.heading = dir_to_heading(facing);
        }

        let t = self.unit_mut(target).ok_or(Rejection::UnknownEntity)?;
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
            events.push(Event::Died {
                unit: target,
                killer: attacker,
            });
        } else if t.def.ai != AiKind::Passive && t.ai.target.is_none() && !t.ai.returning {
            // 반격 — 싸우는 상대가 없을 때만 갈아탄다. 귀환 중에는 받지 않는다.
            t.ai.target = Some(attacker);
            events.push(Event::Engaged {
                unit: target,
                target: attacker,
            });
        }
        Ok(())
    }

    /// 한 tick 진행.
    pub(crate) fn step(&mut self, dt: Duration, events: &mut Vec<Event>) {
        self.now += dt;
        let dt = dt.as_secs_f32();
        let tiles = &self.tiles;
        for (entity, unit) in self.units.iter_mut().flatten() {
            unit.prev_pos = unit.pos;
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
