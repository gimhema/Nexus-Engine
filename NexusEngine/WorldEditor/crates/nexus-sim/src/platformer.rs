//! 횡스크롤 물리 — 중력·점프·발판·사다리.
//!
//! # 같은 타일맵을 단면으로 읽는다
//!
//! 횡스크롤 존은 지면 평면(XY)을 **옆에서 자른 단면**으로 쓴다: **X = 가로, Y = 높이(위가 +)**.
//! 타일맵·존 파일·에디터 칠하기는 그대로이고, 타일의 뜻만 이렇게 읽는다:
//!
//! | 타일 | 횡스크롤에서 |
//! |---|---|
//! | `walkable: false` | **벽·땅** — 통과할 수 없는 블록 |
//! | `walkable` + `level ≥ 1` | **발판** — 위에서만 밟힌다(아래에서 뛰어오르면 뚫고 올라간다). ↓ 로 내려선다 |
//! | `walkable` + `ramp` | **사다리** — ↑/↓ 로 오르내린다. 맨 윗칸은 발판처럼 밟힌다 |
//! | 그 밖 | 빈 공간 |
//!
//! 맵 밖은 벽이다 — 존 경계가 곧 바닥·벽·천장이다. 쿼터뷰 존에서는 이 모듈이 아무 일도 하지 않는다
//! ([`SimWorld::set_platformer`](crate::SimWorld::set_platformer) 가 `None`).
//!
//! # 누가 어떻게 움직이나
//!
//! - **걷기**는 `Intent::Walk`(키보드 — 방향을 바꿀 때만 낸다) 또는 `Intent::MoveTo`(클릭·AI — 목표의
//!   **X 로만** 걸어간다, 길찾기 없음). 높이는 중력이 정한다.
//! - **점프**는 `Intent::Jump` — 땅·발판 위나 사다리에서만. **오르내리기**는 `Intent::Climb`.
//! - **직업이 없는 유닛(몬스터·NPC)은 낭떠러지에서 멈춘다** — 자기 발판을 벗어나지 않는다(`Blocked`).
//!   플레이어는 떨어진다.
//!
//! 수치(중력·점프 속도·몸 크기)는 [`Platformer`] 이고 앱이 `rules.ron` 에서 읽어 넘긴다.
//! 판정은 축마다 따로(가로 → 세로) 하고, 한 tick 안에서도 칸보다 짧게 끊어 움직여 얇은 벽을 뚫지 않는다.

use nexus_core::Vec2;

use crate::tilemap::{TileCoord, TileMap};

/// 횡스크롤 수치. 길이는 m, 속도는 m/s.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Platformer {
    /// 아래로 당기는 가속도 (m/s²).
    pub gravity: f32,
    /// 점프 순간의 위 속도. 오르는 높이는 `jump_speed² / (2 × gravity)`.
    pub jump_speed: f32,
    /// 떨어지는 속도의 상한.
    pub max_fall_speed: f32,
    /// 사다리를 오르내리는 속도.
    pub climb_speed: f32,
    /// 몸 상자 `(폭, 키)` — 발밑 가운데가 유닛 위치다.
    pub body: Vec2,
}

impl Default for Platformer {
    fn default() -> Self {
        Self {
            gravity: 30.0,
            jump_speed: 10.0,
            max_fall_speed: 20.0,
            climb_speed: 3.0,
            body: Vec2::new(0.6, 1.2),
        }
    }
}

impl Platformer {
    /// 잘못된 값(0 이하·NaN)을 기본값으로.
    #[must_use]
    pub(crate) fn sanitized(self) -> Self {
        let pos = |v: f32, d: f32| if v.is_finite() && v > 0.0 { v } else { d };
        let d = Self::default();
        Self {
            gravity: pos(self.gravity, d.gravity),
            jump_speed: pos(self.jump_speed, d.jump_speed),
            max_fall_speed: pos(self.max_fall_speed, d.max_fall_speed),
            climb_speed: pos(self.climb_speed, d.climb_speed),
            body: Vec2::new(pos(self.body.x, d.body.x), pos(self.body.y, d.body.y)),
        }
    }
}

/// 유닛 하나의 횡스크롤 상태. 쿼터뷰 존에서는 쓰지 않는다.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Body {
    /// 세로 속도 (위가 +).
    pub(crate) vy: f32,
    /// 키보드 걷기 방향 (-1 왼쪽 · 0 · 1 오른쪽).
    pub(crate) walk: i8,
    /// 사다리 오르내리기 방향. `climbing` 일 때만 본다.
    pub(crate) climb: i8,
    pub(crate) climbing: bool,
    /// 땅·발판을 딛고 있다 (지난 tick 끝 기준).
    pub(crate) grounded: bool,
    /// 이 높이 이상에 윗면이 있는 발판은 무시한다 — 발판에서 내려서는 중.
    pub(crate) drop_from: Option<f32>,
    /// 이번 tick 에 가로로 움직였다 — 걷기 애니메이션용.
    pub(crate) walked: bool,
}

/// 한 tick 의 결과.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Outcome {
    /// `MoveTo` 가 없었다.
    Idle,
    /// `MoveTo` 로 걷는 중.
    Moving,
    /// `MoveTo` 목표의 X 에 닿았다.
    Arrived,
    /// 벽·낭떠러지에 막혀 `MoveTo` 를 그만뒀다.
    Blocked,
}

const EPS: f32 = 1e-3;

/// 칸이 통과할 수 없는 블록인가. 맵 밖은 벽이다.
fn solid(tiles: &TileMap, c: TileCoord) -> bool {
    tiles.get(c).is_none_or(|t| !t.walkable)
}

fn ladder(tiles: &TileMap, c: TileCoord) -> bool {
    tiles.get(c).is_some_and(|t| t.walkable && t.ramp)
}

/// 위에서만 밟히는 발판인가. **사다리 맨 윗칸도 발판이다** — 올라가서 그 위에 선다.
fn platform(tiles: &TileMap, c: TileCoord) -> bool {
    tiles
        .get(c)
        .is_some_and(|t| t.walkable && !t.ramp && t.level >= 1)
        || (ladder(tiles, c) && !ladder(tiles, TileCoord::new(c.x, c.y + 1)))
}

/// 발판을 밟을 수 있는가 — 내려서는 중이면 그 높이 이상의 발판은 건너뛴다.
fn may_stand_on_platform(top: f32, drop_from: Option<f32>) -> bool {
    drop_from.is_none_or(|d| top < d - EPS)
}

/// `v` 이상인 가장 가까운 칸 경계 (한 축).
fn grid_ceil(v: f32, origin: f32, size: f32) -> f32 {
    origin + ((v - origin) / size - EPS).ceil() * size
}

/// `v` 이하인 가장 가까운 칸 경계 (한 축).
fn grid_floor(v: f32, origin: f32, size: f32) -> f32 {
    origin + ((v - origin) / size + EPS).floor() * size
}

fn overlaps_solid(tiles: &TileMap, min: Vec2, max: Vec2) -> bool {
    let lo = tiles.world_to_tile(min);
    let hi = tiles.world_to_tile(max - Vec2::splat(EPS));
    (lo.y..=hi.y).any(|y| (lo.x..=hi.x).any(|x| solid(tiles, TileCoord::new(x, y))))
}

fn body_box(pos: Vec2, body: Vec2) -> (Vec2, Vec2) {
    (
        Vec2::new(pos.x - body.x * 0.5, pos.y),
        Vec2::new(pos.x + body.x * 0.5, pos.y + body.y),
    )
}

/// 발밑 칸 줄 — 몸 폭이 걸치는 칸들.
fn feet_cells(tiles: &TileMap, pos: Vec2, body: Vec2) -> impl Iterator<Item = TileCoord> {
    let (min, max) = body_box(pos, body);
    let lo = tiles.world_to_tile(Vec2::new(min.x, pos.y - EPS * 2.0));
    let hi = tiles.world_to_tile(Vec2::new(max.x - EPS, pos.y - EPS * 2.0));
    (lo.x..=hi.x).map(move |x| TileCoord::new(x, lo.y))
}

/// 발밑에 딛을 것이 있는가 — 블록이나 (내려서는 중이 아닌) 발판.
fn supported(tiles: &TileMap, pos: Vec2, body: Vec2, drop_from: Option<f32>) -> bool {
    let size = tiles.tile_size();
    feet_cells(tiles, pos, body).any(|c| {
        let top = tiles.tile_bounds(c).1.y;
        // 칸 윗면이 발 높이와 맞아야 딛은 것이다.
        if (top - pos.y).abs() > size * 0.25 {
            return false;
        }
        solid(tiles, c) || (platform(tiles, c) && may_stand_on_platform(top, drop_from))
    })
}

/// 몸이 걸친 사다리 칸. `below` 면 발밑 바로 아래 칸(위에서 내려갈 때)을 본다.
fn ladder_under(tiles: &TileMap, pos: Vec2, body: Vec2, below: bool) -> Option<TileCoord> {
    let probe_y = if below {
        pos.y - EPS * 4.0
    } else {
        pos.y + body.y * 0.3
    };
    let c = tiles.world_to_tile(Vec2::new(pos.x, probe_y));
    ladder(tiles, c).then_some(c)
}

/// `Intent::Climb` 로 잡을 수 있는 사다리 — 그 칸 가운데 X.
pub(crate) fn grab_ladder(tiles: &TileMap, pos: Vec2, body: Vec2, dir: i8) -> Option<f32> {
    let c = ladder_under(tiles, pos, body, dir < 0)
        .or_else(|| ladder_under(tiles, pos, body, false))?;
    Some(tiles.tile_center(c).x)
}

/// 발판(블록이 아닌) 위에 서 있으면 그 높이 — ↓ 로 내려설 수 있는 자리.
pub(crate) fn on_platform(tiles: &TileMap, pos: Vec2, body: Vec2) -> Option<f32> {
    let mut any_platform = false;
    for c in feet_cells(tiles, pos, body) {
        if solid(tiles, c) {
            return None;
        }
        any_platform |= platform(tiles, c);
    }
    any_platform.then_some(pos.y)
}

/// 블록에 박혀 있으면 위로 밀어 올린다 — 마커를 땅 칸에 찍었거나 칠하기로 몸 위에 블록이 생겼을 때.
pub(crate) fn unstick(tiles: &TileMap, pos: &mut Vec2, body: Vec2) {
    let (oy, size) = (tiles.origin().y, tiles.tile_size());
    for _ in 0..64 {
        let (min, max) = body_box(*pos, body);
        if !overlaps_solid(tiles, min, max) {
            return;
        }
        pos.y = grid_floor(pos.y, oy, size) + size;
    }
}

/// 한 tick. `goal` 은 `MoveTo` 의 목표 X (없으면 `b.walk` 로 걷는다).
/// `ledge_stop` 이면 발판 끝에서 멈춘다 (몬스터·NPC).
#[allow(clippy::too_many_arguments)]
pub(crate) fn step(
    cfg: &Platformer,
    tiles: &TileMap,
    pos: &mut Vec2,
    heading: &mut f32,
    b: &mut Body,
    goal: Option<f32>,
    speed: f32,
    ledge_stop: bool,
    dt: f32,
) -> Outcome {
    let body = cfg.body;
    let size = tiles.tile_size();
    let origin = tiles.origin();
    // 칸보다 짧게 끊어 움직인다 — 빠른 유닛이 얇은 벽을 건너뛰지 않게.
    let stride = size * 0.45;
    unstick(tiles, pos, body);
    b.walked = false;

    // ── 사다리 ──────────────────────────────────────────────────────────────
    if b.climbing {
        let next = Vec2::new(pos.x, pos.y + f32::from(b.climb) * cfg.climb_speed * dt);
        let (min, max) = body_box(next, body);
        let still_on = ladder_under(tiles, next, body, false).is_some()
            || (b.climb < 0 && ladder_under(tiles, next, body, true).is_some());
        if overlaps_solid(tiles, min, max) {
            if b.climb < 0 {
                // 바닥 블록에 닿았다 — 그 위에 내려선다.
                pos.y = grid_ceil(next.y, origin.y, size);
                b.climbing = false;
                b.grounded = true;
            }
            b.climb = 0;
        } else if !still_on {
            b.climbing = false;
            if b.climb > 0 {
                // 사다리 끝 — 맨 윗칸 위(발판)에 올라선다.
                let c = tiles.world_to_tile(Vec2::new(pos.x, pos.y + body.y * 0.3));
                pos.y = tiles.tile_bounds(c).1.y.max(pos.y);
                b.grounded = true;
            }
        } else {
            *pos = next;
        }
        b.vy = 0.0;
        if b.climbing {
            b.grounded = false;
            if b.climb != 0 {
                *heading = core::f32::consts::FRAC_PI_2;
            }
            return Outcome::Idle;
        }
    }

    // ── 가로 ────────────────────────────────────────────────────────────────
    let dir = match goal {
        Some(gx) if (gx - pos.x).abs() > EPS => (gx - pos.x).signum(),
        Some(_) => 0.0,
        None => f32::from(b.walk.signum()),
    };
    let mut outcome = match goal {
        Some(_) if dir == 0.0 => Outcome::Arrived,
        Some(_) => Outcome::Moving,
        None => Outcome::Idle,
    };
    if dir != 0.0 {
        *heading = if dir > 0.0 {
            0.0
        } else {
            core::f32::consts::PI
        };
        let mut left = speed * dt;
        if let Some(gx) = goal {
            left = left.min((gx - pos.x).abs());
        }
        while left > 0.0 {
            let d = left.min(stride);
            let next = Vec2::new(pos.x + dir * d, pos.y);
            let (min, max) = body_box(next, body);
            if overlaps_solid(tiles, min, max) {
                // 벽 앞 칸 경계까지 붙인다.
                let (cmin, cmax) = body_box(*pos, body);
                let half = body.x * 0.5;
                let snap = if dir > 0.0 {
                    grid_ceil(cmax.x, origin.x, size) - half
                } else {
                    grid_floor(cmin.x, origin.x, size) + half
                };
                let (smin, smax) = body_box(Vec2::new(snap, pos.y), body);
                if (snap - pos.x) * dir > 0.0
                    && (snap - pos.x).abs() <= d
                    && !overlaps_solid(tiles, smin, smax)
                {
                    pos.x = snap;
                    b.walked = true;
                }
                if goal.is_some() {
                    outcome = Outcome::Blocked;
                }
                break;
            }
            if ledge_stop && b.grounded && !supported(tiles, next, body, b.drop_from) {
                if goal.is_some() {
                    outcome = Outcome::Blocked;
                }
                break;
            }
            pos.x = next.x;
            b.walked = true;
            left -= d;
        }
        if let Some(gx) = goal
            && outcome == Outcome::Moving
            && (gx - pos.x).abs() <= EPS
        {
            pos.x = gx;
            outcome = Outcome::Arrived;
        }
    }

    // ── 세로 ────────────────────────────────────────────────────────────────
    if b.vy <= 0.0 && supported(tiles, *pos, body, b.drop_from) {
        b.vy = 0.0;
        b.grounded = true;
        b.drop_from = None;
        return outcome;
    }
    b.grounded = false;
    b.vy = (b.vy - cfg.gravity * dt).max(-cfg.max_fall_speed);
    let mut left = b.vy * dt;
    while left != 0.0 {
        let d = left.signum() * left.abs().min(stride);
        let from_feet = pos.y;
        let next = Vec2::new(pos.x, pos.y + d);
        let (nmin, nmax) = body_box(next, body);
        if d > 0.0 {
            if overlaps_solid(tiles, nmin, nmax) {
                // 천장 — 머리를 박고 떨어지기 시작한다.
                let head = grid_ceil(pos.y + body.y, origin.y, size);
                pos.y = (head - body.y).max(from_feet);
                b.vy = 0.0;
                break;
            }
        } else {
            // 발이 지나간 칸 경계 중 처음 걸리는 윗면에 선다.
            let lo = tiles.world_to_tile(Vec2::new(nmin.x, next.y));
            let hi = tiles.world_to_tile(Vec2::new(nmax.x - EPS, next.y));
            let mut landed: Option<f32> = None;
            for x in lo.x..=hi.x {
                let c = TileCoord::new(x, lo.y);
                let top = tiles.tile_bounds(c).1.y;
                let crossed = from_feet >= top - EPS && next.y < top;
                let can_stand = solid(tiles, c)
                    || (platform(tiles, c) && may_stand_on_platform(top, b.drop_from));
                if crossed && can_stand {
                    landed = Some(landed.map_or(top, |t| t.max(top)));
                }
            }
            if let Some(top) = landed {
                pos.y = top;
                b.vy = 0.0;
                b.grounded = true;
                b.drop_from = None;
                break;
            }
        }
        pos.y = next.y;
        left -= d;
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tilemap::Tile;

    const DT: f32 = 0.05;

    /// 20×12 칸(1m), 바닥은 y=0 줄이 블록. 서 있는 높이는 1.0.
    fn room() -> TileMap {
        let mut t = TileMap::new(20, 12, 1.0, Vec2::ZERO, Tile::default());
        for x in 0..20 {
            t.set(TileCoord::new(x, 0), block());
        }
        t
    }

    fn block() -> Tile {
        Tile {
            walkable: false,
            ..Tile::default()
        }
    }

    fn plat() -> Tile {
        Tile {
            level: 1,
            ..Tile::default()
        }
    }

    fn rung() -> Tile {
        Tile {
            ramp: true,
            ..Tile::default()
        }
    }

    struct Guy {
        pos: Vec2,
        heading: f32,
        b: Body,
    }

    impl Guy {
        fn at(x: f32, y: f32) -> Self {
            Self {
                pos: Vec2::new(x, y),
                heading: 0.0,
                b: Body::default(),
            }
        }

        fn run(&mut self, t: &TileMap, goal: Option<f32>, ledge: bool, ticks: u32) -> Outcome {
            let cfg = Platformer::default();
            let mut last = Outcome::Idle;
            for _ in 0..ticks {
                last = step(
                    &cfg,
                    t,
                    &mut self.pos,
                    &mut self.heading,
                    &mut self.b,
                    goal,
                    4.0,
                    ledge,
                    DT,
                );
                if goal.is_some() && last != Outcome::Moving {
                    break;
                }
            }
            last
        }
    }

    #[test]
    fn falls_and_lands_on_the_floor() {
        let t = room();
        let mut g = Guy::at(5.5, 6.0);
        g.run(&t, None, false, 40);
        assert!((g.pos.y - 1.0).abs() < 1e-4, "{:?}", g.pos);
        assert!(g.b.grounded);
    }

    #[test]
    fn jump_height_follows_the_formula() {
        let t = room();
        let mut g = Guy::at(5.5, 1.0);
        g.run(&t, None, false, 2);
        g.b.vy = Platformer::default().jump_speed;
        let mut top: f32 = 0.0;
        for _ in 0..40 {
            g.run(&t, None, false, 1);
            top = top.max(g.pos.y);
        }
        let cfg = Platformer::default();
        let expect = cfg.jump_speed * cfg.jump_speed / (2.0 * cfg.gravity);
        assert!(
            (top - 1.0 - expect).abs() < 0.4,
            "top={top} expect={expect}"
        );
        assert!((g.pos.y - 1.0).abs() < 1e-4, "다시 내려선다: {:?}", g.pos);
    }

    #[test]
    fn walls_stop_walking_and_moveto_reports_blocked() {
        let mut t = room();
        t.set(TileCoord::new(10, 1), block());
        let mut g = Guy::at(5.5, 1.0);
        g.run(&t, None, false, 2);
        assert_eq!(g.run(&t, Some(15.0), false, 60), Outcome::Blocked);
        assert!((g.pos.x - 9.7).abs() < 1e-3, "벽에 붙는다: {:?}", g.pos);
    }

    #[test]
    fn moveto_walks_only_along_x() {
        let t = room();
        let mut g = Guy::at(2.5, 1.0);
        g.run(&t, None, false, 2);
        assert_eq!(g.run(&t, Some(8.0), false, 100), Outcome::Arrived);
        assert!((g.pos - Vec2::new(8.0, 1.0)).length() < 1e-4, "{:?}", g.pos);
    }

    #[test]
    fn monsters_stop_at_ledges_players_fall() {
        let mut t = room();
        for x in 2..8 {
            t.set(TileCoord::new(x, 4), plat());
        }
        let mut mob = Guy::at(4.5, 5.0);
        mob.run(&t, None, true, 2);
        assert!(mob.b.grounded, "발판 위");
        assert_eq!(mob.run(&t, Some(12.0), true, 60), Outcome::Blocked);
        assert!(
            mob.pos.x <= 8.3 && (mob.pos.y - 5.0).abs() < 1e-4,
            "{:?}",
            mob.pos
        );

        let mut hero = Guy::at(4.5, 5.0);
        hero.run(&t, None, false, 2);
        hero.run(&t, Some(12.0), false, 80);
        hero.run(&t, None, false, 40);
        assert!(
            (hero.pos.y - 1.0).abs() < 1e-4,
            "떨어져 바닥에: {:?}",
            hero.pos
        );
    }

    #[test]
    fn platforms_hold_from_above_and_let_you_through_from_below() {
        let mut t = room();
        for x in 3..8 {
            t.set(TileCoord::new(x, 1), plat());
        }
        // 아래에서 뛰어 올라 뚫고 지나간 뒤 위에 선다 (점프 높이 약 1.67m, 발판 윗면은 1m 위).
        let mut g = Guy::at(5.5, 1.0);
        g.run(&t, None, false, 2);
        g.b.vy = Platformer::default().jump_speed;
        g.run(&t, None, false, 60);
        assert!((g.pos.y - 2.0).abs() < 1e-4, "발판 위: {:?}", g.pos);
        // ↓ 로 내려서면 바닥까지.
        g.b.drop_from = on_platform(&t, g.pos, Platformer::default().body);
        assert!(g.b.drop_from.is_some());
        g.b.grounded = false;
        g.run(&t, None, false, 60);
        assert!((g.pos.y - 1.0).abs() < 1e-4, "바닥: {:?}", g.pos);
        assert_eq!(
            on_platform(&t, g.pos, Platformer::default().body),
            None,
            "블록 위"
        );
    }

    #[test]
    fn ladders_climb_up_to_the_platform_and_back_down() {
        let mut t = room();
        for x in 3..8 {
            t.set(TileCoord::new(x, 4), plat());
        }
        for y in 1..5 {
            t.set(TileCoord::new(5, y), rung());
        }
        let body = Platformer::default().body;
        let mut g = Guy::at(5.2, 1.0);
        g.run(&t, None, false, 2);
        g.pos.x = grab_ladder(&t, g.pos, body, 1).expect("사다리 앞");
        g.b.climbing = true;
        g.b.climb = 1;
        g.run(&t, None, false, 60);
        assert!(!g.b.climbing, "끝까지 올라 내렸다");
        assert!((g.pos.y - 5.0).abs() < 1e-4, "발판 위: {:?}", g.pos);
        assert!(g.b.grounded);

        // 위에서 ↓ — 사다리를 타고 바닥까지.
        assert!(grab_ladder(&t, g.pos, body, -1).is_some());
        g.b.climbing = true;
        g.b.climb = -1;
        g.run(&t, None, false, 80);
        assert!(!g.b.climbing);
        assert!((g.pos.y - 1.0).abs() < 1e-4, "바닥: {:?}", g.pos);
    }

    #[test]
    fn stuck_in_a_block_is_pushed_up() {
        let t = room();
        let mut p = Vec2::new(3.5, 0.2);
        unstick(&t, &mut p, Platformer::default().body);
        assert!((p.y - 1.0).abs() < 1e-4, "{p:?}");
    }

    #[test]
    fn outside_the_map_is_a_wall() {
        let t = room();
        let mut g = Guy::at(1.0, 1.0);
        g.run(&t, None, false, 2);
        g.run(&t, Some(-5.0), false, 40);
        assert!((g.pos.x - 0.3).abs() < 1e-3, "{:?}", g.pos);
    }
}
