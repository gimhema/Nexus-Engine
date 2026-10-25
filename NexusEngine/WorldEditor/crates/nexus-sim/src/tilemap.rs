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
//!
//! # 저장소는 청크다 (R6-3)
//!
//! 존 전체(2km)를 1m 타일로 덮으면 400만 칸이다. 대부분은 기본 타일이라 칸을 다 들고 있지 않는다 —
//! [`CHUNK`]×[`CHUNK`] 칸 묶음(청크)으로 나누고, **한 번도 칠하지 않은 청크는 만들지 않는다**
//! (그 칸은 전부 `fill`). 그래서:
//!
//! - 메모리·복제(플레이 시작)·저장이 **칠한 양**에 비례한다 — 맵 크기가 아니라.
//! - 맵을 늘리는 것([`TileMap::grow_to_cover`])이 칸을 옮기지 않는다 — 바깥 좌표만 밀고
//!   안쪽 좌표(`shift`)는 그대로 둔다.
//! - 길찾기는 **살펴본 칸만** 기억한다 — 맵 전체 크기의 배열을 잡지 않는다.
//!
//! 공개 좌표([`TileCoord`])·규칙은 그대로다. 청크는 이 모듈 밖으로 드러나지 않는다.

use std::collections::BinaryHeap;

use nexus_core::Vec2;

/// 청크 한 변의 칸 수.
pub const CHUNK: i32 = 32;
const CHUNK_AREA: usize = (CHUNK * CHUNK) as usize;

type Chunk = Box<[Tile; CHUNK_AREA]>;

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

/// 타일맵 — 균일 격자, 저장은 청크 (모듈 doc 「저장소는 청크다」).
#[derive(Clone)]
pub struct TileMap {
    width: u32,
    height: u32,
    tile_size: f32,
    origin: Vec2,
    /// 만들지 않은 청크의 칸.
    fill: Tile,
    /// 안쪽 좌표 = 바깥 좌표 + `shift`. 맵이 음수 쪽으로 늘면 바깥 좌표만 밀리고 안쪽은 그대로다.
    shift: (i32, i32),
    /// `chunks[0]` 의 청크 좌표(안쪽 좌표 ÷ [`CHUNK`]).
    chunk_min: (i32, i32),
    /// 청크 격자의 가로·세로 청크 수.
    chunk_cols: i32,
    chunk_rows: i32,
    /// 행 우선. `None` = 전부 `fill`.
    chunks: Vec<Option<Chunk>>,
}

// 칸을 다 찍으면 수백만 줄이다 — 크기와 만든 청크 수만.
impl std::fmt::Debug for TileMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TileMap")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("tile_size", &self.tile_size)
            .field("origin", &self.origin)
            .field("chunks", &self.chunk_count())
            .finish_non_exhaustive()
    }
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
        let mut map = Self {
            width,
            height,
            tile_size,
            origin,
            fill,
            shift: (0, 0),
            chunk_min: (0, 0),
            chunk_cols: 0,
            chunk_rows: 0,
            chunks: Vec::new(),
        };
        map.relayout();
        map
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

    /// 만들지 않은 청크의 칸 — 보통 기본 타일.
    #[must_use]
    pub fn fill(&self) -> Tile {
        self.fill
    }

    /// 만든 청크 수 — 메모리가 칠한 양에 비례하는지 볼 때 (R6-3).
    #[must_use]
    pub fn chunk_count(&self) -> usize {
        self.chunks.iter().flatten().count()
    }

    /// 청크 격자를 지금 맵 영역(안쪽 좌표)에 맞춘다 — 이미 만든 청크는 옮길 뿐 복사하지 않는다.
    fn relayout(&mut self) {
        let lo = (
            self.shift.0.div_euclid(CHUNK),
            self.shift.1.div_euclid(CHUNK),
        );
        let hi = (
            (self.shift.0 + self.width as i32 - 1).div_euclid(CHUNK),
            (self.shift.1 + self.height as i32 - 1).div_euclid(CHUNK),
        );
        let (cols, rows) = (hi.0 - lo.0 + 1, hi.1 - lo.1 + 1);
        if lo == self.chunk_min && cols == self.chunk_cols && rows == self.chunk_rows {
            return;
        }
        let mut chunks: Vec<Option<Chunk>> = Vec::new();
        chunks.resize_with((cols * rows) as usize, || None);
        let old = std::mem::take(&mut self.chunks);
        for (i, chunk) in old.into_iter().enumerate() {
            let Some(chunk) = chunk else { continue };
            let i = i as i32;
            let (cx, cy) = (
                self.chunk_min.0 + i % self.chunk_cols,
                self.chunk_min.1 + i / self.chunk_cols,
            );
            // 맵은 줄지 않으므로 옛 청크는 늘 새 격자 안에 있다.
            if (lo.0..=hi.0).contains(&cx) && (lo.1..=hi.1).contains(&cy) {
                chunks[((cy - lo.1) * cols + (cx - lo.0)) as usize] = Some(chunk);
            }
        }
        self.chunk_min = lo;
        self.chunk_cols = cols;
        self.chunk_rows = rows;
        self.chunks = chunks;
    }

    /// 바깥 좌표 → (청크 칸 번호, 청크 안 칸 번호). 범위 밖이면 `None`.
    fn locate(&self, at: TileCoord) -> Option<(usize, usize)> {
        if !self.contains(at) {
            return None;
        }
        let (ix, iy) = (at.x + self.shift.0, at.y + self.shift.1);
        let (cx, cy) = (
            ix.div_euclid(CHUNK) - self.chunk_min.0,
            iy.div_euclid(CHUNK) - self.chunk_min.1,
        );
        let (lx, ly) = (ix.rem_euclid(CHUNK), iy.rem_euclid(CHUNK));
        Some((
            (cy * self.chunk_cols + cx) as usize,
            (ly * CHUNK + lx) as usize,
        ))
    }

    /// 맵이 `min`~`max` 영역을 다 덮도록 **늘린다** (줄이지는 않는다 — 칠한 칸을 잃지 않게).
    /// 새 칸은 `fill`, 칸 격자의 위상(원점)은 그대로다. 한 변은 `max_side` 칸까지.
    ///
    /// 음수 쪽으로 늘면 기존 칸의 좌표가 밀린다 — 돌려준 `(dx, dy)` 를 기존 [`TileCoord`] 에 더하면
    /// 같은 칸을 가리킨다. 바뀐 것이 없으면 `None`. 칸을 옮기지 않으므로 맵 크기와 무관하게 싸다 (R6-3).
    pub fn grow_to_cover(
        &mut self,
        min: Vec2,
        max: Vec2,
        fill: Tile,
        max_side: u32,
    ) -> Option<(i32, i32)> {
        if !(min.is_finite() && max.is_finite()) {
            return None;
        }
        let ts = self.tile_size;
        // 영역 끝이 든 칸까지 (max 는 그 칸을 포함하도록 내림 + 1).
        let lo = |v: f32, o: f32| ((v - o) / ts).floor() as i64;
        let (x0, y0) = (
            lo(min.x, self.origin.x).min(0),
            lo(min.y, self.origin.y).min(0),
        );
        let (x1, y1) = (
            (lo(max.x, self.origin.x) + 1).max(i64::from(self.width)),
            (lo(max.y, self.origin.y) + 1).max(i64::from(self.height)),
        );
        let cap = i64::from(max_side.max(self.width).max(self.height));
        // 상한을 넘으면 기존 맵 쪽을 지키고 늘어나는 쪽을 자른다.
        let x0 = x0.max(x1 - cap).min(0);
        let y0 = y0.max(y1 - cap).min(0);
        let x1 = x1.min(x0 + cap);
        let y1 = y1.min(y0 + cap);
        let (width, height) = ((x1 - x0) as u32, (y1 - y0) as u32);
        if x0 == 0 && y0 == 0 && width == self.width && height == self.height {
            return None;
        }
        let (dx, dy) = ((-x0) as i32, (-y0) as i32);
        let (old_w, old_h) = (self.width as i32, self.height as i32);
        self.origin -= Vec2::new(dx as f32, dy as f32) * ts;
        self.shift = (self.shift.0 - dx, self.shift.1 - dy);
        self.width = width;
        self.height = height;
        self.relayout();
        // 새 칸은 만들지 않은 청크라 `self.fill` 이다. 다른 값을 바라면 새로 덮인 칸만 칠한다.
        if fill != self.fill {
            for y in 0..height as i32 {
                for x in 0..width as i32 {
                    let old = (x - dx, y - dy);
                    if !(0..old_w).contains(&old.0) || !(0..old_h).contains(&old.1) {
                        self.set(TileCoord::new(x, y), fill);
                    }
                }
            }
        }
        Some((dx, dy))
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

    #[must_use]
    pub fn get(&self, at: TileCoord) -> Option<Tile> {
        let (c, i) = self.locate(at)?;
        Some(self.chunks[c].as_ref().map_or(self.fill, |chunk| chunk[i]))
    }

    /// 범위 밖이면 아무 일도 하지 않고 `false`. 만들지 않은 청크에 `fill` 을 쓰면 청크를 만들지 않는다.
    pub fn set(&mut self, at: TileCoord, tile: Tile) -> bool {
        let Some((c, i)) = self.locate(at) else {
            return false;
        };
        let fill = self.fill;
        match &mut self.chunks[c] {
            Some(chunk) => chunk[i] = tile,
            slot @ None => {
                if tile != fill {
                    let mut chunk: Chunk = Box::new([fill; CHUNK_AREA]);
                    chunk[i] = tile;
                    *slot = Some(chunk);
                }
            }
        }
        true
    }

    /// `fill` 이 아닌 칸을 **행 묶음**으로 — `(y, x, 길이, 타일)`, 행 우선 (R6-3).
    /// 만들지 않은 청크는 통째로 건너뛰므로 칠한 양에 비례한다 (존 저장이 쓴다).
    #[must_use]
    pub fn runs(&self) -> Vec<(u32, u32, u32, Tile)> {
        let mut out = Vec::new();
        let (w, h) = (self.width as i32, self.height as i32);
        for y in 0..h {
            let mut x = 0;
            let mut run: Option<(i32, Tile)> = None;
            let mut close = |run: &mut Option<(i32, Tile)>, end: i32| {
                if let Some((start, tile)) = run.take() {
                    out.push((y as u32, start as u32, (end - start) as u32, tile));
                }
            };
            while x < w {
                let Some((c, i)) = self.locate(TileCoord::new(x, y)) else {
                    break;
                };
                let Some(chunk) = &self.chunks[c] else {
                    // 이 청크의 남은 칸은 전부 fill — 열린 묶음을 닫고 청크 끝으로 건너뛴다.
                    close(&mut run, x);
                    x += CHUNK - (x + self.shift.0).rem_euclid(CHUNK);
                    continue;
                };
                let tile = chunk[i];
                if run.is_none_or(|(_, t)| t != tile) {
                    close(&mut run, x);
                    if tile != self.fill {
                        run = Some((x, tile));
                    }
                }
                x += 1;
            }
            close(&mut run, x.min(w));
        }
        out
    }

    /// `lo`~`hi`(양 끝 포함) 안에서 `fill` 이 아닌 칸 — 만들지 않은 청크는 건너뛴다 (R6-3).
    /// 화면에 보이는 칸만 그릴 때 쓴다. 순서는 청크 단위라 행 우선이 아니다.
    pub fn painted_in(
        &self,
        lo: TileCoord,
        hi: TileCoord,
    ) -> impl Iterator<Item = (TileCoord, Tile)> + '_ {
        let x0 = lo.x.max(0);
        let y0 = lo.y.max(0);
        let x1 = hi.x.min(self.width as i32 - 1);
        let y1 = hi.y.min(self.height as i32 - 1);
        let (sx, sy) = self.shift;
        // 빈 범위는 빈 청크 범위로.
        let span = |a: i32, b: i32, s: i32| {
            if b < a {
                (1, 0)
            } else {
                ((a + s).div_euclid(CHUNK), (b + s).div_euclid(CHUNK))
            }
        };
        let (cy0, cy1) = span(y0, y1, sy);
        let (cx0, cx1) = span(x0, x1, sx);
        (cy0..=cy1)
            .flat_map(move |cy| (cx0..=cx1).map(move |cx| (cx, cy)))
            .filter_map(move |(cx, cy)| {
                let slot = (cy - self.chunk_min.1) * self.chunk_cols + (cx - self.chunk_min.0);
                let chunk = self.chunks.get(usize::try_from(slot).ok()?)?.as_ref()?;
                Some((cx, cy, chunk))
            })
            .flat_map(move |(cx, cy, chunk)| {
                chunk.iter().enumerate().filter_map(move |(i, &tile)| {
                    if tile == self.fill {
                        return None;
                    }
                    let i = i as i32;
                    let at =
                        TileCoord::new(cx * CHUNK + i % CHUNK - sx, cy * CHUNK + i / CHUNK - sy);
                    ((x0..=x1).contains(&at.x) && (y0..=y1).contains(&at.y)).then_some((at, tile))
                })
            })
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

        // 작업 공간은 **살펴본 청크만** 배열로 잡는다 — 맵 전체 크기의 배열을 잡으면 2km 존에서 길찾기
        // 한 번에 수십 MB 다. 칸마다 HashMap 을 쓰면 넓게 도는 길찾기가 두 배 느려졌다 (R6-3).
        #[derive(Clone, Copy)]
        struct Node {
            cost: u32,
            /// 어느 쪽에서 왔나 — `NEIGHBORS` 의 번호 + 1. 0 = 출발점(또는 아직 모름).
            came: u8,
            closed: bool,
        }
        const UNSEEN: Node = Node {
            cost: u32::MAX,
            came: 0,
            closed: false,
        };
        const NEIGHBORS: [(i32, i32); 8] = [
            (-1, -1),
            (0, -1),
            (1, -1),
            (-1, 0),
            (1, 0),
            (-1, 1),
            (0, 1),
            (1, 1),
        ];
        type Scratch = Vec<Option<Box<[Node; CHUNK_AREA]>>>;
        fn slot(scratch: &mut Scratch, (c, i): (usize, usize)) -> &mut Node {
            &mut scratch[c].get_or_insert_with(|| Box::new([UNSEEN; CHUNK_AREA]))[i]
        }
        let mut scratch: Scratch = Vec::new();
        scratch.resize_with(self.chunks.len(), || None);
        // (우선순위, 위치) — BinaryHeap 이 최대 힙이라 f 값을 뒤집어 넣는다.
        // 같은 f 값은 좌표 순으로 꺼내므로 결과는 결정적이다.
        let mut open = BinaryHeap::new();

        *slot(&mut scratch, self.locate(from)?) = Node {
            cost: 0,
            came: 0,
            closed: false,
        };
        open.push((std::cmp::Reverse(Self::heuristic(from, to)), from));

        let mut visited = 0usize;
        while let Some((_, current)) = open.pop() {
            let here = self.locate(current)?;
            if current == to {
                let mut path = vec![current];
                let mut at = current;
                loop {
                    let came = slot(&mut scratch, self.locate(at)?).came;
                    let Some(&(dx, dy)) = came
                        .checked_sub(1)
                        .and_then(|k| NEIGHBORS.get(usize::from(k)))
                    else {
                        break;
                    };
                    at = TileCoord::new(at.x - dx, at.y - dy);
                    path.push(at);
                }
                path.reverse();
                return Some(path);
            }

            let node = slot(&mut scratch, here);
            if node.closed {
                continue;
            }
            node.closed = true;
            let base = node.cost;

            visited += 1;
            if visited > MAX_SEARCH_NODES {
                return None;
            }

            for (k, &(dx, dy)) in NEIGHBORS.iter().enumerate() {
                let next = TileCoord::new(current.x + dx, current.y + dy);
                if !self.can_step(current, next) {
                    continue;
                }
                let Some(at) = self.locate(next) else {
                    continue;
                };
                let tentative = base.saturating_add(self.step_cost(current, next));
                let entry = slot(&mut scratch, at);
                if entry.closed || tentative >= entry.cost {
                    continue;
                }
                entry.cost = tentative;
                entry.came = k as u8 + 1;
                open.push((
                    std::cmp::Reverse(tentative + Self::heuristic(next, to)),
                    next,
                ));
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

    #[test]
    fn growing_keeps_painted_tiles_under_the_same_world_spot() {
        let wall = Tile {
            walkable: false,
            ..Tile::default()
        };
        let mut map = TileMap::new(4, 4, 1.0, Vec2::ZERO, Tile::default());
        map.set(at(1, 2), wall);
        let spot = map.tile_center(at(1, 2));

        // 음수 쪽 2.5m, 양수 쪽 6m 까지 — 칸 격자 위상(정수 m)은 그대로.
        let shift = map.grow_to_cover(
            Vec2::new(-2.5, -1.0),
            Vec2::new(6.0, 3.0),
            Tile::default(),
            4096,
        );
        assert_eq!(shift, Some((3, 1)));
        assert_eq!(map.origin(), Vec2::new(-3.0, -1.0));
        assert_eq!((map.width(), map.height()), (10, 5));
        // 같은 칸이 밀린 좌표에 — 월드 위치는 같다.
        let moved = at(1 + 3, 2 + 1);
        assert_eq!(map.get(moved), Some(wall));
        assert_eq!(map.tile_center(moved), spot);
        assert_eq!(map.world_to_tile(spot), moved);
        // 새 칸은 기본 타일.
        assert_eq!(map.get(at(0, 0)), Some(Tile::default()));

        // 이미 덮으면 아무 일도 없다 — 줄이지 않는다.
        assert_eq!(
            map.grow_to_cover(Vec2::ZERO, Vec2::new(1.0, 1.0), Tile::default(), 4096),
            None
        );
        assert_eq!((map.width(), map.height()), (10, 5));
    }

    #[test]
    fn growing_stops_at_the_side_limit_and_keeps_the_old_map() {
        let mut map = TileMap::new(4, 4, 1.0, Vec2::ZERO, Tile::default());
        map.set(
            at(0, 0),
            Tile {
                level: 2,
                ..Tile::default()
            },
        );
        let shift = map.grow_to_cover(Vec2::splat(-100.0), Vec2::splat(100.0), Tile::default(), 16);
        let (dx, dy) = shift.unwrap();
        assert_eq!((map.width(), map.height()), (16, 16));
        assert_eq!(map.get(at(dx, dy)).unwrap().level, 2, "옛 칸은 남는다");
        assert_eq!(
            map.grow_to_cover(Vec2::NAN, Vec2::ONE, Tile::default(), 16),
            None
        );
    }

    // ── 청크 저장소 (R6-3) ───────────────────────────────────────────────────

    /// 단순 배열과 같은 답을 내는지 — 칠하고, 음수·양수 쪽으로 늘리고, 다시 칠하기를 섞어서.
    #[test]
    fn chunked_storage_matches_a_plain_grid() {
        let mut rng = crate::loot::Rng::new(7);
        let mut map = TileMap::new(50, 41, 1.0, Vec2::new(-3.0, 5.0), Tile::default());
        // 기준: 월드 칸 → 타일 (늘어나도 같은 월드 칸은 같은 타일).
        let mut truth: std::collections::HashMap<(i64, i64), Tile> =
            std::collections::HashMap::new();
        let world_key = |map: &TileMap, c: TileCoord| {
            let p = map.tile_center(c);
            (p.x.floor() as i64, p.y.floor() as i64)
        };
        for round in 0..6 {
            for _ in 0..400 {
                let c = at(
                    (rng.next_u64() % u64::from(map.width())) as i32,
                    (rng.next_u64() % u64::from(map.height())) as i32,
                );
                let tile = Tile {
                    walkable: !rng.next_u64().is_multiple_of(3),
                    level: (rng.next_u64() % 3) as u8,
                    ramp: rng.next_u64().is_multiple_of(5),
                };
                assert!(map.set(c, tile));
                truth.insert(world_key(&map, c), tile);
            }
            // 번갈아 음수 쪽·양수 쪽으로 늘린다 (청크 경계에 맞지 않는 양으로).
            let (lo, hi) = map.world_bounds();
            let grow = if round % 2 == 0 {
                (lo - Vec2::new(37.0, 5.0), hi)
            } else {
                (lo, hi + Vec2::new(11.0, 70.0))
            };
            map.grow_to_cover(grow.0, grow.1, Tile::default(), 4096);
        }
        let mut painted = 0;
        for y in 0..map.height() as i32 {
            for x in 0..map.width() as i32 {
                let c = at(x, y);
                let want = truth.get(&world_key(&map, c)).copied().unwrap_or_default();
                assert_eq!(map.get(c), Some(want), "{c:?}");
                painted += usize::from(want != Tile::default());
            }
        }
        // 행 묶음과 보이는 범위 조회가 같은 칸을 돌려준다.
        let from_runs: usize = map.runs().iter().map(|r| r.2 as usize).sum();
        assert_eq!(from_runs, painted);
        for (y, x, len, tile) in map.runs() {
            for dx in 0..len {
                assert_eq!(map.get(at((x + dx) as i32, y as i32)), Some(tile));
            }
        }
        let all = map.painted_in(at(-5, -5), at(i32::MAX, i32::MAX)).count();
        assert_eq!(all, painted);
        let window: Vec<_> = map.painted_in(at(10, 20), at(40, 33)).collect();
        assert!(window.iter().all(|(c, t)| (10..=40).contains(&c.x)
            && (20..=33).contains(&c.y)
            && map.get(*c) == Some(*t)));
    }

    #[test]
    fn a_whole_zone_map_costs_only_what_is_painted() {
        // 2km 존을 1m 로 — 400만 칸. 칠한 칸만 청크가 생긴다.
        let mut map = TileMap::new(2000, 2000, 1.0, Vec2::splat(-1000.0), Tile::default());
        assert_eq!(map.chunk_count(), 0);
        assert_eq!(map.get(at(1999, 1999)), Some(Tile::default()));
        // fill 을 쓰는 것은 청크를 만들지 않는다.
        map.set(at(5, 5), Tile::default());
        assert_eq!(map.chunk_count(), 0);
        for x in 0..100 {
            wall(&mut map, 1000 + x, 1000);
        }
        assert_eq!(map.chunk_count(), 4, "100칸 벽은 32칸 청크 넷에 걸친다");
        assert_eq!(
            map.runs(),
            [(1000, 1000, 100, map.get(at(1000, 1000)).unwrap())]
        );
        // 벽을 돌아가는 길 — 맵 전체를 훑지 않는다.
        let path = map
            .find_path(at(1050, 990), at(1050, 1010))
            .expect("돌아가는 길");
        assert_contiguous(&map, &path);
        // 복제도 칠한 양만큼.
        assert_eq!(map.clone().chunk_count(), 4);
        // 맵 끝에서 끝까지 — 열린 평지라 곧장 간다.
        assert_eq!(
            map.find_path(at(0, 0), at(1999, 0)).map(|p| p.len()),
            Some(2000)
        );
    }
}
