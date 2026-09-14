//! 고정 timestep 시뮬레이션 타이밍.
//!
//! 시뮬레이션을 렌더 프레임레이트에서 분리한다. 이는 나중에 서버를 붙일 때를 위한
//! 준비이기도 하다 — NexusEngine 의 `ZoneActor` 는 50ms(20Hz) 고정 tick 으로 돌고,
//! 클라이언트 시뮬레이션이 프레임레이트에 묶여 있으면 둘을 맞물리게 할 수 없다.
//!
//! 사용 패턴:
//!
//! ```
//! use nexus_core::time::FixedTimestep;
//! use std::time::Duration;
//!
//! let mut ts = FixedTimestep::new(20);
//! let steps = ts.accumulate(Duration::from_millis(120));
//! for _ in 0..steps {
//!     // 시뮬레이션 1 스텝 — dt 는 항상 ts.step() 으로 일정하다
//! }
//! let _alpha = ts.alpha(); // 렌더 보간 계수 [0, 1)
//! ```

use core::time::Duration;

/// 고정 간격 시뮬레이션 누산기.
///
/// 프레임 델타를 누적했다가 고정 크기 스텝으로 나누어 소비한다.
/// 남은 잔여분은 [`alpha`](Self::alpha) 로 노출되어 렌더 보간에 쓰인다.
#[derive(Debug, Clone)]
pub struct FixedTimestep {
    step: Duration,
    accumulator: Duration,
    max_steps_per_frame: u32,
}

impl FixedTimestep {
    /// 기본 시뮬레이션 주기 (Hz). NexusEngine `ZoneActor` tick 과 동일하다.
    pub const DEFAULT_HZ: u32 = 20;

    /// 한 프레임에 실행을 허용하는 최대 스텝 수.
    ///
    /// 이 한도가 없으면 한 프레임이 밀렸을 때 따라잡기 위해 더 많은 스텝을 돌리고,
    /// 그 때문에 다음 프레임이 또 밀리는 "죽음의 나선"에 빠진다.
    pub const DEFAULT_MAX_STEPS: u32 = 5;

    /// 주어진 주기(Hz)로 생성한다. `hz` 가 0 이면 1 로 취급한다.
    #[must_use]
    pub fn new(hz: u32) -> Self {
        let hz = hz.max(1);
        Self {
            step: Duration::from_secs(1) / hz,
            accumulator: Duration::ZERO,
            max_steps_per_frame: Self::DEFAULT_MAX_STEPS,
        }
    }

    /// 한 스텝의 고정 길이. 시뮬레이션에 넘길 `dt` 는 항상 이 값이다.
    #[must_use]
    pub fn step(&self) -> Duration {
        self.step
    }

    /// 프레임 델타를 누적하고, 이번 프레임에 실행해야 할 스텝 수를 반환한다.
    ///
    /// 반환값은 [`DEFAULT_MAX_STEPS`](Self::DEFAULT_MAX_STEPS) 로 제한된다.
    /// 한도를 넘겨 밀린 시간은 버려지며(프레임 스킵), 잔여 소수분만 보존해
    /// [`alpha`](Self::alpha) 의 연속성을 유지한다.
    pub fn accumulate(&mut self, frame_dt: Duration) -> u32 {
        self.accumulator += frame_dt;

        let mut steps = 0;
        while self.accumulator >= self.step && steps < self.max_steps_per_frame {
            self.accumulator -= self.step;
            steps += 1;
        }

        // 한도에 걸려 아직도 밀려 있다면 따라잡기를 포기하고 잔여분만 남긴다.
        //
        // 나노초 정수 연산을 쓴다. f64 모듈로는 0.05 같은 값이 이진수로 정확하지
        // 않아 잔여분이 step 과 같아지는 경우가 생기고, 그러면 `accumulator < step`
        // 불변식이 깨져 alpha() 가 1.0 이 된다.
        if self.accumulator >= self.step {
            let remainder = self.accumulator.as_nanos() % self.step.as_nanos();
            self.accumulator = Duration::from_nanos(u64::try_from(remainder).unwrap_or(0));
        }

        debug_assert!(
            self.accumulator < self.step,
            "누산기는 항상 한 스텝 미만이어야 한다"
        );

        steps
    }

    /// 마지막 시뮬레이션 상태와 다음 상태 사이의 보간 계수. 범위는 `[0, 1)`.
    ///
    /// 렌더는 이 값으로 두 상태를 섞어 그린다. 이렇게 해야 시뮬레이션 주기(20Hz)보다
    /// 높은 프레임레이트에서도 움직임이 매끄럽다.
    #[must_use]
    pub fn alpha(&self) -> f32 {
        let a = self.accumulator.as_secs_f32() / self.step.as_secs_f32();
        a.clamp(0.0, 1.0)
    }

    /// 누산기를 비운다. 창 최소화 복귀 등 큰 시간 점프 직후에 호출한다.
    pub fn reset(&mut self) {
        self.accumulator = Duration::ZERO;
    }
}

impl Default for FixedTimestep {
    fn default() -> Self {
        Self::new(Self::DEFAULT_HZ)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_20hz_50ms() {
        let ts = FixedTimestep::default();
        assert_eq!(ts.step(), Duration::from_millis(50));
    }

    #[test]
    fn zero_hz_does_not_panic() {
        let ts = FixedTimestep::new(0);
        assert_eq!(ts.step(), Duration::from_secs(1));
    }

    #[test]
    fn exact_step_yields_one_step_and_zero_alpha() {
        let mut ts = FixedTimestep::new(20);
        assert_eq!(ts.accumulate(Duration::from_millis(50)), 1);
        assert!(ts.alpha().abs() < 1e-6, "alpha = {}", ts.alpha());
    }

    #[test]
    fn short_frames_accumulate_until_a_step_fires() {
        // 60Hz 렌더(16.67ms) → 20Hz 시뮬: 세 프레임마다 한 스텝
        let mut ts = FixedTimestep::new(20);
        let frame = Duration::from_micros(16_667);

        assert_eq!(ts.accumulate(frame), 0);
        assert_eq!(ts.accumulate(frame), 0);
        assert_eq!(ts.accumulate(frame), 1);
    }

    #[test]
    fn sixty_frames_at_60hz_produce_about_twenty_steps() {
        let mut ts = FixedTimestep::new(20);
        let frame = Duration::from_micros(16_667);

        let total: u32 = (0..60).map(|_| ts.accumulate(frame)).sum();
        assert_eq!(total, 20, "1초 분량 렌더 프레임은 20 스텝을 만들어야 한다");
    }

    #[test]
    fn alpha_stays_in_unit_range() {
        let mut ts = FixedTimestep::new(20);
        for ms in [0, 7, 13, 49, 51, 120, 999] {
            ts.accumulate(Duration::from_millis(ms));
            let a = ts.alpha();
            assert!((0.0..1.0).contains(&a), "alpha 범위 이탈: {a}");
        }
    }

    #[test]
    fn long_stall_is_clamped_to_max_steps() {
        // 10초가 밀려 들어와도 200 스텝을 돌리지 않는다 (죽음의 나선 방지)
        let mut ts = FixedTimestep::new(20);
        let steps = ts.accumulate(Duration::from_secs(10));

        assert_eq!(steps, FixedTimestep::DEFAULT_MAX_STEPS);
        assert!(
            ts.accumulator < ts.step,
            "한도 초과분은 버려져야 한다 — 남은 값 {:?}",
            ts.accumulator
        );
    }

    #[test]
    fn stall_preserves_sub_step_remainder() {
        // 10.030초 = 200 스텝 + 30ms → 잔여 30ms 가 보존되어야 한다
        let mut ts = FixedTimestep::new(20);
        ts.accumulate(Duration::from_millis(10_030));

        let remainder = ts.accumulator.as_millis();
        assert!(
            (29..=31).contains(&remainder),
            "잔여분이 보존되지 않음: {remainder}ms"
        );
    }

    #[test]
    fn reset_clears_accumulator() {
        let mut ts = FixedTimestep::new(20);
        ts.accumulate(Duration::from_millis(30));
        ts.reset();
        assert!(ts.alpha().abs() < 1e-6);
    }
}
