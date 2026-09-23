//! 유닛 — 지면 위를 움직이는 시뮬레이션 대상 (플레이어·NPC·몬스터 공통).
//!
//! 수치([`UnitDef`])는 코드가 아니라 **데이터**다. 스폰하는 쪽이 넘겨준다.

use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use nexus_core::{Entity, Vec2};

use crate::combat::SkillId;
use crate::faction::FactionId;
use crate::item::Inventory;
use crate::loot::LootTableId;
use crate::progress::{Growth, Progress};

/// 전투 AI 행동 유형. 서버 `EAIType` 과 같은 의미다.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AiKind {
    /// AI 없음 / 공격받아도 반격하지 않는다 (플레이어·토끼·상인). **기본값.**
    #[default]
    Passive,
    /// 공격받으면 반격하고, 먼저 덤비지 않는다 (경비병).
    Defensive,
    /// 어그로 범위 안의 **적대** 진영을 보면 먼저 덤빈다 (슬라임).
    Aggressive,
}

/// 유닛 종류별 정적 수치. 스폰 시 복사되어 유닛마다 따로 갖는다.
///
/// 전투·AI 수치는 서버 `GameDataEntityBase` (maxHp / attack / defense / factionId / aiType /
/// aggroRange) 와 같은 의미다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnitDef {
    /// 이동 속도 (m/s). 음수·NaN 은 0 으로 본다.
    pub move_speed: f32,
    /// 최대 HP. 0 은 1 로 본다 — 태어나자마자 죽은 유닛은 만들지 않는다.
    pub max_hp: u32,
    pub attack: u32,
    pub defense: u32,
    /// 피해를 받지 않는다 (서버 `NpcEntityData::isImmortal`). 공격 대상이 되면 거절된다.
    pub immortal: bool,
    pub faction: FactionId,
    pub ai: AiKind,
    /// 공격형 AI 가 적을 알아채는 거리 (m).
    pub aggro_range: f32,
    /// 스폰 지점에서 이만큼(m) 벗어나면 추격을 포기하고 돌아간다. `aggro_range` 보다 작으면
    /// `aggro_range` 로 올린다 — 알아챈 자리에서 곧바로 포기하게 되므로.
    pub leash_range: f32,
    /// AI 가 쓰는 기본 공격. 없으면 AI 는 싸우지 않는다.
    pub basic_attack: Option<SkillId>,
    /// 죽을 때 굴릴 드롭 테이블.
    pub loot: Option<LootTableId>,
    /// 이 유닛을 죽인 쪽이 받는 경험치 (P3).
    pub exp_reward: u32,
    /// 레벨이 오를 때마다 더해지는 수치. 기본은 0 — 성장하지 않는다.
    pub growth: Growth,
}

impl Default for UnitDef {
    /// 움직이지 않고 싸우지 않는 최소 유닛. 실제 수치는 데이터에서 채운다.
    fn default() -> Self {
        Self {
            move_speed: 0.0,
            max_hp: 1,
            attack: 0,
            defense: 0,
            immortal: false,
            faction: FactionId::NONE,
            ai: AiKind::Passive,
            aggro_range: 0.0,
            leash_range: 0.0,
            basic_attack: None,
            loot: None,
            exp_reward: 0,
            growth: Growth::default(),
        }
    }
}

/// 음수·NaN·∞ 를 0 으로.
fn non_negative(v: f32) -> f32 {
    if v.is_finite() && v > 0.0 { v } else { 0.0 }
}

impl UnitDef {
    /// 수치를 보정한다 — 음수 속도는 뒤로 걷고, NaN 은 위치를 NaN 으로 오염시킨다.
    #[must_use]
    pub(crate) fn sanitized(self) -> Self {
        let aggro_range = non_negative(self.aggro_range);
        Self {
            move_speed: non_negative(self.move_speed),
            max_hp: self.max_hp.max(1),
            aggro_range,
            leash_range: non_negative(self.leash_range).max(aggro_range),
            ..self
        }
    }
}

/// AI 가 tick 사이에 기억하는 것.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct AiState {
    /// 지금 싸우는 상대.
    pub(crate) target: Option<Entity>,
    /// 마지막으로 경로를 잡은 추격 지점. 대상이 조금 움직일 때마다 A* 를 다시 돌지 않기 위해.
    pub(crate) chase_goal: Option<Vec2>,
    /// 추격을 포기하고 스폰 지점으로 돌아가는 중. 이 동안은 싸움을 받지 않는다.
    pub(crate) returning: bool,
    /// 이 시각 전에는 새 적을 찾지 않는다 — 갈 수 없는 적을 매 tick 다시 쫓지 않게.
    pub(crate) acquire_after: Duration,
}

/// 시뮬레이션 유닛 한 개의 상태.
///
/// 필드는 크레이트 밖에서 읽기만 한다 — 바꾸는 길은 [`Intent`](crate::Intent) 뿐이다.
#[derive(Clone, Debug)]
pub struct Unit {
    pub(crate) def: UnitDef,
    pub(crate) pos: Vec2,
    /// 직전 tick 의 위치. 렌더 보간용 (이음매 ②).
    pub(crate) prev_pos: Vec2,
    /// 라디안, `nexus_core::units` 규약.
    pub(crate) heading: f32,
    /// 남은 경유점. 맨 앞이 다음 목표이고 맨 뒤가 최종 목적지다.
    pub(crate) waypoints: VecDeque<Vec2>,
    /// 현재 HP. 0 이면 죽은 것이다.
    pub(crate) hp: u32,
    /// 스킬별 다시 쓸 수 있는 시각 (시뮬레이션 시계 기준).
    pub(crate) cooldowns: HashMap<SkillId, Duration>,
    /// 스폰 지점. AI 의 추격 한계(leash)와 귀환 기준.
    pub(crate) home: Vec2,
    pub(crate) ai: AiState,
    pub(crate) inventory: Inventory,
    /// 장착 장비의 수치 합. 장착·해제 때 다시 계산한다.
    pub(crate) bonus_attack: u32,
    pub(crate) bonus_defense: u32,
    /// 레벨·경험치 (P3). 몬스터도 갖지만 보통 1레벨 그대로다.
    pub(crate) progress: Progress,
}

impl Unit {
    pub(crate) fn new(pos: Vec2, heading: f32, def: UnitDef) -> Self {
        let def = def.sanitized();
        Self {
            def,
            pos,
            prev_pos: pos,
            heading,
            waypoints: VecDeque::new(),
            hp: def.max_hp,
            cooldowns: HashMap::new(),
            home: pos,
            ai: AiState::default(),
            inventory: Inventory::default(),
            bonus_attack: 0,
            bonus_defense: 0,
            progress: Progress::default(),
        }
    }

    #[must_use]
    pub fn def(&self) -> UnitDef {
        self.def
    }

    #[must_use]
    pub fn hp(&self) -> u32 {
        self.hp
    }

    /// 스폰 지점.
    #[must_use]
    pub fn home(&self) -> Vec2 {
        self.home
    }

    /// 레벨 성장 + 장비가 더해진 공격력 — 전투는 이 값을 쓴다.
    #[must_use]
    pub fn attack(&self) -> u32 {
        self.def
            .attack
            .saturating_add(self.grown(self.def.growth.attack))
            .saturating_add(self.bonus_attack)
    }

    /// 레벨 성장이 반영된 최대 HP. **`def().max_hp` 대신 이것을 쓴다.**
    #[must_use]
    pub fn max_hp(&self) -> u32 {
        self.def
            .max_hp
            .saturating_add(self.grown(self.def.growth.max_hp))
            .max(1)
    }

    /// 레벨·경험치.
    #[must_use]
    pub fn progress(&self) -> Progress {
        self.progress
    }

    /// 성장치 × (레벨 - 1).
    fn grown(&self, per_level: u32) -> u32 {
        per_level.saturating_mul(self.progress.level() - 1)
    }

    /// 레벨 성장 + 장비가 더해진 방어력.
    #[must_use]
    pub fn defense(&self) -> u32 {
        self.def
            .defense
            .saturating_add(self.grown(self.def.growth.defense))
            .saturating_add(self.bonus_defense)
    }

    #[must_use]
    pub fn inventory(&self) -> &Inventory {
        &self.inventory
    }

    /// AI 가 지금 싸우는 상대.
    #[must_use]
    pub fn target(&self) -> Option<Entity> {
        self.ai.target
    }

    /// AI 가 추격을 포기하고 돌아가는 중인가.
    #[must_use]
    pub fn is_returning(&self) -> bool {
        self.ai.returning
    }

    /// 살아 있는가. 죽은 유닛은 움직이지도 공격하지도 않지만 월드에는 남는다
    /// (시체 — 치울지는 스폰한 쪽이 정한다).
    #[must_use]
    pub fn is_alive(&self) -> bool {
        self.hp > 0
    }

    /// `skill` 을 `now` 에 쓸 수 있는가.
    #[must_use]
    pub fn is_ready(&self, skill: SkillId, now: Duration) -> bool {
        self.cooldowns.get(&skill).is_none_or(|&ready| now >= ready)
    }

    /// 현재 tick 의 위치 (m).
    #[must_use]
    pub fn pos(&self) -> Vec2 {
        self.pos
    }

    /// 직전 tick 의 위치 (m).
    #[must_use]
    pub fn prev_pos(&self) -> Vec2 {
        self.prev_pos
    }

    /// 두 tick 사이를 `alpha`(`[0, 1]`) 로 보간한 위치 — 렌더는 이것을 그린다.
    #[must_use]
    pub fn render_pos(&self, alpha: f32) -> Vec2 {
        self.prev_pos.lerp(self.pos, alpha.clamp(0.0, 1.0))
    }

    /// 바라보는 방향 (라디안).
    #[must_use]
    pub fn heading(&self) -> f32 {
        self.heading
    }

    /// 이동 중인가.
    #[must_use]
    pub fn is_moving(&self) -> bool {
        !self.waypoints.is_empty()
    }

    /// 남은 경유점 — 경로 미리보기용.
    pub fn waypoints(&self) -> impl Iterator<Item = Vec2> + '_ {
        self.waypoints.iter().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_speed_is_clamped_to_zero() {
        for bad in [-1.0, f32::NAN, f32::INFINITY] {
            let def = UnitDef {
                move_speed: bad,
                ..UnitDef::default()
            }
            .sanitized();
            assert_eq!(def.move_speed, 0.0, "{bad}");
        }
        assert_eq!(
            UnitDef {
                move_speed: 2.5,
                ..UnitDef::default()
            }
            .sanitized()
            .move_speed,
            2.5
        );
    }

    #[test]
    fn render_pos_interpolates_between_ticks() {
        let mut unit = Unit::new(
            Vec2::ZERO,
            0.0,
            UnitDef {
                move_speed: 1.0,
                ..UnitDef::default()
            },
        );
        unit.pos = Vec2::new(2.0, 0.0);
        assert_eq!(unit.render_pos(0.0), Vec2::ZERO);
        assert_eq!(unit.render_pos(0.5), Vec2::new(1.0, 0.0));
        assert_eq!(unit.render_pos(1.0), Vec2::new(2.0, 0.0));
        assert_eq!(
            unit.render_pos(7.0),
            Vec2::new(2.0, 0.0),
            "범위 밖 alpha 는 자른다"
        );
    }
}
