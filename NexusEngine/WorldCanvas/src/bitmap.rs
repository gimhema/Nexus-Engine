//! 팔레트 문자 비트맵과 그 위의 연산.
//!
//! 픽셀은 RGBA 가 아니라 팔레트 문자(`u8`)다. 연산 결과를 그대로 텍스트로 되돌려 보여 줄 수 있다
//! (`worldcanvas show`). 좌표는 `i32` 로 받고 **범위 밖은 조용히 잘라낸다** — `shift` 나
//! 가장자리에 걸친 `patch` 처럼 일부가 밖으로 나가는 것은 정상적인 작업이다.

use crate::color::{KEEP, TRANSPARENT};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bitmap {
    w: u32,
    h: u32,
    px: Vec<u8>,
}

impl Bitmap {
    /// 투명으로 채운 비트맵.
    #[must_use]
    pub fn new(w: u32, h: u32) -> Self {
        Self {
            w,
            h,
            px: vec![TRANSPARENT; w as usize * h as usize],
        }
    }

    #[must_use]
    pub fn width(&self) -> u32 {
        self.w
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.h
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        let inside = x >= 0 && y >= 0 && (x as u32) < self.w && (y as u32) < self.h;
        inside.then(|| y as usize * self.w as usize + x as usize)
    }

    #[must_use]
    pub fn get(&self, x: i32, y: i32) -> Option<u8> {
        self.index(x, y).map(|i| self.px[i])
    }

    pub fn set(&mut self, x: i32, y: i32, c: u8) {
        if let Some(i) = self.index(x, y) {
            self.px[i] = c;
        }
    }

    /// 행 단위 문자열 — 좌상단부터.
    #[must_use]
    pub fn rows(&self) -> Vec<String> {
        self.px
            .chunks(self.w.max(1) as usize)
            .map(|r| String::from_utf8_lossy(r).into_owned())
            .collect()
    }

    /// 모든 픽셀 (행 우선).
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.px
    }

    /// 텍스트 행을 `(x, y)` 에 덮어쓴다. [`KEEP`] 은 건너뛴다.
    pub fn patch(&mut self, x: i32, y: i32, rows: &[Vec<u8>]) {
        for (dy, row) in rows.iter().enumerate() {
            for (dx, &c) in row.iter().enumerate() {
                if c != KEEP {
                    self.set(x + dx as i32, y + dy as i32, c);
                }
            }
        }
    }

    /// 꽉 찬 사각형.
    pub fn rect(&mut self, x: i32, y: i32, w: u32, h: u32, c: u8) {
        for yy in y..y + h as i32 {
            for xx in x..x + w as i32 {
                self.set(xx, yy, c);
            }
        }
    }

    /// 사각형 테두리 (1px).
    pub fn frame_rect(&mut self, x: i32, y: i32, w: u32, h: u32, c: u8) {
        if w == 0 || h == 0 {
            return;
        }
        let (x1, y1) = (x + w as i32 - 1, y + h as i32 - 1);
        self.line(x, y, x1, y, c);
        self.line(x, y1, x1, y1, c);
        self.line(x, y, x, y1, c);
        self.line(x1, y, x1, y1, c);
    }

    /// 브레젠험 직선. 양 끝점을 포함한다.
    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: u8) {
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = ((x1 - x0).signum(), (y1 - y0).signum());
        let (mut x, mut y, mut err) = (x0, y0, dx + dy);
        loop {
            self.set(x, y, c);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// 4방향으로 이어진 같은 색 영역을 `c` 로 칠한다.
    pub fn fill(&mut self, x: i32, y: i32, c: u8) {
        let Some(target) = self.get(x, y) else {
            return;
        };
        if target == c {
            return;
        }
        let mut stack = vec![(x, y)];
        while let Some((x, y)) = stack.pop() {
            if self.get(x, y) != Some(target) {
                continue;
            }
            self.set(x, y, c);
            stack.extend([(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]);
        }
    }

    /// 좌우 뒤집기.
    pub fn flip_h(&mut self) {
        let w = self.w as usize;
        for row in self.px.chunks_mut(w.max(1)) {
            row.reverse();
        }
    }

    /// 상하 뒤집기.
    pub fn flip_v(&mut self) {
        let w = self.w as usize;
        let rows: Vec<Vec<u8>> = self.px.chunks(w.max(1)).rev().map(<[u8]>::to_vec).collect();
        self.px = rows.concat();
    }

    /// 통째로 민다. 밖으로 나간 픽셀은 버리고 빈 자리는 투명이다.
    pub fn shift(&mut self, dx: i32, dy: i32) {
        let src = self.clone();
        self.px.fill(TRANSPARENT);
        for y in 0..self.h as i32 {
            for x in 0..self.w as i32 {
                if let Some(c) = src.get(x, y) {
                    self.set(x + dx, y + dy, c);
                }
            }
        }
    }

    /// 사각형 영역을 떼어 `(dx, dy)` 만큼 옮긴다. 떠난 자리는 투명, 투명 픽셀은 덮어쓰지 않는다.
    ///
    /// 팔다리를 한두 픽셀 움직이는 걷기 프레임용.
    pub fn move_region(&mut self, x: i32, y: i32, w: u32, h: u32, dx: i32, dy: i32) {
        let piece = self.crop(x, y, w, h);
        self.rect(x, y, w, h, TRANSPARENT);
        self.stamp(&piece, x + dx, y + dy, &[]);
    }

    /// 영역을 잘라 새 비트맵으로. 밖은 투명.
    #[must_use]
    pub fn crop(&self, x: i32, y: i32, w: u32, h: u32) -> Self {
        let mut out = Self::new(w, h);
        for yy in 0..h as i32 {
            for xx in 0..w as i32 {
                if let Some(c) = self.get(x + xx, y + yy) {
                    out.set(xx, yy, c);
                }
            }
        }
        out
    }

    /// `from` 을 전부 `to` 로.
    pub fn swap(&mut self, from: u8, to: u8) {
        for p in &mut self.px {
            if *p == from {
                *p = to;
            }
        }
    }

    /// 사각형 안에서만 `from` 을 `to` 로.
    pub fn swap_in(&mut self, from: u8, to: u8, x: i32, y: i32, w: u32, h: u32) {
        for yy in y..y + h as i32 {
            for xx in x..x + w as i32 {
                if self.get(xx, yy) == Some(from) {
                    self.set(xx, yy, to);
                }
            }
        }
    }

    /// 다른 비트맵을 `(x, y)` 에 올린다. 투명 픽셀은 밑그림을 남긴다.
    ///
    /// `soft` 는 반투명 색(빛·그림자) 문자들 — 이 색은 **빈 자리에만** 얹는다. 비트맵은 섞지 않고
    /// 덮어쓰므로, 반투명 광채가 몸 위에 얹히면 그 자리가 배경이 비치는 구멍이 된다.
    pub fn stamp(&mut self, src: &Self, x: i32, y: i32, soft: &[u8]) {
        for sy in 0..src.h as i32 {
            for sx in 0..src.w as i32 {
                if let Some(c) = src.get(sx, sy).filter(|&c| c != TRANSPARENT) {
                    self.put_over(x + sx, y + sy, c, soft);
                }
            }
        }
    }

    fn put_over(&mut self, x: i32, y: i32, c: u8, soft: &[u8]) {
        if soft.contains(&c) && self.get(x, y).is_some_and(|d| d != TRANSPARENT) {
            return;
        }
        self.set(x, y, c);
    }

    /// 돌려서 올린다 — RotSprite 방식. 부품을 Scale2x 로 세 번 키워(8배) 계단을 매끈한 선으로
    /// 바꾼 뒤, 돌린 자리의 각 픽셀 중심에서 8배 그림을 뽑는다. 1배에서 바로 돌리면 선이 끊기고
    /// 잡티가 생긴다.
    ///
    /// `pivot` 은 부품 안의 회전 중심(연속 좌표)이고, 돌리지 않은 `stamp` 와 같은 자리에 온다 —
    /// 칼을 손잡이 중심으로 돌리면 손잡이는 그대로 있다. `deg` 양수가 시계 방향(y 가 아래).
    pub fn stamp_rotated(
        &mut self,
        src: &Self,
        x: i32,
        y: i32,
        deg: f32,
        pivot: (f32, f32),
        soft: &[u8],
    ) {
        const K: f32 = 8.0;
        let big = src.scale2x().scale2x().scale2x();
        let (sin, cos) = deg.to_radians().sin_cos();
        let (cx, cy) = (x as f32 + pivot.0, y as f32 + pivot.1);
        // 돌린 부품이 덮을 수 있는 범위 — 피벗에서 가장 먼 모서리까지의 반지름.
        let r = [
            (0.0, 0.0),
            (src.w as f32, 0.0),
            (0.0, src.h as f32),
            (src.w as f32, src.h as f32),
        ]
        .iter()
        .map(|(px, py)| (px - pivot.0).hypot(py - pivot.1))
        .fold(0.0_f32, f32::max)
        .ceil() as i32
            + 1;
        for ty in (cy as i32 - r)..=(cy as i32 + r) {
            for tx in (cx as i32 - r)..=(cx as i32 + r) {
                // 목표 픽셀 중심을 거꾸로 돌려 부품 좌표로.
                let (dx, dy) = (tx as f32 + 0.5 - cx, ty as f32 + 0.5 - cy);
                let u = cos * dx + sin * dy + pivot.0;
                let v = -sin * dx + cos * dy + pivot.1;
                if u < 0.0 || v < 0.0 {
                    continue;
                }
                let c = big.get((u * K) as i32, (v * K) as i32);
                if let Some(c) = c.filter(|&c| c != TRANSPARENT) {
                    self.put_over(tx, ty, c, soft);
                }
            }
        }
    }

    /// Scale2x(EPX) — 두 배로 키우며 대각선 계단을 메운다. 팔레트 문자 그대로라 새 색이 안 생긴다.
    #[must_use]
    pub fn scale2x(&self) -> Self {
        let mut out = Self::new(self.w * 2, self.h * 2);
        for y in 0..self.h as i32 {
            for x in 0..self.w as i32 {
                let p = self.get(x, y).unwrap_or(TRANSPARENT);
                let at = |dx, dy| self.get(x + dx, y + dy).unwrap_or(TRANSPARENT);
                let (a, b, c, d) = (at(0, -1), at(1, 0), at(-1, 0), at(0, 1));
                let e0 = if c == a && c != d && a != b { a } else { p };
                let e1 = if a == b && a != c && b != d { b } else { p };
                let e2 = if d == c && d != b && c != a { c } else { p };
                let e3 = if b == d && b != a && d != c { d } else { p };
                let (ox, oy) = (x * 2, y * 2);
                out.set(ox, oy, e0);
                out.set(ox + 1, oy, e1);
                out.set(ox, oy + 1, e2);
                out.set(ox + 1, oy + 1, e3);
            }
        }
        out
    }

    /// `(x, y, w, h)` 에 내접하는 채운 타원. 픽셀 중심으로 안팎을 가린다.
    pub fn ellipse(&mut self, x: i32, y: i32, w: u32, h: u32, c: u8) {
        let (rx, ry) = (w as f32 / 2.0, h as f32 / 2.0);
        let (cx, cy) = (x as f32 + rx, y as f32 + ry);
        for py in y..y + h as i32 {
            for px in x..x + w as i32 {
                let dx = (px as f32 + 0.5 - cx) / rx;
                let dy = (py as f32 + 0.5 - cy) / ry;
                if dx * dx + dy * dy <= 1.0 {
                    self.set(px, py, c);
                }
            }
        }
    }

    /// 채운 다각형. 꼭짓점은 픽셀의 **중심** 좌표로 본다 (`(3, 4)` 는 3열 4행 픽셀의 한가운데) —
    /// 그래서 꼭짓점 픽셀이 칠해지고, 가장자리가 `line` 과 같은 자리에 온다.
    /// 픽셀 중심이 다각형 안(짝홀 규칙)이거나 가장자리 위면 칠한다.
    pub fn poly(&mut self, points: &[(i32, i32)], c: u8) {
        if points.len() < 3 {
            return;
        }
        let min_x = points.iter().map(|p| p.0).min().unwrap_or(0);
        let max_x = points.iter().map(|p| p.0).max().unwrap_or(0);
        let min_y = points.iter().map(|p| p.1).min().unwrap_or(0);
        let max_y = points.iter().map(|p| p.1).max().unwrap_or(0);
        let edges: Vec<((f32, f32), (f32, f32))> = points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .map(|(a, b)| ((a.0 as f32, a.1 as f32), (b.0 as f32, b.1 as f32)))
            .collect();
        for py in min_y..=max_y {
            for px in min_x..=max_x {
                let (x, y) = (px as f32, py as f32);
                let mut inside = false;
                let mut on_edge = false;
                for &((x0, y0), (x1, y1)) in &edges {
                    // 가장자리 위 — 선분과의 거리가 반 픽셀 이하이고 선분 범위 안.
                    let (dx, dy) = (x1 - x0, y1 - y0);
                    let len2 = dx * dx + dy * dy;
                    let t = if len2 > 0.0 {
                        ((x - x0) * dx + (y - y0) * dy) / len2
                    } else {
                        0.0
                    };
                    if (0.0..=1.0).contains(&t) {
                        let (qx, qy) = (x0 + t * dx - x, y0 + t * dy - y);
                        if qx.abs() <= 0.5 && qy.abs() <= 0.5 && (qx * qx + qy * qy) <= 0.3 {
                            on_edge = true;
                        }
                    }
                    if (y0 > y) != (y1 > y) && x < x0 + (y - y0) * dx / dy {
                        inside = !inside;
                    }
                }
                if inside || on_edge {
                    self.set(px, py, c);
                }
            }
        }
    }

    /// 왼쪽 절반을 오른쪽에 좌우 대칭으로 복사한다.
    pub fn mirror(&mut self) {
        let w = self.w as i32;
        for y in 0..self.h as i32 {
            for x in 0..w / 2 {
                if let Some(c) = self.get(x, y) {
                    self.set(w - 1 - x, y, c);
                }
            }
        }
    }

    /// 불투명 픽셀에 상하좌우로 맞닿은 투명 픽셀을 `c` 로 칠한다 — 외곽선.
    pub fn outline(&mut self, c: u8) {
        let src = self.clone();
        let opaque = |x: i32, y: i32| src.get(x, y).is_some_and(|p| p != TRANSPARENT);
        for y in 0..self.h as i32 {
            for x in 0..self.w as i32 {
                let empty = src.get(x, y) == Some(TRANSPARENT);
                if empty
                    && (opaque(x + 1, y)
                        || opaque(x - 1, y)
                        || opaque(x, y + 1)
                        || opaque(x, y - 1))
                {
                    self.set(x, y, c);
                }
            }
        }
    }

    /// 램프(밝은 것부터) 색 픽셀을 `steps` 단계 옮긴다. 사각형을 주면 그 안에서만.
    pub fn shade(&mut self, ramp: &[u8], steps: i32, region: Option<(i32, i32, u32, u32)>) {
        let (x0, y0, w, h) = region.unwrap_or((0, 0, self.w, self.h));
        for y in y0..y0 + h as i32 {
            for x in x0..x0 + w as i32 {
                let Some(i) = self
                    .get(x, y)
                    .and_then(|c| ramp.iter().position(|&k| k == c))
                else {
                    continue;
                };
                let j = (i as i32 + steps).clamp(0, ramp.len() as i32 - 1);
                self.set(x, y, ramp[j as usize]);
            }
        }
    }

    /// 왼쪽 위 빛으로 램프 색 덩어리에 명암을 깐다 — [`crate::doc::Op::Light`].
    ///
    /// 덩어리 마스크를 반경 `radius` 상자 흐림으로 세 번 흐려 둥근 높이로 삼고, 그 기울기로
    /// 면 방향을 얻는다. `radius` 가 0 이면 덩어리 두께(안쪽 가장 깊은 곳까지 거리)의 절반. 빛과 면 방향의 내적이 정면(평평한 면)보다 작으면 어둡게, 크면 밝게.
    /// 평평한 가운데는 원래 색을 지키고, 등진 쪽 가장자리가 초승달 모양으로 어두워진다.
    pub fn light(&mut self, ramp: &[u8], radius: u32, dark: u32, light: u32) {
        let (w, h) = (self.w as usize, self.h as usize);
        let idx = |c: u8| ramp.iter().position(|&k| k == c);
        let mask: Vec<f32> = self
            .px
            .iter()
            .map(|&c| if idx(c).is_some() { 1.0 } else { 0.0 })
            .collect();
        let radius = if radius == 0 {
            (depth(&mask, w, h) / 2).max(2)
        } else {
            radius
        };
        let mut height = mask.clone();
        for _ in 0..3 {
            height = box_blur(&height, w, h, radius as usize);
        }
        let at = |x: i32, y: i32| {
            if x < 0 || y < 0 || x as usize >= w || y as usize >= h {
                0.0
            } else {
                height[y as usize * w + x as usize]
            }
        };
        // 빛 방향 (화면 왼쪽 위, 보는 사람 쪽으로 기울어짐). 높이 기울기를 반경만큼 키워
        // 덩어리 가장자리 면이 거의 옆을 보게 한다.
        let l = {
            let v = [-1.0_f32, -1.0, 0.9];
            let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            [v[0] / n, v[1] / n, v[2] / n]
        };
        let flat = l[2];
        let scale = radius as f32 * 3.0;
        // 가장 등진 면과 가장 마주 본 면의 내적 — 단계 환산 기준.
        let side = (l[0] * l[0] + l[1] * l[1]).sqrt();
        let src = self.px.clone();
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                let c = src[y as usize * w + x as usize];
                let Some(i) = idx(c) else { continue };
                let gx = (at(x + 1, y) - at(x - 1, y)) * 0.5 * scale;
                let gy = (at(x, y + 1) - at(x, y - 1)) * 0.5 * scale;
                let n = (gx * gx + gy * gy + 1.0).sqrt();
                let dot = (-gx * l[0] - gy * l[1] + l[2]) / n;
                let delta = dot - flat;
                let steps = if delta < 0.0 {
                    (-delta / (side + flat) * dark as f32).round() as i32
                } else {
                    -((delta / (1.0 - flat)) * light as f32).round() as i32
                };
                let j = (i as i32 + steps).clamp(0, ramp.len() as i32 - 1);
                self.set(x, y, ramp[j as usize]);
            }
        }
    }

    /// 색 있는 외곽선 — 색 `c` 픽셀을 맞닿은 안쪽 색에 따라 `pairs` 의 결과색으로 바꾼다.
    ///
    /// 이웃은 위·왼쪽·오른쪽·아래 순서로 보고, 먼저 찾은 쌍을 쓴다. 맞는 쌍이 없으면 그대로 둔다.
    pub fn selout(&mut self, c: u8, pairs: &[(u8, u8)]) {
        let src = self.clone();
        for y in 0..self.h as i32 {
            for x in 0..self.w as i32 {
                if src.get(x, y) != Some(c) {
                    continue;
                }
                let found = [(0, -1), (-1, 0), (1, 0), (0, 1)]
                    .iter()
                    .find_map(|(dx, dy)| {
                        let near = src.get(x + dx, y + dy)?;
                        pairs
                            .iter()
                            .find(|(from, _)| *from == near)
                            .map(|(_, to)| *to)
                    });
                if let Some(to) = found {
                    self.set(x, y, to);
                }
            }
        }
    }
}

/// 마스크 안에서 바깥까지 가장 먼 거리 (4방향 걸음) — 덩어리 두께.
fn depth(mask: &[f32], w: usize, h: usize) -> u32 {
    let mut d: Vec<u32> = mask
        .iter()
        .map(|&m| if m > 0.0 { u32::MAX / 2 } else { 0 })
        .collect();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let up = if y > 0 { d[i - w] } else { 0 };
            let left = if x > 0 { d[i - 1] } else { 0 };
            d[i] = d[i].min(up + 1).min(left + 1);
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let i = y * w + x;
            let down = if y + 1 < h { d[i + w] } else { 0 };
            let right = if x + 1 < w { d[i + 1] } else { 0 };
            d[i] = d[i].min(down + 1).min(right + 1);
        }
    }
    d.into_iter().max().unwrap_or(0)
}

/// 가로·세로 상자 흐림 (반경 `r`). 그림 밖은 0 으로 본다.
fn box_blur(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let span = (2 * r + 1) as f32;
    let mut tmp = vec![0.0; src.len()];
    for y in 0..h {
        for x in 0..w {
            let lo = x.saturating_sub(r);
            let hi = (x + r).min(w - 1);
            tmp[y * w + x] = src[y * w + lo..=y * w + hi].iter().sum::<f32>() / span;
        }
    }
    let mut out = vec![0.0; src.len()];
    for y in 0..h {
        for x in 0..w {
            let lo = y.saturating_sub(r);
            let hi = (y + r).min(h - 1);
            out[y * w + x] = (lo..=hi).map(|yy| tmp[yy * w + x]).sum::<f32>() / span;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bm(rows: &[&str]) -> Bitmap {
        let mut b = Bitmap::new(rows[0].len() as u32, rows.len() as u32);
        let rows: Vec<Vec<u8>> = rows.iter().map(|r| r.bytes().collect()).collect();
        b.patch(0, 0, &rows);
        b
    }

    fn text(b: &Bitmap) -> Vec<String> {
        b.rows()
    }

    #[test]
    fn flips() {
        let mut b = bm(&["AB.", "C.."]);
        b.flip_h();
        assert_eq!(text(&b), [".BA", "..C"]);
        b.flip_v();
        assert_eq!(text(&b), ["..C", ".BA"]);
    }

    #[test]
    fn shift_drops_pixels_that_leave() {
        let mut b = bm(&["AB", "CD"]);
        b.shift(1, 0);
        assert_eq!(text(&b), [".A", ".C"]);
        b.shift(0, -1);
        assert_eq!(text(&b), [".C", ".."]);
    }

    #[test]
    fn patch_keeps_tilde_and_clips_outside() {
        let mut b = bm(&["AAA", "AAA"]);
        b.patch(1, 1, &[b"B~B".to_vec()]);
        assert_eq!(text(&b), ["AAA", "ABA"]);
    }

    #[test]
    fn line_includes_both_ends() {
        let mut b = Bitmap::new(4, 4);
        b.line(0, 0, 3, 3, b'X');
        assert_eq!(text(&b), ["X...", ".X..", "..X.", "...X"]);
        let mut b = Bitmap::new(3, 1);
        b.line(2, 0, 0, 0, b'X');
        assert_eq!(text(&b), ["XXX"]);
    }

    #[test]
    fn fill_stays_inside_the_region() {
        let mut b = bm(&["..A..", ".A.A.", "..A.."]);
        b.fill(2, 1, b'B');
        assert_eq!(text(&b), ["..A..", ".ABA.", "..A.."]);
        b.fill(0, 0, b'C');
        // 왼쪽 빈 영역만 — 오른쪽은 A 로 막혀 이어지지 않는다.
        assert_eq!(text(&b), ["CCA..", "CABA.", "CCA.."]);
    }

    #[test]
    fn outline_wraps_opaque_pixels() {
        let mut b = bm(&["...", ".A.", "..."]);
        b.outline(b'K');
        assert_eq!(text(&b), [".K.", "KAK", ".K."]);
    }

    #[test]
    fn selout_follows_the_inner_color() {
        let mut b = bm(&["KKK", "KAK", "KBK", "K.K"]);
        b.selout(b'K', &[(b'A', b'a'), (b'B', b'b')]);
        assert_eq!(text(&b), ["KaK", "aAa", "bBb", "K.K"]);
    }

    #[test]
    fn move_region_lifts_a_limb() {
        let mut b = bm(&["A.", "B.", "C."]);
        b.move_region(0, 2, 1, 1, 1, 0);
        assert_eq!(text(&b), ["A.", "B.", ".C"]);
    }

    #[test]
    fn stamp_respects_transparency() {
        let mut b = bm(&["AAA"]);
        b.stamp(&bm(&["B.B"]), 0, 0, &[]);
        assert_eq!(text(&b), ["BAB"]);
    }

    #[test]
    fn soft_colors_only_fill_empty_pixels() {
        let mut b = bm(&["A.."]);
        b.stamp(&bm(&["gg."]), 0, 0, b"g");
        assert_eq!(text(&b), ["Ag."], "반투명은 빈 자리에만");
    }

    #[test]
    fn swap_in_stays_inside_the_region() {
        let mut b = bm(&["AAA", "AAA"]);
        b.swap_in(b'A', b'B', 1, 0, 5, 1);
        assert_eq!(text(&b), ["ABB", "AAA"]);
    }

    #[test]
    fn ellipse_fills_its_box() {
        let mut b = Bitmap::new(5, 3);
        b.ellipse(0, 0, 5, 3, b'O');
        assert_eq!(text(&b), [".OOO.", "OOOOO", ".OOO."]);
    }

    #[test]
    fn poly_fills_a_triangle_including_its_corners() {
        let mut b = Bitmap::new(5, 3);
        b.poly(&[(0, 2), (4, 2), (2, 0)], b'T');
        assert_eq!(text(&b), ["..T..", ".TTT.", "TTTTT"]);
    }

    #[test]
    fn mirror_copies_the_left_half() {
        let mut b = bm(&["AB...", "C...."]);
        b.mirror();
        assert_eq!(text(&b), ["AB.BA", "C...C"]);
    }

    #[test]
    fn shade_walks_the_ramp_and_stops_at_its_end() {
        let mut b = bm(&["abcX"]);
        b.shade(b"abc", 1, None);
        assert_eq!(text(&b), ["bccX"]);
        b.shade(b"abc", -5, Some((0, 0, 1, 1)));
        assert_eq!(text(&b), ["accX"]);
    }

    #[test]
    fn light_darkens_the_lower_right_and_keeps_the_middle() {
        let mut b = Bitmap::new(16, 16);
        b.ellipse(0, 0, 16, 16, b'b');
        b.light(b"abcd", 0, 2, 1);
        assert_eq!(b.get(8, 8), Some(b'b'), "가운데는 원래 색");
        let dark = |c| matches!(c, Some(b'c' | b'd'));
        assert!(dark(b.get(13, 13)), "오른쪽 아래는 그늘: {:?}", b.rows());
        assert_eq!(b.get(3, 3), Some(b'a'), "왼쪽 위는 밝은 면: {:?}", b.rows());
    }

    #[test]
    fn rotation_by_right_angles_is_exact() {
        let bar = bm(&["A", "B", "C"]);
        let mut b = Bitmap::new(3, 3);
        b.stamp_rotated(&bar, 1, 0, 0.0, (0.5, 1.5), &[]);
        assert_eq!(text(&b), [".A.", ".B.", ".C."], "0도는 stamp 와 같다");
        let mut b = Bitmap::new(3, 3);
        b.stamp_rotated(&bar, 1, 0, 90.0, (0.5, 1.5), &[]);
        assert_eq!(text(&b), ["...", "CBA", "..."], "시계 방향 90도");
    }

    #[test]
    fn rotated_line_stays_one_pixel_wide_and_unbroken() {
        let mut bar = Bitmap::new(1, 12);
        bar.rect(0, 0, 1, 12, b'S');
        let mut b = Bitmap::new(16, 16);
        b.stamp_rotated(&bar, 8, 2, 30.0, (0.5, 6.0), &[]);
        let px: Vec<(i32, i32)> = (0..16)
            .flat_map(|y| (0..16).map(move |x| (x, y)))
            .filter(|&(x, y)| b.get(x, y) == Some(b'S'))
            .collect();
        assert!((11..=14).contains(&px.len()), "{:?}", b.rows());
        // 끊기지 않음 — 행마다 이어진다.
        let rows: std::collections::BTreeSet<i32> = px.iter().map(|p| p.1).collect();
        assert_eq!(
            rows.len() as i32,
            rows.last().unwrap() - rows.first().unwrap() + 1
        );
    }

    #[test]
    fn scale2x_rounds_diagonal_steps() {
        // 대각선 두 점 사이 모서리가 메워져 선이 이어진다.
        let b = bm(&["A.", ".A"]).scale2x();
        assert_eq!(text(&b), ["AA..", "AAA.", ".AAA", "..AA"]);
        // 모서리는 안팎 모두 둥글게.
        let b = bm(&["AA", "A."]).scale2x();
        assert_eq!(text(&b), [".AAA", "AAAA", "AAA.", "AA.."]);
    }

    #[test]
    fn frame_rect_draws_border_only() {
        let mut b = Bitmap::new(4, 3);
        b.frame_rect(0, 0, 4, 3, b'K');
        assert_eq!(text(&b), ["KKKK", "K..K", "KKKK"]);
    }
}
