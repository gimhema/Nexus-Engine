//! 도트 품질 검사 — 그림을 눈으로 보기 전에 기계적으로 잡을 수 있는 결함을 좌표와 함께 찾는다.
//!
//! 정답 판정이 아니라 **살펴볼 곳 목록**이다. 눈 하이라이트처럼 일부러 찍은 외톨이 픽셀도 걸린다.
//! 고칠 때마다 다시 돌려 숫자가 줄어드는지 보는 용도 — 원본끼리 비교할 기준이 된다.
//!
//! | 종류 | 무엇 |
//! |---|---|
//! | 고립 | 8방향 이웃에 같은 색이 없는 픽셀 — 잡티 |
//! | 계단 | 실루엣 외곽의 계단 길이가 앞뒤와 튀는 곳 (`2-1-2`, `1-3-1`) |
//! | 밴딩 | 1px 중간 단계가 두 색 사이에 길게 끼어 나란히 달리는 곳 |
//! | 줄무늬 | 같은 색 사이에 1px 다른 색 줄이 길게 — 결 없는 단조로운 줄 |
//! | 명도 | 맞닿은 두 색의 명도 차가 거의 없음 — 흑백에서 덩어리가 안 갈린다 |
//! | 팔레트 | 거의 같은 색 쌍, 안 쓰는 색, 몇 픽셀에만 쓰인 색 |
//! | 움직임 | 클립의 이웃 프레임끼리 바뀐 픽셀 비율 |

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::bitmap::Bitmap;
use crate::color::{Oklab, Palette, TRANSPARENT};
use crate::doc::{Document, ImageKind};
use crate::error::Result;
use crate::resolve::Resolver;

/// 고립 픽셀은 눈 하이라이트 같은 의도가 많아 결함 수에서 따로 센다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Orphan,
    Jaggy,
    Banding,
    Stripe,
}

impl Kind {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Orphan => "고립",
            Self::Jaggy => "계단",
            Self::Banding => "밴딩",
            Self::Stripe => "줄무늬",
        }
    }

    /// 미리보기 표시 색.
    #[must_use]
    pub fn mark(self) -> [u8; 4] {
        match self {
            Self::Orphan => [0xff, 0x30, 0xff, 0xff],
            Self::Jaggy => [0x20, 0xff, 0xff, 0xff],
            Self::Banding => [0xff, 0xd0, 0x20, 0xff],
            Self::Stripe => [0x40, 0xff, 0x60, 0xff],
        }
    }
}

/// 그림 안 한 곳. 밴딩·줄무늬는 `len` 픽셀 길이의 줄이다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub kind: Kind,
    pub x: i32,
    pub y: i32,
    /// 줄 방향과 길이 — `(dx, dy)` 로 `len` 번.
    pub dir: (i32, i32),
    pub len: u32,
    pub note: String,
}

impl Finding {
    /// 이 결함이 덮는 픽셀들.
    pub fn pixels(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        (0..self.len.max(1) as i32).map(|i| (self.x + self.dir.0 * i, self.y + self.dir.1 * i))
    }
}

/// 맞닿은 두 색의 명도 차가 작은 쌍.
#[derive(Clone, Debug)]
pub struct Contact {
    pub a: u8,
    pub b: u8,
    pub delta_l: f32,
    /// 맞닿은 변 수 (검사한 그림 전체 합).
    pub edges: u32,
    pub at: (String, i32, i32),
}

#[derive(Clone, Debug, Default)]
pub struct PaletteReport {
    pub colors: usize,
    /// 거의 같은 색 쌍과 OKLab 거리.
    pub near: Vec<(u8, u8, f32)>,
    /// 어떤 그림에도 안 쓰인 색.
    pub unused: Vec<u8>,
    /// 검사한 그림 전체에서 몇 픽셀에만 쓰인 색과 그 수.
    pub rare: Vec<(u8, u32)>,
    /// 이웃 단계의 명도 차가 너무 작은 램프 — `(램프, 밝은 쪽, 어두운 쪽, ΔL)`.
    /// 어두운 쪽이 더 밝으면 ΔL 이 음수다 (순서가 뒤집힘).
    pub flat_ramps: Vec<(String, u8, u8, f32)>,
}

/// 클립의 한 방향 행 — 이웃 프레임끼리 바뀐 픽셀 비율.
#[derive(Clone, Debug)]
pub struct Motion {
    pub clip: String,
    pub direction: usize,
    pub steps: Vec<Step>,
}

/// 이웃한 두 프레임의 차이.
///
/// 몸 전체가 1px 오르내리기만 해도 픽셀 절반이 바뀌므로, 통째 이동(±2px)을 맞춘 뒤에도
/// 남는 차이를 **모양 변화**로 따로 잰다 — 팔다리·망토가 실제로 움직였는지.
#[derive(Clone, Debug)]
pub struct Step {
    pub from: String,
    pub to: String,
    /// 그대로 비교해 바뀐 픽셀 수.
    pub changed: u32,
    /// 가장 잘 맞는 통째 이동.
    pub shift: (i32, i32),
    /// 그 이동을 맞춘 뒤 남은 차이 / 실루엣.
    pub shape: f32,
}

/// 디테일 밀도 — 같은 색으로 이어진 덩어리(4방향)의 크기. 검사한 그림들의 평균.
///
/// 참고 화풍(`style/sheet_style.png`)은 평균 1.7~2.4px 이다. 크면 넓고 매끈한 면이 많다는 뜻 —
/// 결·주름·반사로 면을 쪼개야 한다.
#[derive(Clone, Copy, Debug, Default)]
pub struct Density {
    /// 덩어리 평균 크기 (px).
    pub mean: f32,
    /// 프레임당 20px 이상 덩어리 수.
    pub large: f32,
    /// 프레임당 색 수.
    pub colors: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Report {
    /// 그림 이름 → 결함. 검사한 순서.
    pub images: Vec<(String, Vec<Finding>)>,
    pub density: Density,
    pub contacts: Vec<Contact>,
    pub palette: PaletteReport,
    pub motion: Vec<Motion>,
}

impl Report {
    #[must_use]
    pub fn count(&self, kind: Kind) -> usize {
        self.images
            .iter()
            .flat_map(|(_, f)| f)
            .filter(|f| f.kind == kind)
            .count()
    }
}

/// 큰 덩어리로 세는 크기.
const LARGE_CLUSTER: u32 = 20;
/// 참고 화풍의 덩어리 평균 — 이보다 크면 보고서에 "면이 넓다" 를 붙인다.
pub const DENSITY_TARGET: f32 = 3.0;

/// 같은 색으로 4방향 이어진 덩어리들의 크기 (불투명만).
fn clusters(bm: &Bitmap) -> Vec<u32> {
    let (w, h) = (bm.width() as i32, bm.height() as i32);
    let mut seen = vec![false; (w * h) as usize];
    let mut out = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let Some(c) = bm.get(x, y).filter(|&c| c != TRANSPARENT) else {
                continue;
            };
            if seen[(y * w + x) as usize] {
                continue;
            }
            let (mut n, mut stack) = (0, vec![(x, y)]);
            seen[(y * w + x) as usize] = true;
            while let Some((px, py)) = stack.pop() {
                n += 1;
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (qx, qy) = (px + dx, py + dy);
                    if bm.get(qx, qy) == Some(c) && !seen[(qy * w + qx) as usize] {
                        seen[(qy * w + qx) as usize] = true;
                        stack.push((qx, qy));
                    }
                }
            }
            out.push(n);
        }
    }
    out
}

/// 거의 같은 색으로 보는 OKLab 거리.
const NEAR_COLOR: f32 = 0.035;
/// 명도가 비슷하다고 보는 OKLab L 차.
const LOW_CONTRAST: f32 = 0.04;
/// 램프 이웃 단계의 최소 명도 차 — 이보다 작으면 흑백에서 단계가 안 보인다.
const RAMP_STEP_MIN: f32 = 0.04;
/// 드문 색 — 검사한 그림 전체에서 이 수 이하.
const RARE_PIXELS: u32 = 3;
/// 밴딩·줄무늬로 볼 최소 길이.
const RUN_MIN: u32 = 5;
/// 움직임이 적다고 볼 비율.
pub const MOTION_LOW: f32 = 0.03;

/// 검사할 그림 — 이름을 주지 않으면 시트 프레임, 아틀라스 그림, 둘 다 없으면 모든 frame.
#[must_use]
pub fn targets(doc: &Document) -> Vec<String> {
    let mut seen = HashSet::new();
    let names: Vec<String> = if let Some(sheet) = &doc.sheet {
        sheet
            .clips
            .iter()
            .flat_map(|c| c.rows.iter().flatten())
            .cloned()
            .collect()
    } else if let Some(atlas) = &doc.atlas {
        atlas.rows.iter().flatten().cloned().collect()
    } else {
        doc.images
            .iter()
            .filter(|i| i.kind == ImageKind::Frame)
            .map(|i| i.name.clone())
            .collect()
    };
    names
        .into_iter()
        .filter(|n| seen.insert(n.clone()))
        .collect()
}

pub fn lint(doc: &Document, names: &[String]) -> Result<Report> {
    let mut r = Resolver::new(doc);
    let lab: HashMap<u8, Oklab> = doc.palette.iter().map(|(k, c)| (k, c.oklab())).collect();

    let mut report = Report::default();
    let mut usage: BTreeMap<u8, u32> = BTreeMap::new();
    let mut contacts = ContactMap::new();
    let (mut sizes, mut large, mut colors) = (Vec::new(), 0u32, 0usize);
    for name in names {
        let bm = r.get(name)?;
        let found = clusters(&bm);
        large += found.iter().filter(|&&n| n >= LARGE_CLUSTER).count() as u32;
        sizes.extend(found);
        colors += bm
            .pixels()
            .iter()
            .filter(|&&c| c != TRANSPARENT)
            .collect::<HashSet<_>>()
            .len();
        let mut found = Vec::new();
        orphans(&bm, &lab, &mut found);
        jaggies(&bm, &mut found);
        runs(&bm, &lab, &mut found);
        report.images.push((name.clone(), found));

        for &c in bm.pixels() {
            if c != TRANSPARENT {
                *usage.entry(c).or_default() += 1;
            }
        }
        count_contacts(&bm, name, &mut contacts);
    }

    // 명도가 비슷한 이웃 — 맞닿은 변이 많은 순.
    let mut low: Vec<Contact> = contacts
        .into_iter()
        .filter_map(|((a, b), (edges, at))| {
            // 반투명 색(빛·그림자)은 바탕과 섞여 보이므로 명도 비교에서 뺀다.
            let solid = |k: u8| doc.palette.get(k).is_some_and(|c| c.0[3] == 0xFF);
            if !(solid(a) && solid(b)) {
                return None;
            }
            let delta_l = (lab.get(&a)?.l - lab.get(&b)?.l).abs();
            (delta_l < LOW_CONTRAST && edges >= 4).then_some(Contact {
                a,
                b,
                delta_l,
                edges,
                at,
            })
        })
        .collect();
    low.sort_by_key(|c| std::cmp::Reverse(c.edges));
    report.contacts = low;

    let n = names.len().max(1) as f32;
    report.density = Density {
        mean: sizes.iter().sum::<u32>() as f32 / sizes.len().max(1) as f32,
        large: large as f32 / n,
        colors: colors as f32 / n,
    };
    report.palette = palette_report(doc, &mut r, &usage, &lab)?;
    report.motion = motion(doc, &mut r)?;
    Ok(report)
}

fn opaque(bm: &Bitmap, x: i32, y: i32) -> bool {
    bm.get(x, y).is_some_and(|c| c != TRANSPARENT)
}

/// 상하좌우 어딘가가 투명(또는 그림 밖)인 불투명 픽셀 — 실루엣 가장자리.
fn is_edge(bm: &Bitmap, x: i32, y: i32) -> bool {
    [(1, 0), (-1, 0), (0, 1), (0, -1)]
        .iter()
        .any(|(dx, dy)| !opaque(bm, x + dx, y + dy))
}

const AROUND: [(i32, i32); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

/// 같은 색 이웃이 없고, 명도가 이웃들 사이에 끼어 있는 픽셀 — 축소·양자화가 남기는 잡티.
///
/// 이웃보다 확실히 밝거나 어두운 외톨이(눈 하이라이트, 리벳, 동공)는 일부러 찍은 강조라 뺀다.
fn orphans(bm: &Bitmap, lab: &HashMap<u8, Oklab>, out: &mut Vec<Finding>) {
    let l = |k: u8| lab.get(&k).map_or(0.0, |o| o.l);
    for y in 0..bm.height() as i32 {
        for x in 0..bm.width() as i32 {
            let Some(c) = bm.get(x, y).filter(|&c| c != TRANSPARENT) else {
                continue;
            };
            let near: Vec<u8> = AROUND
                .iter()
                .filter_map(|(dx, dy)| bm.get(x + dx, y + dy))
                .collect();
            if near.contains(&c) {
                continue;
            }
            // 실루엣 가장자리는 selout 이 한 점씩 색을 바꾸는 자리라 보지 않는다.
            // 투명 위 외톨이 점(별·불티)도 여기서 빠진다.
            if is_edge(bm, x, y) {
                continue;
            }
            let (lo, hi) = near.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &n| {
                (lo.min(l(n)), hi.max(l(n)))
            });
            if l(c) > hi || l(c) < lo {
                continue;
            }
            out.push(Finding {
                kind: Kind::Orphan,
                x,
                y,
                dir: (0, 0),
                len: 1,
                note: format!("'{}'", c as char),
            });
        }
    }
}

/// 실루엣의 네 방향 윤곽(왼·오른·위·아래 첫 불투명 픽셀)을 따라 계단 길이가 튀는 곳을 찾는다.
fn jaggies(bm: &Bitmap, out: &mut Vec<Finding>) {
    let (w, h) = (bm.width() as i32, bm.height() as i32);
    let mut seen = HashSet::new();
    // (윤곽 값 목록, 값·위치 → 그림 좌표)
    let mut side = |profile: Vec<Option<i32>>, to_xy: &dyn Fn(i32, i32) -> (i32, i32)| {
        for (pos, jump) in profile_jaggies(&profile) {
            let (x, y) = to_xy(pos, jump);
            if seen.insert((x, y)) {
                out.push(Finding {
                    kind: Kind::Jaggy,
                    x,
                    y,
                    dir: (0, 0),
                    len: 1,
                    note: String::new(),
                });
            }
        }
    };
    let first = |n: i32, f: &dyn Fn(i32) -> bool| (0..n).find(|&i| f(i));
    side(
        (0..h).map(|y| first(w, &|x| opaque(bm, x, y))).collect(),
        &|y, v| (v, y),
    );
    side(
        (0..h)
            .map(|y| first(w, &|x| opaque(bm, w - 1 - x, y)))
            .collect(),
        &|y, v| (w - 1 - v, y),
    );
    side(
        (0..w).map(|x| first(h, &|y| opaque(bm, x, y))).collect(),
        &|x, v| (x, v),
    );
    side(
        (0..w)
            .map(|x| first(h, &|y| opaque(bm, x, h - 1 - y)))
            .collect(),
        &|x, v| (x, h - 1 - v),
    );
}

/// 윤곽 `p[i]`(행 i 의 가장자리 깊이)에서 튀는 계단. `(행, 깊이)` 를 돌려준다.
///
/// 같은 값이 이어지는 구간이 한 계단이고, 구간 사이 값 차가 계단 폭이다. 계단마다 기울기
/// `폭 / 구간 길이` 를 두고, 같은 방향으로 꺾이는 세 계단에서 가운데만 앞뒤보다 1.5배 넘게
/// 튀면 (앞뒤끼리는 비슷한데) 계단이 고르지 않은 것이다. 6px 넘는 점프는 선이 아니라
/// 모양의 일부(팔·무기)라 거기서 끊는다.
fn profile_jaggies(p: &[Option<i32>]) -> Vec<(i32, i32)> {
    // (구간 시작 행, 구간 길이, 값)
    let mut segs: Vec<(i32, i32, i32)> = Vec::new();
    let mut out = Vec::new();
    let flush = |segs: &mut Vec<(i32, i32, i32)>, out: &mut Vec<(i32, i32)>| {
        // 처음·마지막 구간은 잘려 있을 수 있어 기울기 계산에서 뺀다.
        let steps: Vec<(f32, i32, i32, i32)> = segs
            .windows(2)
            .enumerate()
            .filter(|(i, _)| *i >= 1)
            .map(|(_, s)| {
                let jump = s[1].2 - s[0].2;
                (
                    jump.abs() as f32 / s[0].1 as f32,
                    jump.signum(),
                    s[1].0,
                    s[1].2,
                )
            })
            .collect();
        for t in steps.windows(3) {
            let (a, b, c) = (t[0], t[1], t[2]);
            if a.1 != b.1 || b.1 != c.1 {
                continue;
            }
            let (lo, hi) = (a.0.min(c.0), a.0.max(c.0));
            let similar = hi / lo <= 1.5;
            if similar && (b.0 > hi * 1.5 || b.0 < lo / 1.5) {
                out.push((b.2, b.3));
            }
        }
        segs.clear();
    };
    for (i, v) in p.iter().enumerate() {
        let i = i as i32;
        match (*v, segs.last_mut()) {
            (None, _) => flush(&mut segs, &mut out),
            (Some(v), Some(last)) if last.2 == v => last.1 += 1,
            (Some(v), Some(last)) if (last.2 - v).abs() > 6 => {
                flush(&mut segs, &mut out);
                segs.push((i, 1, v));
            }
            (Some(v), _) => segs.push((i, 1, v)),
        }
    }
    flush(&mut segs, &mut out);
    out
}

/// 1px 폭으로 길게 끼어 있는 색 줄 — 밴딩(명도가 양쪽 사이)과 줄무늬(양쪽이 같은 색).
fn runs(bm: &Bitmap, lab: &HashMap<u8, Oklab>, out: &mut Vec<Finding>) {
    let (w, h) = (bm.width() as i32, bm.height() as i32);
    // 이웃 방향 (dx, dy) 와 그에 수직인 줄 방향.
    for (side, along) in [((1, 0), (0, 1)), ((0, 1), (1, 0))] {
        let key = |x: i32, y: i32| -> Option<(u8, u8, u8)> {
            let a = bm.get(x, y).filter(|&c| c != TRANSPARENT)?;
            if is_edge(bm, x, y) {
                return None;
            }
            let (bx, by) = (x - side.0, y - side.1);
            let (cx, cy) = (x + side.0, y + side.1);
            let b = bm.get(bx, by)?;
            let c = bm.get(cx, cy)?;
            if b == a || c == a || is_edge(bm, bx, by) || is_edge(bm, cx, cy) {
                return None;
            }
            Some((a, b.min(c), b.max(c)))
        };
        let mut done = HashSet::new();
        for y in 0..h {
            for x in 0..w {
                if done.contains(&(x, y)) {
                    continue;
                }
                let Some(k) = key(x, y) else { continue };
                let mut len = 0;
                while key(x + along.0 * len, y + along.1 * len) == Some(k) {
                    done.insert((x + along.0 * len, y + along.1 * len));
                    len += 1;
                }
                if (len as u32) < RUN_MIN {
                    continue;
                }
                let (a, b, c) = k;
                let kind = if b == c {
                    Kind::Stripe
                } else {
                    let l = |k: u8| lab.get(&k).map_or(0.0, |o| o.l);
                    let between = (l(b) - l(a)) * (l(c) - l(a)) < 0.0;
                    if !between {
                        continue;
                    }
                    Kind::Banding
                };
                out.push(Finding {
                    kind,
                    x,
                    y,
                    dir: along,
                    len: len as u32,
                    note: if b == c {
                        format!("'{}' 사이 '{}'", b as char, a as char)
                    } else {
                        format!("'{}'·'{}' 사이 '{}'", b as char, c as char, a as char)
                    },
                });
            }
        }
    }
}

/// 색 쌍 → (맞닿은 변 수, 처음 본 자리).
type ContactMap = HashMap<(u8, u8), (u32, (String, i32, i32))>;

fn count_contacts(bm: &Bitmap, name: &str, acc: &mut ContactMap) {
    for y in 0..bm.height() as i32 {
        for x in 0..bm.width() as i32 {
            let Some(a) = bm.get(x, y).filter(|&c| c != TRANSPARENT) else {
                continue;
            };
            for (dx, dy) in [(1, 0), (0, 1)] {
                let Some(b) = bm
                    .get(x + dx, y + dy)
                    .filter(|&c| c != TRANSPARENT && c != a)
                else {
                    continue;
                };
                let e = acc
                    .entry((a.min(b), a.max(b)))
                    .or_insert_with(|| (0, (name.to_owned(), x, y)));
                e.0 += 1;
            }
        }
    }
}

fn palette_report(
    doc: &Document,
    r: &mut Resolver<'_>,
    usage: &BTreeMap<u8, u32>,
    lab: &HashMap<u8, Oklab>,
) -> Result<PaletteReport> {
    let keys: Vec<u8> = doc.palette.iter().map(|(k, _)| k).collect();
    let mut near = Vec::new();
    for (i, &a) in keys.iter().enumerate() {
        for &b in &keys[i + 1..] {
            let d = lab[&a].distance(lab[&b]);
            let alpha = |k: u8| doc.palette.get(k).map_or(0, |c| c.0[3]);
            if d < NEAR_COLOR && alpha(a) == alpha(b) {
                near.push((a, b, d));
            }
        }
    }
    near.sort_by(|x, y| x.2.total_cmp(&y.2));

    // 안 쓰는 색은 부품까지 모든 그림을 본다 — 시트에 없는 겹 프레임에만 쓰인 색도 있다.
    let mut used = HashSet::new();
    for img in &doc.images {
        used.extend(r.get(&img.name)?.pixels().iter().copied());
    }
    let unused = keys.iter().copied().filter(|k| !used.contains(k)).collect();
    let rare = usage
        .iter()
        .filter(|(_, n)| **n <= RARE_PIXELS)
        .map(|(k, n)| (*k, *n))
        .collect();
    let mut flat_ramps = Vec::new();
    for ramp in &doc.ramps {
        for w in ramp.keys.windows(2) {
            let d = lab[&w[0]].l - lab[&w[1]].l;
            if d < RAMP_STEP_MIN {
                flat_ramps.push((ramp.name.clone(), w[0], w[1], d));
            }
        }
    }
    Ok(PaletteReport {
        colors: keys.len(),
        near,
        unused,
        rare,
        flat_ramps,
    })
}

fn motion(doc: &Document, r: &mut Resolver<'_>) -> Result<Vec<Motion>> {
    let Some(sheet) = &doc.sheet else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for clip in &sheet.clips {
        if clip.frames() < 2 {
            continue;
        }
        for (d, row) in clip.rows.iter().enumerate() {
            let mut pairs: Vec<(&String, &String)> = row.iter().zip(row.iter().skip(1)).collect();
            if clip.looping && row.len() > 2 {
                pairs.push((&row[row.len() - 1], &row[0]));
            }
            let mut steps = Vec::new();
            for (a, b) in pairs {
                let (ba, bb) = (r.get(a)?, r.get(b)?);
                let body = ba.pixels().iter().filter(|&&p| p != TRANSPARENT).count() as u32;
                let changed = diff(&ba, &bb, 0, 0);
                let mut best = (changed, (0, 0));
                for dy in -2..=2 {
                    for dx in -2..=2 {
                        let n = diff(&ba, &bb, dx, dy);
                        if n < best.0 {
                            best = (n, (dx, dy));
                        }
                    }
                }
                steps.push(Step {
                    from: a.clone(),
                    to: b.clone(),
                    changed,
                    shift: best.1,
                    shape: best.0 as f32 / body.max(1) as f32,
                });
            }
            out.push(Motion {
                clip: clip.state.clone(),
                direction: d,
                steps,
            });
        }
    }
    Ok(out)
}

/// `a` 를 `(dx, dy)` 만큼 옮겨 `b` 와 겹쳤을 때 다른 픽셀 수.
fn diff(a: &Bitmap, b: &Bitmap, dx: i32, dy: i32) -> u32 {
    let mut n = 0;
    for y in 0..b.height() as i32 {
        for x in 0..b.width() as i32 {
            let pa = a.get(x - dx, y - dy).unwrap_or(TRANSPARENT);
            if Some(pa) != b.get(x, y) {
                n += 1;
            }
        }
    }
    n
}

/// 사람이 읽는 보고서. `limit` 는 그림·종류마다 보여 줄 좌표 수.
#[must_use]
///
/// `detail` 이 거짓이면 그림마다 종류별 개수 한 줄 — 프레임이 많은 시트를 훑을 때.
pub fn format(report: &Report, palette: &Palette, limit: usize, detail: bool) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let kinds = [Kind::Jaggy, Kind::Banding, Kind::Stripe, Kind::Orphan];

    for (name, found) in &report.images {
        if found.is_empty() {
            continue;
        }
        if !detail {
            let counts: Vec<String> = kinds
                .iter()
                .filter_map(|&k| {
                    let n = found.iter().filter(|f| f.kind == k).count();
                    (n > 0).then(|| format!("{} {n}", k.label()))
                })
                .collect();
            let _ = writeln!(s, "{name:<24} {}", counts.join(" · "));
            continue;
        }
        let _ = writeln!(s, "{name}");
        for kind in kinds {
            let list: Vec<&Finding> = found.iter().filter(|f| f.kind == kind).collect();
            if list.is_empty() {
                continue;
            }
            let shown: Vec<String> = list
                .iter()
                .take(limit)
                .map(|f| {
                    let pos = format!("({},{})", f.x, f.y);
                    let len = if f.len > 1 {
                        let d = if f.dir.0 != 0 { "→" } else { "↓" };
                        format!("{d}{}", f.len)
                    } else {
                        String::new()
                    };
                    let note = if f.note.is_empty() {
                        String::new()
                    } else {
                        format!(" {}", f.note)
                    };
                    format!("{pos}{len}{note}")
                })
                .collect();
            let more = if list.len() > limit {
                format!(" 외 {}", list.len() - limit)
            } else {
                String::new()
            };
            let _ = writeln!(
                s,
                "  {} {:>3}: {}{more}",
                kind.label(),
                list.len(),
                shown.join("  ")
            );
        }
    }

    if !report.contacts.is_empty() {
        let _ = writeln!(
            s,
            "\n명도가 비슷한 이웃 (ΔL < {LOW_CONTRAST}, 맞닿은 변 많은 순)"
        );
        for c in report.contacts.iter().take(limit) {
            let _ = writeln!(
                s,
                "  '{}' {} · '{}' {}  ΔL {:.3}  변 {}  예: {} ({},{})",
                c.a as char,
                hex(palette, c.a),
                c.b as char,
                hex(palette, c.b),
                c.delta_l,
                c.edges,
                c.at.0,
                c.at.1,
                c.at.2
            );
        }
    }

    let p = &report.palette;
    let _ = writeln!(s, "\n팔레트 {}색", p.colors);
    if !p.near.is_empty() {
        let pairs: Vec<String> = p
            .near
            .iter()
            .take(limit * 2)
            .map(|(a, b, d)| format!("'{}''{}' {d:.3}", *a as char, *b as char))
            .collect();
        let more = if p.near.len() > limit * 2 {
            format!(" 외 {}", p.near.len() - limit * 2)
        } else {
            String::new()
        };
        let _ = writeln!(
            s,
            "  거의 같은 색 {}쌍 (OKLab < {NEAR_COLOR}): {}{more}",
            p.near.len(),
            pairs.join("  ")
        );
    }
    if !p.unused.is_empty() {
        let keys: String = p.unused.iter().map(|&k| k as char).collect();
        let _ = writeln!(s, "  안 쓰는 색 {}: {keys}", p.unused.len());
    }
    if !p.rare.is_empty() {
        let list: Vec<String> = p
            .rare
            .iter()
            .map(|(k, n)| format!("'{}'×{n}", *k as char))
            .collect();
        let _ = writeln!(
            s,
            "  {RARE_PIXELS}픽셀 이하로 쓰인 색 {}: {}",
            p.rare.len(),
            list.join(" ")
        );
    }

    for (name, a, b, d) in &p.flat_ramps {
        let _ = writeln!(
            s,
            "  램프 `{name}` '{}'→'{}' ΔL {d:+.3} — {}",
            *a as char,
            *b as char,
            if *d < 0.0 {
                "어두운 쪽이 더 밝음 (순서)"
            } else {
                "단계가 너무 가까움"
            }
        );
    }

    if !report.motion.is_empty() {
        let _ = writeln!(
            s,
            "\n움직임 — 이웃 프레임의 모양 변화 (통째 이동을 맞춘 뒤 남은 차이 / 실루엣)"
        );
        for m in &report.motion {
            let parts: Vec<String> = m
                .steps
                .iter()
                .map(|st| {
                    let warn = if st.changed == 0 {
                        " 같음!"
                    } else if st.shape < MOTION_LOW {
                        " 적음"
                    } else {
                        ""
                    };
                    let shift = if st.shift == (0, 0) {
                        String::new()
                    } else {
                        format!("({:+},{:+})", st.shift.0, st.shift.1)
                    };
                    format!("{:.1}%{shift}{warn}", st.shape * 100.0)
                })
                .collect();
            let _ = writeln!(
                s,
                "  {} 방향 {}: {}",
                m.clip,
                m.direction,
                parts.join(" → ")
            );
        }
    }

    let d = report.density;
    let _ = writeln!(
        s,
        "\n디테일 밀도 — 덩어리 평균 {:.1}px (참고 화풍 1.7~2.4){} · 20px+ 덩어리 {:.1}개/프레임 · 색 {:.0}/프레임",
        d.mean,
        if d.mean > DENSITY_TARGET {
            " 면이 넓다"
        } else {
            ""
        },
        d.large,
        d.colors
    );
    let total = |k| report.count(k);
    let _ = writeln!(
        s,
        "\n합계 — 계단 {} · 밴딩 {} · 줄무늬 {} · 고립 {} · 명도 비슷 {}쌍 · 거의 같은 색 {}쌍",
        total(Kind::Jaggy),
        total(Kind::Banding),
        total(Kind::Stripe),
        total(Kind::Orphan),
        report.contacts.len(),
        p.near.len()
    );
    s
}

fn hex(p: &Palette, k: u8) -> String {
    p.get(k).map(|c| c.to_string()).unwrap_or_default()
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

    #[test]
    fn orphan_is_a_lone_color_among_others() {
        let mut out = Vec::new();
        let lab: HashMap<u8, Oklab> = [
            (b'A', crate::color::Rgba([0, 0, 0, 255]).oklab()),
            (b'B', crate::color::Rgba([90, 90, 90, 255]).oklab()),
            (b'C', crate::color::Rgba([200, 200, 200, 255]).oklab()),
            (b'W', crate::color::Rgba([255, 255, 255, 255]).oklab()),
        ]
        .into_iter()
        .collect();
        // B 는 A·C 사이 명도 — 잡티. W 는 이웃보다 밝은 하이라이트라 뺀다.
        orphans(
            &bm(&["AACCCC", "ABCCWC", "AACCCC", "AACC..", "....C."]),
            &lab,
            &mut out,
        );
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].x, out[0].y), (1, 1));
    }

    #[test]
    fn clusters_count_same_colour_regions() {
        let mut c = clusters(&bm(&["AAB", "A.B", "CCB"]));
        c.sort_unstable();
        assert_eq!(c, [2, 3, 3]);
    }

    #[test]
    fn regular_staircase_is_clean_and_a_bump_is_not() {
        // 2-2-2-2 계단 — 고르다.
        let even: Vec<Option<i32>> = [0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5]
            .iter()
            .map(|&v| Some(v))
            .collect();
        assert!(profile_jaggies(&even).is_empty());
        // 2-2-1-2-2 — 가운데 계단이 튄다.
        let bump: Vec<Option<i32>> = [0, 0, 1, 1, 2, 3, 3, 4, 4, 5, 5]
            .iter()
            .map(|&v| Some(v))
            .collect();
        assert_eq!(profile_jaggies(&bump).len(), 1);
    }

    #[test]
    fn stripes_and_banding_are_long_one_pixel_runs() {
        let lab: HashMap<u8, Oklab> = [
            (b'A', crate::color::Rgba([250, 250, 250, 255]).oklab()),
            (b'B', crate::color::Rgba([150, 150, 150, 255]).oklab()),
            (b'C', crate::color::Rgba([50, 50, 50, 255]).oklab()),
        ]
        .into_iter()
        .collect();
        let rows = [
            "AAAAAAA", "AAAAAAA", "AABACAA", "AABACAA", "AABACAA", "AABACAA", "AABACAA", "AAAAAAA",
            "AAAAAAA",
        ];
        let mut out = Vec::new();
        runs(&bm(&rows), &lab, &mut out);
        assert!(
            out.iter().any(|f| f.kind == Kind::Stripe && f.len == 5),
            "{out:?}"
        );

        let rows = [
            "CCCCCCC", "CCCCCCC", "CCABCCC", "CCABCCC", "CCABCCC", "CCABCCC", "CCABCCC", "CCCCCCC",
            "CCCCCCC",
        ];
        let mut out = Vec::new();
        runs(&bm(&rows), &lab, &mut out);
        assert!(out.iter().any(|f| f.kind == Kind::Banding), "{out:?}");
    }
}
