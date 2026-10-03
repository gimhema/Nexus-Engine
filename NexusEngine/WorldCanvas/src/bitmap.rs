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
        self.stamp(&piece, x + dx, y + dy);
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
    pub fn stamp(&mut self, src: &Self, x: i32, y: i32) {
        for sy in 0..src.h as i32 {
            for sx in 0..src.w as i32 {
                if let Some(c) = src.get(sx, sy).filter(|&c| c != TRANSPARENT) {
                    self.set(x + sx, y + sy, c);
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
    fn move_region_lifts_a_limb() {
        let mut b = bm(&["A.", "B.", "C."]);
        b.move_region(0, 2, 1, 1, 1, 0);
        assert_eq!(text(&b), ["A.", "B.", ".C"]);
    }

    #[test]
    fn stamp_respects_transparency() {
        let mut b = bm(&["AAA"]);
        b.stamp(&bm(&["B.B"]), 0, 0);
        assert_eq!(text(&b), ["BAB"]);
    }

    #[test]
    fn swap_in_stays_inside_the_region() {
        let mut b = bm(&["AAA", "AAA"]);
        b.swap_in(b'A', b'B', 1, 0, 5, 1);
        assert_eq!(text(&b), ["ABB", "AAA"]);
    }

    #[test]
    fn mirror_copies_the_left_half() {
        let mut b = bm(&["AB...", "C...."]);
        b.mirror();
        assert_eq!(text(&b), ["AB.BA", "C...C"]);
    }

    #[test]
    fn frame_rect_draws_border_only() {
        let mut b = Bitmap::new(4, 3);
        b.frame_rect(0, 0, 4, 3, b'K');
        assert_eq!(text(&b), ["KKKK", "K..K", "KKKK"]);
    }
}
