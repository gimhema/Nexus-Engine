//! 월드 시계 — 하루 시각과 낮·밤.
//!
//! 서버의 `GameLogic`(월드 시간 + `DayBegin`·`NightBegin` 이벤트)과 같은 자리다. 시각은 **규칙**이라
//! 권한자가 가진다 — 단계 2 에서는 서버가 시각을 알려 주고, 화면(주변광·플레이어 주변 빛)은 그 값을 그릴 뿐이다.
//!
//! - 시각은 하루 안의 **정수 밀리초**로 센다 (`f32` 를 누적하지 않는다 — 결정적이고 오래 돌려도 틀어지지 않는다).
//! - 하루 길이(`day_ms`)는 시뮬레이션 시간이다. 서버 기본값과 같은 **실제 1분 = 게임 1시간**(하루 24분)이 기본.
//! - 밤은 `dusk_hour` 부터 다음 날 `dawn_hour` 전까지. 넘어가는 tick 에 [`crate::Event::NightBegan`] /
//!   [`crate::Event::DayBegan`] 이 난다.
//! - 시계가 없는 존(`None`, 집 안·던전)에서는 시각이 흐르지 않는다 — 시각을 이어 갈지는 앱이 정한다(저장 파일).

use std::time::Duration;

/// 하루 길이와 낮·밤 경계.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DayCycle {
    /// 하루(24시간)의 길이 — 시뮬레이션 밀리초.
    pub day_ms: u64,
    /// 해 뜨는 시각 (0~24).
    pub dawn_hour: f32,
    /// 해 지는 시각 (0~24).
    pub dusk_hour: f32,
}

impl Default for DayCycle {
    fn default() -> Self {
        Self {
            day_ms: 24 * 60 * 1000,
            dawn_hour: 6.0,
            dusk_hour: 19.0,
        }
    }
}

impl DayCycle {
    /// 잘못된 값을 쓸 수 있는 값으로 — 하루는 1초 이상, 시각은 `[0, 24)`.
    #[must_use]
    pub(crate) fn sanitized(self) -> Self {
        let hour = |h: f32, d: f32| {
            if h.is_finite() { h.rem_euclid(24.0) } else { d }
        };
        let d = Self::default();
        Self {
            day_ms: self.day_ms.max(1000),
            dawn_hour: hour(self.dawn_hour, d.dawn_hour),
            dusk_hour: hour(self.dusk_hour, d.dusk_hour),
        }
    }

    /// 이 시각이 밤인가.
    #[must_use]
    pub fn is_night_at(&self, hour: f32) -> bool {
        if self.dawn_hour <= self.dusk_hour {
            hour < self.dawn_hour || hour >= self.dusk_hour
        } else {
            // 해가 자정을 넘겨 지는 이상한 설정 — 낮이 자정을 걸친다.
            hour >= self.dusk_hour && hour < self.dawn_hour
        }
    }

    fn hour_of(&self, ms: u64) -> f32 {
        (ms as f64 * 24.0 / self.day_ms as f64) as f32
    }

    fn ms_of(&self, hour: f32) -> u64 {
        let h = if hour.is_finite() {
            f64::from(hour).rem_euclid(24.0)
        } else {
            0.0
        };
        ((h / 24.0 * self.day_ms as f64) as u64).min(self.day_ms - 1)
    }
}

/// 흐르는 시계 — 하루 정의 + 하루 안의 밀리초.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Clock {
    pub(crate) cycle: DayCycle,
    ms: u64,
}

/// 한 tick 동안 낮·밤이 바뀌었는가.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Turn {
    None,
    Day,
    Night,
}

impl Clock {
    pub(crate) fn new(cycle: DayCycle, hour: f32) -> Self {
        let cycle = cycle.sanitized();
        Self {
            ms: cycle.ms_of(hour),
            cycle,
        }
    }

    /// 지금 시각 (0~24).
    pub(crate) fn hour(&self) -> f32 {
        self.cycle.hour_of(self.ms)
    }

    pub(crate) fn is_night(&self) -> bool {
        self.cycle.is_night_at(self.hour())
    }

    pub(crate) fn set_hour(&mut self, hour: f32) {
        self.ms = self.cycle.ms_of(hour);
    }

    /// `dt` 만큼 흐른다. 한 tick 에 낮·밤이 여러 번 바뀔 만큼 길면 **마지막 상태**만 알린다.
    pub(crate) fn advance(&mut self, dt: Duration) -> Turn {
        let before = self.is_night();
        let step = u64::try_from(dt.as_millis()).unwrap_or(u64::MAX);
        self.ms = (self.ms + step % self.cycle.day_ms) % self.cycle.day_ms;
        match (before, self.is_night()) {
            (false, true) => Turn::Night,
            (true, false) => Turn::Day,
            _ => Turn::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cycle() -> DayCycle {
        DayCycle {
            day_ms: 24_000, // 1초 = 1시간
            dawn_hour: 6.0,
            dusk_hour: 19.0,
        }
    }

    #[test]
    fn the_clock_runs_and_wraps_at_midnight() {
        let mut c = Clock::new(cycle(), 23.0);
        assert!((c.hour() - 23.0).abs() < 1e-3);
        c.advance(Duration::from_millis(1500));
        assert!((c.hour() - 0.5).abs() < 1e-3, "{}", c.hour());
    }

    #[test]
    fn day_and_night_turn_once_at_the_boundaries() {
        let mut c = Clock::new(cycle(), 18.0);
        let mut turns = Vec::new();
        for _ in 0..(24 * 20) {
            match c.advance(Duration::from_millis(50)) {
                Turn::None => {}
                t => turns.push((t, (c.hour() * 10.0).round() / 10.0)),
            }
        }
        // 하루를 한 바퀴 — 19시에 밤, 6시에 낮이 한 번씩.
        assert_eq!(turns, [(Turn::Night, 19.0), (Turn::Day, 6.0)]);
    }

    #[test]
    fn night_spans_midnight() {
        let c = cycle();
        assert!(c.is_night_at(23.0) && c.is_night_at(2.0) && c.is_night_at(19.0));
        assert!(!c.is_night_at(6.0) && !c.is_night_at(12.0) && !c.is_night_at(18.9));
    }

    #[test]
    fn the_world_announces_nightfall_and_dawn() {
        use crate::{Authority, Event, LocalAuthority, SimWorld, Tile, TileMap};
        use nexus_core::Vec2;
        let mut w = SimWorld::new(TileMap::new(4, 4, 1.0, Vec2::ZERO, Tile::default()));
        assert_eq!(w.time_of_day(), None, "시계가 없는 존");
        assert!(!w.is_night());
        w.set_day_cycle(Some(cycle()), 18.9);
        let mut auth = LocalAuthority::new(w);
        let dt = Duration::from_millis(50);
        let events: Vec<Event> = (0..4).flat_map(|_| auth.tick(dt)).collect();
        assert_eq!(events, [Event::NightBegan]);
        assert!(auth.world().is_night());
        auth.world_mut().set_time_of_day(5.95);
        let events: Vec<Event> = (0..4).flat_map(|_| auth.tick(dt)).collect();
        assert_eq!(events, [Event::DayBegan]);
        assert!(!auth.world().is_night());
    }

    #[test]
    fn bad_values_are_tamed() {
        let c = DayCycle {
            day_ms: 0,
            dawn_hour: f32::NAN,
            dusk_hour: 43.0,
        }
        .sanitized();
        assert_eq!(c.day_ms, 1000);
        assert_eq!(c.dawn_hour, 6.0);
        assert_eq!(c.dusk_hour, 19.0);
        let mut k = Clock::new(cycle(), -1.0);
        assert!((k.hour() - 23.0).abs() < 1e-3, "음수 시각은 전날로");
        k.set_hour(24.0);
        assert!(k.hour() < 1e-3);
    }
}
