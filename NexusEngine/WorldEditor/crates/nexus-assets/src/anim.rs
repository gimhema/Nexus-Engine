//! 스프라이트 시트 메타데이터와 애니메이션 재생.
//!
//! **GPU 도 시간 소스도 모른다.** 프레임 진행은 호출자가 넘기는 `dt` 로만 일어나므로,
//! 전부 단위 테스트가 가능하다.
//!
//! # 시트 배치
//!
//! 행 = `클립 시작 행 + 방향 인덱스`, 열 = 프레임. 방향이 4개인 걸으면
//! 행 4개를 연달아 차지한다.
//!
//! ```text
//!        열0    열1    열2    열3
//! 행0  [Idle 동 ......................]
//! 행1  [Idle 북 ......................]
//! 행2  [Idle 서 ......................]
//! 행3  [Idle 남 ......................]
//! 행4  [Walk 동 ......................]
//!  ...
//! ```
//!
//! 방향 인덱스는 [`nexus_core::units::direction_index`] 가 만든다 — `0` 이 동(+X),
//! 반시계로 증가. **시트 행 순서가 이 규약과 맞아야 한다.**
//!
//! # 방향 수는 시트마다 다르다
//!
//! [`SpriteSheet::directions`] 는 데이터다. 4방향 시트를 8방향으로 바꿔도
//! 엔진 코드는 그대로다 — **호출부에서 방향 수를 상수로 박지 말 것.**

use core::time::Duration;

use nexus_render::UvRect;

use crate::GridAtlas;

/// 스프라이트 동작 상태. 시트가 상태마다 클립 하나를 대응시킨다.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AnimState {
    Idle,
    Walk,
    Attack,
    Cast,
    Hit,
    Die,
}

/// 동작 하나의 재생 정보.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clip {
    /// 시트에서 이 클립의 **방향 0번** 행. 방향 수만큼 아래로 이어진다.
    pub row: u32,
    /// 프레임 수 (열 개수). 0 이면 1 로 취급한다.
    pub frames: u32,
    /// 프레임 하나의 지속 시간.
    pub frame_time: Duration,
    /// 끝나면 처음으로 돌아가는가.
    ///
    /// `false` 면 마지막 프레임에서 멈춘다 — 죽음·피격처럼 한 번만 재생하는 동작.
    pub looping: bool,
}

impl Clip {
    /// 프레임 수. 0 을 1 로 보정한다 — 나눗셈에서 터지지 않게.
    #[must_use]
    pub fn frame_count(&self) -> u32 {
        self.frames.max(1)
    }
}

/// 애니메이션 시트 — 격자 아틀라스 + 방향 수 + 클립 목록.
///
/// 지금은 앱 코드가 직접 만든다. 시트 정의 파일(존 애셋과 함께 읽는)은 S5/S7 에서 들어온다.
#[derive(Clone, Debug)]
pub struct SpriteSheet {
    atlas: GridAtlas,
    directions: u32,
    /// 상태 → 클립. 항목이 몇 개뿐이라 맵보다 선형 탐색이 싸다.
    clips: Vec<(AnimState, Clip)>,
}

impl SpriteSheet {
    /// `directions` 는 1 이상이어야 한다 — 0 은 1 로 보정된다.
    #[must_use]
    pub fn new(atlas: GridAtlas, directions: u32, clips: Vec<(AnimState, Clip)>) -> Self {
        Self {
            atlas,
            directions: directions.max(1),
            clips,
        }
    }

    /// 방향 수. **상수로 박지 말고 항상 여기서 읽을 것.**
    #[must_use]
    pub fn directions(&self) -> u32 {
        self.directions
    }

    #[must_use]
    pub fn atlas(&self) -> &GridAtlas {
        &self.atlas
    }

    /// 상태에 대응하는 클립. 없으면 `None`.
    #[must_use]
    pub fn clip(&self, state: AnimState) -> Option<&Clip> {
        self.clips.iter().find(|(s, _)| *s == state).map(|(_, c)| c)
    }

    /// 상태의 클립, 없으면 [`AnimState::Idle`], 그것도 없으면 시트의 첫 클립.
    ///
    /// 아직 아트가 없는 동작을 요청해도 화면이 비지 않도록 하는 것이다.
    #[must_use]
    pub fn clip_or_fallback(&self, state: AnimState) -> Option<&Clip> {
        self.clip(state)
            .or_else(|| self.clip(AnimState::Idle))
            .or_else(|| self.clips.first().map(|(_, c)| c))
    }

    /// 한 칸의 UV. 범위를 벗어나면 아틀라스가 가장자리로 잘라 준다.
    #[must_use]
    pub fn uv(&self, state: AnimState, direction: u32, frame: u32) -> UvRect {
        let Some(clip) = self.clip_or_fallback(state) else {
            return UvRect::FULL;
        };
        let direction = direction.min(self.directions - 1);
        self.atlas
            .uv(frame % clip.frame_count(), clip.row + direction)
    }
}

/// 재생 상태. 엔티티 하나가 하나씩 들고 있는다.
///
/// **진행은 고정 timestep(`App::fixed_update`)에서만** 한다. 렌더 프레임에서 진행하면
/// 프레임레이트에 따라 애니메이션 속도가 달라진다.
#[derive(Clone, Copy, Debug)]
pub struct SpriteAnimator {
    state: AnimState,
    frame: u32,
    /// 현재 프레임에서 지난 시간.
    elapsed: Duration,
    /// 비루프 클립이 마지막 프레임에 도달했는가.
    finished: bool,
}

impl Default for SpriteAnimator {
    fn default() -> Self {
        Self {
            state: AnimState::Idle,
            frame: 0,
            elapsed: Duration::ZERO,
            finished: false,
        }
    }
}

impl SpriteAnimator {
    #[must_use]
    pub fn state(&self) -> AnimState {
        self.state
    }

    #[must_use]
    pub fn frame(&self) -> u32 {
        self.frame
    }

    /// 비루프 클립이 끝까지 갔는가. 루프 클립은 항상 `false`.
    #[must_use]
    pub fn finished(&self) -> bool {
        self.finished
    }

    /// 상태를 바꾼다.
    ///
    /// **같은 상태로 다시 호출해도 프레임이 리셋되지 않는다.** 매 tick `set_state(Walk)` 를
    /// 부르는 코드가 흔한데, 리셋하면 걷기가 첫 프레임에서 얼어붙는다.
    pub fn set_state(&mut self, state: AnimState) {
        if self.state == state {
            return;
        }
        self.state = state;
        self.frame = 0;
        self.elapsed = Duration::ZERO;
        self.finished = false;
    }

    /// 고정 timestep 만큼 진행한다.
    ///
    /// 큰 `dt` 가 들어와도 **루프를 돌지 않고** 나눗셈으로 건너뛴다 — 창 최소화 복귀처럼
    /// 몇 초가 한 번에 밀려올 수 있다.
    pub fn advance(&mut self, sheet: &SpriteSheet, dt: Duration) {
        let Some(clip) = sheet.clip_or_fallback(self.state).copied() else {
            return;
        };
        let count = clip.frame_count();
        let step = clip.frame_time;

        if step.is_zero() || count == 1 {
            // 한 장짜리이거나 시간이 0 이면 진행할 것이 없다.
            self.finished = !clip.looping;
            return;
        }
        if self.finished {
            return;
        }

        self.elapsed += dt;
        let advanced = u64::try_from(self.elapsed.as_nanos() / step.as_nanos()).unwrap_or(u64::MAX);
        if advanced == 0 {
            return;
        }
        self.elapsed -= step.saturating_mul(u32::try_from(advanced).unwrap_or(u32::MAX));

        let next = u64::from(self.frame) + advanced;
        if clip.looping {
            self.frame = u32::try_from(next % u64::from(count)).unwrap_or(0);
        } else if next >= u64::from(count) {
            self.frame = count - 1;
            self.elapsed = Duration::ZERO;
            self.finished = true;
        } else {
            self.frame = u32::try_from(next).unwrap_or(count - 1);
        }
    }

    /// 현재 상태·프레임의 UV.
    #[must_use]
    pub fn uv(&self, sheet: &SpriteSheet, direction: u32) -> UvRect {
        sheet.uv(self.state, direction, self.frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Image;

    fn sheet() -> SpriteSheet {
        // 4열 × 8행. Idle(4방향) 행 0~3, Walk(4방향) 행 4~7.
        let image = Image::blank_for_test(4 * 32, 8 * 48);
        let atlas = GridAtlas::new(&image, 32, 48).unwrap();
        SpriteSheet::new(
            atlas,
            4,
            vec![
                (
                    AnimState::Idle,
                    Clip {
                        row: 0,
                        frames: 2,
                        frame_time: Duration::from_millis(100),
                        looping: true,
                    },
                ),
                (
                    AnimState::Walk,
                    Clip {
                        row: 4,
                        frames: 4,
                        frame_time: Duration::from_millis(100),
                        looping: true,
                    },
                ),
                (
                    AnimState::Die,
                    Clip {
                        row: 4,
                        frames: 3,
                        frame_time: Duration::from_millis(100),
                        looping: false,
                    },
                ),
            ],
        )
    }

    fn tick(a: &mut SpriteAnimator, s: &SpriteSheet, ms: u64) {
        a.advance(s, Duration::from_millis(ms));
    }

    #[test]
    fn frames_advance_on_the_clip_timing() {
        let s = sheet();
        let mut a = SpriteAnimator::default();
        a.set_state(AnimState::Walk);

        tick(&mut a, &s, 50);
        assert_eq!(a.frame(), 0, "아직 한 프레임이 안 찼다");
        tick(&mut a, &s, 50);
        assert_eq!(a.frame(), 1);
        tick(&mut a, &s, 100);
        assert_eq!(a.frame(), 2);
    }

    #[test]
    fn looping_wraps_around() {
        let s = sheet();
        let mut a = SpriteAnimator::default();
        a.set_state(AnimState::Walk);
        for _ in 0..4 {
            tick(&mut a, &s, 100);
        }
        assert_eq!(a.frame(), 0, "4프레임 루프가 처음으로 돌아와야 한다");
        assert!(!a.finished(), "루프 클립은 끝나지 않는다");
    }

    #[test]
    fn non_looping_clamps_at_the_last_frame() {
        let s = sheet();
        let mut a = SpriteAnimator::default();
        a.set_state(AnimState::Die);

        for _ in 0..10 {
            tick(&mut a, &s, 100);
        }
        assert_eq!(a.frame(), 2, "마지막 프레임에서 멈춰야 한다");
        assert!(a.finished());
    }

    #[test]
    fn huge_dt_does_not_spin_and_lands_correctly() {
        // 창 최소화 복귀 등으로 몇 초가 한 번에 밀려올 수 있다.
        let s = sheet();
        let mut a = SpriteAnimator::default();
        a.set_state(AnimState::Walk);

        tick(&mut a, &s, 10_000); // 100프레임 = 4프레임 루프 25바퀴
        assert_eq!(a.frame(), 0);

        let mut b = SpriteAnimator::default();
        b.set_state(AnimState::Walk);
        tick(&mut b, &s, 10_250);
        assert_eq!(b.frame(), 2, "102 % 4 = 2");
    }

    #[test]
    fn repeating_the_same_state_does_not_reset() {
        // 매 tick set_state(Walk) 를 부르는 코드가 흔하다. 리셋하면 걷기가 얼어붙는다.
        let s = sheet();
        let mut a = SpriteAnimator::default();
        a.set_state(AnimState::Walk);
        tick(&mut a, &s, 100);
        a.set_state(AnimState::Walk);
        assert_eq!(a.frame(), 1);
    }

    #[test]
    fn changing_state_restarts_from_the_first_frame() {
        let s = sheet();
        let mut a = SpriteAnimator::default();
        a.set_state(AnimState::Walk);
        tick(&mut a, &s, 250);
        assert_ne!(a.frame(), 0);

        a.set_state(AnimState::Idle);
        assert_eq!(a.frame(), 0);
        assert_eq!(a.state(), AnimState::Idle);
        assert!(!a.finished(), "상태를 바꾸면 finished 도 풀려야 한다");
    }

    #[test]
    fn missing_clip_falls_back_instead_of_vanishing() {
        let s = sheet();
        // Attack 은 시트에 없다 → Idle 로 대체된다
        assert!(s.clip(AnimState::Attack).is_none());
        let fallback = s.clip_or_fallback(AnimState::Attack).copied().unwrap();
        assert_eq!(fallback, *s.clip(AnimState::Idle).unwrap());

        let mut a = SpriteAnimator::default();
        a.set_state(AnimState::Attack);
        tick(&mut a, &s, 100);
        // Idle 클립(2프레임)을 따라간다
        assert_eq!(a.frame(), 1);
    }

    #[test]
    fn direction_picks_a_different_row() {
        let s = sheet();
        let east = s.uv(AnimState::Idle, 0, 0);
        let north = s.uv(AnimState::Idle, 1, 0);
        assert_ne!(east, north, "방향이 달라도 같은 칸을 가리킨다");
        // 행이 하나 내려간 만큼만 차이나야 한다
        assert!((north.min.y - east.min.y - 1.0 / 8.0).abs() < 1e-6);
        assert!((north.min.x - east.min.x).abs() < 1e-6);
    }

    #[test]
    fn frame_picks_a_different_column() {
        let s = sheet();
        let f0 = s.uv(AnimState::Walk, 0, 0);
        let f1 = s.uv(AnimState::Walk, 0, 1);
        assert!((f1.min.x - f0.min.x - 1.0 / 4.0).abs() < 1e-6);
        assert!((f1.min.y - f0.min.y).abs() < 1e-6);
    }

    #[test]
    fn out_of_range_direction_and_frame_clamp() {
        let s = sheet();
        assert_eq!(s.uv(AnimState::Idle, 99, 0), s.uv(AnimState::Idle, 3, 0));
        // 프레임은 클립 길이로 나머지 연산 — 2프레임 Idle 이면 5 → 1
        assert_eq!(s.uv(AnimState::Idle, 0, 5), s.uv(AnimState::Idle, 0, 1));
    }

    #[test]
    fn zero_directions_is_treated_as_one() {
        let image = Image::blank_for_test(32, 48);
        let atlas = GridAtlas::new(&image, 32, 48).unwrap();
        let s = SpriteSheet::new(atlas, 0, Vec::new());
        assert_eq!(s.directions(), 1);
        // 클립이 하나도 없어도 터지지 않는다
        assert_eq!(s.uv(AnimState::Idle, 0, 0), UvRect::FULL);
    }

    #[test]
    fn zero_frame_time_does_not_divide_by_zero() {
        let image = Image::blank_for_test(4 * 32, 48);
        let atlas = GridAtlas::new(&image, 32, 48).unwrap();
        let s = SpriteSheet::new(
            atlas,
            1,
            vec![(
                AnimState::Idle,
                Clip {
                    row: 0,
                    frames: 4,
                    frame_time: Duration::ZERO,
                    looping: true,
                },
            )],
        );
        let mut a = SpriteAnimator::default();
        tick(&mut a, &s, 1000);
        assert_eq!(a.frame(), 0);
    }
}
