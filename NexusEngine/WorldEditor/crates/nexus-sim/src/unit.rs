//! 유닛 — 지면 위를 움직이는 시뮬레이션 대상 (플레이어·NPC·몬스터 공통).
//!
//! 수치([`UnitDef`])는 코드가 아니라 **데이터**다. 스폰하는 쪽이 넘겨준다.

use std::collections::VecDeque;

use nexus_core::Vec2;

/// 유닛 종류별 정적 수치. 스폰 시 복사되어 유닛마다 따로 갖는다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnitDef {
    /// 이동 속도 (m/s). 음수·NaN 은 0 으로 본다.
    pub move_speed: f32,
}

impl UnitDef {
    /// 수치를 보정한다 — 음수 속도는 뒤로 걷고, NaN 은 위치를 NaN 으로 오염시킨다.
    #[must_use]
    pub(crate) fn sanitized(self) -> Self {
        let move_speed = if self.move_speed.is_finite() && self.move_speed > 0.0 {
            self.move_speed
        } else {
            0.0
        };
        Self { move_speed }
    }
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
}

impl Unit {
    pub(crate) fn new(pos: Vec2, heading: f32, def: UnitDef) -> Self {
        Self {
            def: def.sanitized(),
            pos,
            prev_pos: pos,
            heading,
            waypoints: VecDeque::new(),
        }
    }

    #[must_use]
    pub fn def(&self) -> UnitDef {
        self.def
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
            let def = UnitDef { move_speed: bad }.sanitized();
            assert_eq!(def.move_speed, 0.0, "{bad}");
        }
        assert_eq!(UnitDef { move_speed: 2.5 }.sanitized().move_speed, 2.5);
    }

    #[test]
    fn render_pos_interpolates_between_ticks() {
        let mut unit = Unit::new(Vec2::ZERO, 0.0, UnitDef { move_speed: 1.0 });
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
