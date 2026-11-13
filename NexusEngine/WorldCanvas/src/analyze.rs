//! 참고 그림 분석 — 색 수, 재질별 램프, 램프 한 단계의 명도 차와 색상 회전.
//!
//! 감으로 팔레트를 고르는 대신 참고 화풍을 **수치**로 옮기기 위한 것이다. 그림의 색을 OKLab 에서
//! `k` 개로 묶고(가중 k-평균), 색상각이 가까운 것끼리 한 재질로 모아 밝은 것부터 늘어놓는다.
//! 결과는 팔레트 줄과 `ramp` 줄 — 그대로 베끼지 말고 단계 폭·색상 회전 같은 **규칙**을 가져온다.

use std::collections::HashMap;

use crate::color::{Oklab, Rgba};
use crate::export::Rgba8;

#[derive(Clone, Debug)]
pub struct Options {
    /// 분석할 사각형 `(x, y, 가로, 세로)`. 없으면 그림 전체. 그 왼쪽 위 픽셀이 불투명하면 배경색으로 본다.
    pub crop: Option<(u32, u32, u32, u32)>,
    /// 묶을 색 수.
    pub colors: usize,
}

/// 재질 하나 — 색상각이 가까운 색들, 밝은 것부터.
#[derive(Clone, Debug)]
pub struct Family {
    pub colors: Vec<(Rgba, u32)>,
    /// 이웃 단계의 평균 명도 차 (OKLab L).
    pub step: f32,
    /// 어두워질수록 색상각이 도는 평균 각도 (도, 부호 = 방향).
    pub hue_turn: f32,
}

#[derive(Clone, Debug)]
pub struct Analysis {
    pub pixels: u32,
    pub unique: usize,
    pub families: Vec<Family>,
    /// 무채색(채도 낮음) 묶음.
    pub neutrals: Vec<(Rgba, u32)>,
}

/// 무채색으로 볼 OKLCH 채도.
const NEUTRAL_CHROMA: f32 = 0.025;
/// 한 재질로 묶을 색상각 간격.
const HUE_GAP: f32 = 18.0;

#[must_use]
pub fn analyze(img: &Rgba8, opt: &Options) -> Analysis {
    let (x0, y0, w, h) = opt.crop.unwrap_or((0, 0, img.width, img.height));
    // 알파 없는 그림(참고 시트 캡처)은 왼쪽 위 모서리 색을 배경으로 보고 뺀다.
    let corner = img.at(x0.min(img.width - 1), y0.min(img.height - 1));
    let bg = (corner[3] == 0xFF).then(|| Rgba(corner).oklab());
    let mut counts: HashMap<[u8; 3], u32> = HashMap::new();
    for y in y0..(y0 + h).min(img.height) {
        for x in x0..(x0 + w).min(img.width) {
            let p = img.at(x, y);
            let is_bg = bg.is_some_and(|b| Rgba(p).oklab().distance(b) < 0.03);
            if p[3] >= 128 && !is_bg {
                *counts.entry([p[0], p[1], p[2]]).or_default() += 1;
            }
        }
    }
    let pixels = counts.values().sum();
    let unique = counts.len();
    let mut points: Vec<(Oklab, u32, Rgba)> = counts
        .into_iter()
        .map(|(c, n)| {
            let rgba = Rgba([c[0], c[1], c[2], 0xFF]);
            (rgba.oklab(), n, rgba)
        })
        .collect();
    // 결과가 실행마다 같도록.
    points.sort_by_key(|p| (std::cmp::Reverse(p.1), p.2));

    let clusters = kmeans(&points, opt.colors.max(1));
    let mut neutrals = Vec::new();
    let mut chromatic: Vec<(Oklab, Rgba, u32)> = Vec::new();
    for (lab, n) in clusters {
        let rgba = lab.to_rgba();
        if lab.lch().1 < NEUTRAL_CHROMA {
            neutrals.push((rgba, n));
        } else {
            chromatic.push((lab, rgba, n));
        }
    }
    neutrals.sort_by(|a, b| b.0.oklab().l.total_cmp(&a.0.oklab().l));

    // 색상각 순으로 늘어놓고 간격이 큰 곳에서 끊는다 (원이므로 가장 큰 간격에서 시작).
    chromatic.sort_by(|a, b| a.0.lch().2.total_cmp(&b.0.lch().2));
    let hues: Vec<f32> = chromatic.iter().map(|c| c.0.lch().2).collect();
    let mut families: Vec<Vec<(Oklab, Rgba, u32)>> = Vec::new();
    if !chromatic.is_empty() {
        let n = hues.len();
        let gap = |i: usize| (hues[(i + 1) % n] - hues[i]).rem_euclid(360.0);
        let start = (0..n)
            .max_by(|&a, &b| gap(a).total_cmp(&gap(b)))
            .unwrap_or(0)
            + 1;
        let mut cur = Vec::new();
        for k in 0..n {
            let i = (start + k) % n;
            if !cur.is_empty() && gap((i + n - 1) % n) > HUE_GAP {
                families.push(std::mem::take(&mut cur));
            }
            cur.push(chromatic[i]);
        }
        families.push(cur);
    }
    let families = families
        .into_iter()
        .map(|mut f| {
            f.sort_by(|a, b| b.0.l.total_cmp(&a.0.l));
            let pairs: Vec<(f32, f32)> = f
                .windows(2)
                .map(|w| {
                    let dl = w[0].0.l - w[1].0.l;
                    let dh = (w[1].0.lch().2 - w[0].0.lch().2 + 540.0).rem_euclid(360.0) - 180.0;
                    (dl, dh)
                })
                .collect();
            let avg = |v: Vec<f32>| {
                if v.is_empty() {
                    0.0
                } else {
                    v.iter().sum::<f32>() / v.len() as f32
                }
            };
            Family {
                step: avg(pairs.iter().map(|p| p.0).collect()),
                hue_turn: avg(pairs.iter().map(|p| p.1).collect()),
                colors: f.into_iter().map(|c| (c.1, c.2)).collect(),
            }
        })
        .collect();
    Analysis {
        pixels,
        unique,
        families,
        neutrals,
    }
}

/// 가중 k-평균. 시작점은 가장 많이 쓰인 색에서 출발해 매번 가장 먼 색을 고른다 (결정적).
fn kmeans(points: &[(Oklab, u32, Rgba)], k: usize) -> Vec<(Oklab, u32)> {
    if points.is_empty() {
        return Vec::new();
    }
    let k = k.min(points.len());
    let mut centers = vec![points[0].0];
    while centers.len() < k {
        let far = points
            .iter()
            .max_by(|a, b| {
                let d = |p: &Oklab| {
                    centers
                        .iter()
                        .map(|c| c.distance(*p))
                        .fold(f32::MAX, f32::min)
                };
                // 많이 쓰인 색을 약간 우선 — 한두 픽셀 잡색이 중심을 차지하지 않게.
                (d(&a.0) * (a.1 as f32).ln_1p()).total_cmp(&(d(&b.0) * (b.1 as f32).ln_1p()))
            })
            .map_or(points[0].0, |p| p.0);
        centers.push(far);
    }
    let mut weight = vec![0; k];
    for _ in 0..30 {
        let mut sum = vec![(0.0_f32, 0.0_f32, 0.0_f32); k];
        weight = vec![0; k];
        for (p, n, _) in points {
            let i = nearest(&centers, *p);
            let w = *n as f32;
            sum[i].0 += p.l * w;
            sum[i].1 += p.a * w;
            sum[i].2 += p.b * w;
            weight[i] += n;
        }
        for (i, c) in centers.iter_mut().enumerate() {
            if weight[i] > 0 {
                let w = weight[i] as f32;
                *c = Oklab {
                    l: sum[i].0 / w,
                    a: sum[i].1 / w,
                    b: sum[i].2 / w,
                };
            }
        }
    }
    centers
        .into_iter()
        .zip(weight)
        .filter(|(_, n)| *n > 0)
        .collect()
}

fn nearest(centers: &[Oklab], p: Oklab) -> usize {
    centers
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.distance(p).total_cmp(&b.1.distance(p)))
        .map_or(0, |(i, _)| i)
}

/// 보고서 + 바로 붙여 넣을 수 있는 팔레트 줄.
#[must_use]
pub fn format(a: &Analysis) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = writeln!(
        s,
        "불투명 픽셀 {}, 고유 색 {} → {}색으로 묶음",
        a.pixels,
        a.unique,
        a.neutrals.len() + a.families.iter().map(|f| f.colors.len()).sum::<usize>()
    );
    let _ = writeln!(s, "\n재질 (색상각이 가까운 묶음, 밝은 것부터)");
    for (i, f) in a.families.iter().enumerate() {
        let list: Vec<String> = f.colors.iter().map(|(c, _)| c.to_string()).collect();
        let hue = f.colors.first().map_or(0.0, |c| c.0.oklab().lch().2);
        let _ = writeln!(
            s,
            "  {i:>2}. 색상 {hue:>3.0}°  {}단계  단계당 ΔL {:.3}  색상 회전 {:+.1}°/단계  {}",
            f.colors.len(),
            f.step,
            f.hue_turn,
            list.join(" ")
        );
    }
    if !a.neutrals.is_empty() {
        let list: Vec<String> = a.neutrals.iter().map(|(c, _)| c.to_string()).collect();
        let _ = writeln!(s, "  무채색 {}단계  {}", a.neutrals.len(), list.join(" "));
    }
    let multi: Vec<&Family> = a.families.iter().filter(|f| f.colors.len() >= 3).collect();
    if !multi.is_empty() {
        let n = multi.len() as f32;
        let _ = writeln!(
            s,
            "\n3단계 이상 재질 평균 — 단계당 ΔL {:.3}, 색상 회전 {:+.1}°/단계, 단계 수 {:.1}",
            multi.iter().map(|f| f.step).sum::<f32>() / n,
            multi.iter().map(|f| f.hue_turn).sum::<f32>() / n,
            multi.iter().map(|f| f.colors.len() as f32).sum::<f32>() / n,
        );
    }

    // 붙여 넣을 팔레트 — 재질마다 대문자부터 차례로 문자를 준다.
    let mut keys = (b'A'..=b'Z').chain(b'a'..=b'z').chain(b'0'..=b'9');
    let _ = writeln!(s, "\n// 팔레트 초안");
    let mut ramps = Vec::new();
    let groups = a
        .families
        .iter()
        .map(|f| f.colors.as_slice())
        .chain(std::iter::once(a.neutrals.as_slice()));
    for (i, colors) in groups.enumerate() {
        let mut ks = String::new();
        for (c, _) in colors {
            let Some(k) = keys.next() else { break };
            let _ = writeln!(s, "    {} {c}", k as char);
            ks.push(k as char);
        }
        if ks.len() >= 2 {
            let name = if i == a.families.len() {
                String::from("neutral")
            } else {
                format!("m{i}")
            };
            ramps.push(format!("    ramp {name} {ks}"));
        }
    }
    for r in ramps {
        let _ = writeln!(s, "{r}");
    }
    s
}
