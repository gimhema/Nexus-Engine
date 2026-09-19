//! 타일맵 — 걷기 가능 여부, 높이 레벨, 길찾기.
//!
//! # 높이는 렌더링이 아니라 규칙이다
//!
//! 지형은 **화면에서 완전한 평지**다. 높이는 타일마다 붙은 이산 레벨([`Tile::level`])이고,
//! 스타크래프트처럼 **게임 규칙에서만** 의미를 갖는다:
//!
//! - **이동**: 레벨이 다른 타일 사이는 경사로([`Tile::ramp`])를 통해서만, 한 단계씩.
//! - **시야**: 높은 레벨에서 낮은 쪽은 보이고, 낮은 레벨에서 높은 쪽은 보이지 않는다.
//!
//! 높이 지오메트리를 만들지 않으므로 렌더러는 이 모듈을 전혀 모른다.
//! 다만 **절벽 경계는 타일 아트로 표현해야** 화면에서 구분이 된다.
//!
//! # 좌표
//!
//! 타일 `(x, y)` 는 월드 `[origin + (x, y) * size, origin + (x+1, y+1) * size)` 를 덮는다.
//! 타일 y 가 늘면 월드 +Y(북쪽)로 간다 — `nexus_core::units` 규약과 같은 방향이다.

use std::collections::BinaryHeap;

use nexus_core::Vec2;

/// 타일 좌표. 음수와 범위 밖을 표현할 수 있어야 이웃 계산이 단순해진다.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TileCoord {
    pub x: i32,
    pub y: i32,
}

impl TileCoord {
    #[must_use]
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// 타일 한 칸.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Tile {
    /// 지나갈 수 있는가. 벽·물 등은 `false`.
    pub walkable: bool,
    /// 높이 레벨. 같은 레벨끼리는 자유롭게 오간다.
    pub level: u8,
    /// 경사로인가. 자기 레벨과 **한 단계** 차이나는 이웃을 이어 준다.
    pub ramp: bool,
}

impl Default for Tile {
    fn default() -> Self {
        Self {
            walkable: true,
            level: 0,
            ramp: false,
        }
    }
}

/// 길찾기 비용 — 정수로 두면 `f32` 정렬 문제를 피할 수 있다.
const COST_ORTHOGONAL: u32 = 10;
/// 대각선은 √2 배. `14/10 ≈ 1.414`.
const COST_DIAGONAL: u32 = 14;
/// 레벨이 바뀌는 걸음에 붙는 추가 비용. 경사로를 "돌아가는 길"로 취급하게 한다.
const COST_LEVEL_CHANGE: u32 = 10;

/// 한 번의 길찾기가 살펴볼 최대 칸 수. 넓은 맵에서 폭주하지 않게 한다.
const MAX_SEARCH_NODES: usize = 1 << 20;

/// 균일 격자 타일맵.
#[derive(Clone, Debug)]
pub struct TileMap {
    width: u32,
    height: u32,
    tile_size: f32,
    origin: Vec2,
    tiles: Vec<Tile>,
}

impl TileMap {
    /// 모든 칸을 `fill` 로 채운 맵을 만든다.
    ///
    /// `width`/`height` 가 0 이면 1 로, `tile_size` 가 0 이하이면 1.0 으로 보정한다 —
    /// 0 크기 맵은 좌표 변환에서 나눗셈이 터진다.
    #[must_use]
    pub fn new(width: u32, height: u32, tile_size: f32, origin: Vec2, fill: Tile) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        let tile_size = if tile_size > 0.0 { tile_size } else { 1.0 };
        Self {
            width,
            height,
            tile_size,
            origin,
            tiles: vec![fill; width as usize * height as usize],
        }
    }

    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    #[must_use]
    pub fn tile_size(&self) -> f32 {
        self.tile_size
    }

    #[must_use]
    pub fn origin(&self) -> Vec2 {
        self.origin
    }

    /// 맵 전체가 덮는 월드 영역 `(min, max)`.
    #[must_use]
    pub fn world_bounds(&self) -> (Vec2, Vec2) {
        let size = Vec2::new(self.width as f32, self.height as f32) * self.tile_size;
        (self.origin, self.origin + size)
    }

    #[must_use]
    pub fn contains(&self, at: TileCoord) -> bool {
        at.x >= 0 && at.y >= 0 && (at.x as u32) < self.width && (at.y as u32) < self.height
    }

    fn index(&self, at: TileCoord) -> Option<usize> {
        self.contains(at)
            .then(|| at.y as usize * self.width as usize + at.x as usize)
    }

    #[must_use]
    pub fn get(&self, at: TileCoord) -> Option<Tile> {
        self.index(at).map(|i| self.tiles[i])
    }

    /// 범위 밖이면 아무 일도 하지 않고 `false`.
    pub fn set(&mut self, at: TileCoord, tile: Tile) -> bool {
        match self.index(at) {
            Some(i) => {
                self.tiles[i] = tile;
                true
            }
            None => false,
        }
    }

    /// 월드 좌표가 속한 타일. **범위 밖일 수 있다** — [`contains`](Self::contains) 로 확인할 것.
    #[must_use]
    pub fn world_to_tile(&self, world: Vec2) -> TileCoord {
        let local = (world - self.origin) / self.tile_size;
        // floor 를 써야 원점 왼쪽·아래에서도 올바른 칸이 나온다 (as i32 는 0 쪽으로 자른다).
        TileCoord::new(local.x.floor() as i32, local.y.floor() as i32)
    }

    /// 타일 한가운데의 월드 좌표.
    #[must_use]
    pub fn tile_center(&self, at: TileCoord) -> Vec2 {
        self.origin + (Vec2::new(at.x as f32, at.y as f32) + Vec2::splat(0.5)) * self.tile_size
    }

    /// 타일이 덮는 월드 영역 `(min, max)`.
    #[must_use]
    pub fn tile_bounds(&self, at: TileCoord) -> (Vec2, Vec2) {
        let min = self.origin + Vec2::new(at.x as f32, at.y as f32) * self.tile_size;
        (min, min + Vec2::splat(self.tile_size))
    }

    // ── 규칙 ─────────────────────────────────────────────────────────────────

    /// 두 타일이 레벨 규칙상 이어지는가. 인접 여부는 보지 않는다.
    fn levels_connect(a: Tile, b: Tile) -> bool {
        if !a.walkable || !b.walkable {
            return false;
        }
        let diff = a.level.abs_diff(b.level);
        match diff {
            0 => true,
            // 한 단계 차이는 둘 중 하나가 경사로여야 넘어간다.
            1 => a.ramp || b.ramp,
            _ => false,
        }
    }

    /// `from` 에서 `to` 로 **한 걸음** 갈 수 있는가.
    ///
    /// 대각선은 **양옆 칸이 모두 지나갈 수 있어야** 한다 — 그러지 않으면 벽 모서리를
    /// 뚫고 지나간다.
    #[must_use]
    pub fn can_step(&self, from: TileCoord, to: TileCoord) -> bool {
        let (Some(a), Some(b)) = (self.get(from), self.get(to)) else {
            return false;
        };
        let (dx, dy) = (to.x - from.x, to.y - from.y);
        if dx == 0 && dy == 0 || dx.abs() > 1 || dy.abs() > 1 {
            return false;
        }
        if !Self::levels_connect(a, b) {
            return false;
        }
        if dx != 0 && dy != 0 {
            let side = |c: TileCoord| {
                self.get(c)
                    .is_some_and(|s| Self::levels_connect(a, s) && Self::levels_connect(s, b))
            };
            if !side(TileCoord::new(to.x, from.y)) || !side(TileCoord::new(from.x, to.y)) {
                return false;
            }
        }
        true
    }

    /// `observer` 에서 `target` 이 보이는가.
    ///
    /// 스타크래프트 규칙 — **높은 쪽에서 낮은 쪽은 보이고, 낮은 쪽에서 높은 쪽은 안 보인다.**
    /// 벽에 의한 시선 차단은 아직 다루지 않는다 (레벨만 본다).
    #[must_use]
    pub fn can_see(&self, observer: TileCoord, target: TileCoord) -> bool {
        match (self.get(observer), self.get(target)) {
            (Some(o), Some(t)) => o.level >= t.level,
            _ => false,
        }
    }

    /// 한 걸음의 비용. [`can_step`](Self::can_step) 이 참인 걸음에만 의미가 있다.
    fn step_cost(&self, from: TileCoord, to: TileCoord) -> u32 {
        let diagonal = from.x != to.x && from.y != to.y;
        let base = if diagonal {
            COST_DIAGONAL
        } else {
            COST_ORTHOGONAL
        };
        let changes_level = match (self.get(from), self.get(to)) {
            (Some(a), Some(b)) => a.level != b.level,
            _ => false,
        };
        base + if changes_level { COST_LEVEL_CHANGE } else { 0 }
    }

    /// 대각선을 허용하는 8방향 옥타일 휴리스틱. 실제 비용을 넘지 않는다(허용적).
    fn heuristic(from: TileCoord, to: TileCoord) -> u32 {
        let dx = from.x.abs_diff(to.x);
        let dy = from.y.abs_diff(to.y);
        let (lo, hi) = if dx < dy { (dx, dy) } else { (dy, dx) };
        COST_DIAGONAL * lo + COST_ORTHOGONAL * (hi - lo)
    }

    /// `from` → `to` 최단 경로. 반환값은 **`from` 과 `to` 를 모두 포함**한다.
    ///
    /// 길이 없거나 양 끝이 범위 밖·못 걷는 칸이면 `None`.
    /// `from == to` 이면 그 칸 하나짜리 경로.
    #[must_use]
    pub fn find_path(&self, from: TileCoord, to: TileCoord) -> Option<Vec<TileCoord>> {
        if !self.get(from).is_some_and(|t| t.walkable) || !self.get(to).is_some_and(|t| t.walkable)
        {
            return None;
        }
        if from == to {
            return Some(vec![from]);
        }

        let len = self.tiles.len();
        let idx = |c: TileCoord| c.y as usize * self.width as usize + c.x as usize;

        let mut cost = vec![u32::MAX; len];
        let mut came: Vec<Option<TileCoord>> = vec![None; len];
        let mut closed = vec![false; len];
        // (우선순위, 위치) — BinaryHeap 이 최대 힙이라 f 값을 뒤집어 넣는다.
        let mut open = BinaryHeap::new();

        cost[idx(from)] = 0;
        open.push((std::cmp::Reverse(Self::heuristic(from, to)), from));

        let mut visited = 0usize;
        while let Some((_, current)) = open.pop() {
            if current == to {
                let mut path = vec![current];
                let mut at = current;
                while let Some(prev) = came[idx(at)] {
                    path.push(prev);
                    at = prev;
                }
                path.reverse();
                return Some(path);
            }

            let ci = idx(current);
            if closed[ci] {
                continue;
            }
            closed[ci] = true;

            visited += 1;
            if visited > MAX_SEARCH_NODES {
                return None;
            }

            for dy in -1..=1 {
                for dx in -1..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let next = TileCoord::new(current.x + dx, current.y + dy);
                    if !self.can_step(current, next) {
                        continue;
                    }
                    let ni = idx(next);
                    if closed[ni] {
                        continue;
                    }
                    let tentative = cost[ci].saturating_add(self.step_cost(current, next));
                    if tentative < cost[ni] {
                        cost[ni] = tentative;
                        came[ni] = Some(current);
                        open.push((
                            std::cmp::Reverse(tentative + Self::heuristic(next, to)),
                            next,
                        ));
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(w: u32, h: u32) -> TileMap {
        TileMap::new(w, h, 1.0, Vec2::ZERO, Tile::default())
    }

    fn at(x: i32, y: i32) -> TileCoord {
        TileCoord::new(x, y)
    }

    fn wall(map: &mut TileMap, x: i32, y: i32) {
        map.set(
            at(x, y),
            Tile {
                walkable: false,
                ..Tile::default()
            },
        );
    }

    fn raise(map: &mut TileMap, x: i32, y: i32, level: u8, ramp: bool) {
        map.set(
            at(x, y),
            Tile {
                walkable: true,
                level,
                ramp,
            },
        );
    }

    // ── 좌표 ─────────────────────────────────────────────────────────────────

    #[test]
    fn world_and_tile_round_trip() {
        let map = TileMap::new(10, 10, 2.0, Vec2::new(-5.0, 3.0), Tile::default());
        for c in [at(0, 0), at(9, 9), at(4, 7)] {
            assert_eq!(map.world_to_tile(map.tile_center(c)), c, "{c:?}");
        }
    }

    #[test]
    fn world_to_tile_floors_towards_negative() {
        // `as i32` 는 0 쪽으로 자르므로 원점 왼쪽·아래에서 한 칸 어긋난다.
        let map = TileMap::new(4, 4, 1.0, Vec2::ZERO, Tile::default());
        assert_eq!(map.world_to_tile(Vec2::new(-0.1, -0.1)), at(-1, -1));
        assert_eq!(map.world_to_tile(Vec2::new(0.1, 0.1)), at(0, 0));
    }

    #[test]
    fn tile_y_increases_northwards() {
        let map = flat(4, 4);
        assert!(map.tile_center(at(0, 1)).y > map.tile_center(at(0, 0)).y);
    }

    #[test]
    fn out_of_range_access_is_none_not_panic() {
        let map = flat(3, 3);
        for c in [at(-1, 0), at(0, -1), at(3, 0), at(0, 3)] {
            assert!(!map.contains(c));
            assert!(map.get(c).is_none());
        }
        let mut map = map;
        assert!(!map.set(at(99, 99), Tile::default()));
    }

    #[test]
    fn degenerate_size_is_corrected() {
        let map = TileMap::new(0, 0, 0.0, Vec2::ZERO, Tile::default());
        assert_eq!((map.width(), map.height()), (1, 1));
        assert!(map.tile_size() > 0.0, "0 크기는 좌표 변환에서 터진다");
        assert!(map.tile_center(at(0, 0)).is_finite());
    }

    // ── 이동 규칙 ────────────────────────────────────────────────────────────

    #[test]
    fn step_requires_adjacency() {
        let map = flat(5, 5);
        assert!(map.can_step(at(2, 2), at(3, 2)));
        assert!(map.can_step(at(2, 2), at(3, 3)));
        assert!(!map.can_step(at(2, 2), at(2, 2)), "제자리는 걸음이 아니다");
        assert!(
            !map.can_step(at(2, 2), at(4, 2)),
            "두 칸은 한 걸음이 아니다"
        );
    }

    #[test]
    fn step_into_a_wall_is_rejected() {
        let mut map = flat(5, 5);
        wall(&mut map, 3, 2);
        assert!(!map.can_step(at(2, 2), at(3, 2)));
        assert!(
            !map.can_step(at(3, 2), at(2, 2)),
            "벽에서 나오는 것도 막는다"
        );
    }

    #[test]
    fn diagonal_cannot_cut_a_corner() {
        // 모서리를 뚫고 지나가면 캐릭터가 벽을 통과해 보인다.
        let mut map = flat(5, 5);
        wall(&mut map, 3, 2);
        assert!(!map.can_step(at(2, 2), at(3, 3)), "막힌 옆칸으로 대각 통과");

        wall(&mut map, 2, 3);
        assert!(!map.can_step(at(2, 2), at(3, 3)));
    }

    #[test]
    fn same_level_moves_freely() {
        let mut map = flat(5, 5);
        for x in 0..5 {
            raise(&mut map, x, 2, 3, false);
        }
        assert!(map.can_step(at(1, 2), at(2, 2)), "같은 레벨끼리는 자유롭다");
    }

    #[test]
    fn level_change_needs_a_ramp() {
        let mut map = flat(5, 5);
        raise(&mut map, 2, 2, 1, false);
        assert!(!map.can_step(at(1, 2), at(2, 2)), "경사로 없이 올라갔다");

        raise(&mut map, 2, 2, 1, true);
        assert!(map.can_step(at(1, 2), at(2, 2)), "경사로로 올라가야 한다");
        assert!(map.can_step(at(2, 2), at(1, 2)), "내려오는 것도 된다");
    }

    #[test]
    fn level_gap_of_two_is_never_passable() {
        let mut map = flat(5, 5);
        raise(&mut map, 2, 2, 2, true);
        assert!(
            !map.can_step(at(1, 2), at(2, 2)),
            "경사로여도 두 단계는 못 넘는다"
        );
    }

    // ── 시야 규칙 ────────────────────────────────────────────────────────────

    #[test]
    fn high_ground_sees_low_but_not_the_reverse() {
        let mut map = flat(5, 5);
        raise(&mut map, 4, 4, 1, false);
        assert!(
            map.can_see(at(4, 4), at(0, 0)),
            "높은 쪽이 낮은 쪽을 봐야 한다"
        );
        assert!(!map.can_see(at(0, 0), at(4, 4)), "낮은 쪽이 높은 쪽을 봤다");
        assert!(map.can_see(at(0, 0), at(1, 1)), "같은 레벨은 서로 보인다");
    }

    #[test]
    fn vision_outside_the_map_is_false() {
        let map = flat(3, 3);
        assert!(!map.can_see(at(0, 0), at(9, 9)));
        assert!(!map.can_see(at(9, 9), at(0, 0)));
    }

    // ── 길찾기 ───────────────────────────────────────────────────────────────

    /// 경로가 실제로 이어져 있는지 — 한 걸음씩 전부 `can_step` 이어야 한다.
    fn assert_contiguous(map: &TileMap, path: &[TileCoord]) {
        for pair in path.windows(2) {
            assert!(
                map.can_step(pair[0], pair[1]),
                "이어지지 않는 경로: {:?} → {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn path_includes_both_ends() {
        let map = flat(5, 5);
        let path = map.find_path(at(0, 0), at(4, 4)).expect("경로 없음");
        assert_eq!(path.first(), Some(&at(0, 0)));
        assert_eq!(path.last(), Some(&at(4, 4)));
        assert_contiguous(&map, &path);
    }

    #[test]
    fn straight_diagonal_is_the_shortest() {
        let map = flat(5, 5);
        let path = map.find_path(at(0, 0), at(4, 4)).unwrap();
        assert_eq!(path.len(), 5, "대각선 4걸음이어야 한다: {path:?}");
    }

    #[test]
    fn same_tile_is_a_one_step_path() {
        let map = flat(3, 3);
        assert_eq!(map.find_path(at(1, 1), at(1, 1)), Some(vec![at(1, 1)]));
    }

    #[test]
    fn path_goes_around_a_wall() {
        let mut map = flat(5, 5);
        for y in 0..4 {
            wall(&mut map, 2, y);
        }
        let path = map
            .find_path(at(0, 0), at(4, 0))
            .expect("돌아가는 길이 있다");
        assert_contiguous(&map, &path);
        assert!(
            path.iter().all(|c| map.get(*c).unwrap().walkable),
            "벽을 밟고 지나갔다"
        );
        assert!(path.iter().any(|c| c.y >= 4), "벽 위로 돌아가야 한다");
    }

    #[test]
    fn no_path_returns_none() {
        let mut map = flat(5, 5);
        for y in 0..5 {
            wall(&mut map, 2, y);
        }
        assert_eq!(map.find_path(at(0, 0), at(4, 4)), None);
    }

    #[test]
    fn unwalkable_endpoints_have_no_path() {
        let mut map = flat(5, 5);
        wall(&mut map, 4, 4);
        assert_eq!(map.find_path(at(0, 0), at(4, 4)), None);
        assert_eq!(map.find_path(at(4, 4), at(0, 0)), None);
        assert_eq!(map.find_path(at(0, 0), at(99, 99)), None);
    }

    #[test]
    fn path_to_high_ground_goes_through_the_ramp() {
        // 오른쪽 절반이 레벨 1 인 고지대. 경사로는 (2, 0) 한 칸뿐이다.
        let mut map = flat(5, 5);
        for y in 0..5 {
            for x in 2..5 {
                raise(&mut map, x, y, 1, false);
            }
        }
        assert_eq!(
            map.find_path(at(0, 4), at(4, 4)),
            None,
            "아직 길이 없어야 한다"
        );

        raise(&mut map, 2, 0, 1, true);
        let path = map
            .find_path(at(0, 4), at(4, 4))
            .expect("경사로로 갈 수 있다");
        assert_contiguous(&map, &path);
        assert!(path.contains(&at(2, 0)), "경사로를 지나야 한다: {path:?}");
    }

    #[test]
    fn level_change_costs_extra_so_flat_routes_win() {
        // 같은 거리라면 레벨을 오르내리지 않는 쪽을 고른다.
        let mut map = flat(3, 3);
        for x in 0..3 {
            raise(&mut map, x, 2, 1, true);
        }
        let path = map.find_path(at(0, 0), at(2, 0)).unwrap();
        assert!(
            path.iter().all(|c| map.get(*c).unwrap().level == 0),
            "평지 경로가 있는데 고지대로 돌아갔다: {path:?}"
        );
    }

    #[test]
    fn large_map_terminates() {
        // 폭주하지 않는지 — 막힌 맵에서 전체를 훑고 끝나야 한다.
        let mut map = flat(120, 120);
        for y in 0..120 {
            wall(&mut map, 60, y);
        }
        assert_eq!(map.find_path(at(0, 0), at(119, 119)), None);
        let open = flat(120, 120);
        assert!(open.find_path(at(0, 0), at(119, 119)).is_some());
    }
}
