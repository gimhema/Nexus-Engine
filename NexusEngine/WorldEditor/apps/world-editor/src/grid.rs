//! 월드 그리드 생성.
//!
//! 그리드 선을 얇은 쿼드로 만들어 오브젝트와 같은 파이프라인으로 그린다.
//! 별도 셰이더가 필요 없고, 3D 로 가도 같은 방식이 통한다.
//!
//! 간격은 줌에 따라 1·2·5 계열로 자동 전환된다 — 화면에 선이 너무 빽빽하거나
//! 너무 성기지 않도록.

use nexus_core::{Camera2d, Vec2};
use nexus_render::RenderCommand;

/// 그리드 선의 Z. 오브젝트(Z ≥ 0)보다 뒤에 오도록 음수를 쓴다.
const GRID_Z: f32 = -1.0;
const AXIS_Z: f32 = -0.5;

/// 화면에 이 개수 정도의 선이 보이도록 간격을 고른다.
const TARGET_LINE_COUNT: f32 = 24.0;

const MINOR_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 0.06];
const MAJOR_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 0.14];
const AXIS_X_COLOR: [f32; 4] = [0.85, 0.30, 0.35, 0.85];
const AXIS_Y_COLOR: [f32; 4] = [0.35, 0.75, 0.45, 0.85];

/// 현재 줌에 적합한 그리드 간격(m)을 고른다. 1·2·5 × 10ⁿ 계열 (0.1m, 0.2m … 100m, 200m …).
#[must_use]
pub(crate) fn pick_spacing(view_height: f32) -> f32 {
    let raw = view_height / TARGET_LINE_COUNT;
    let magnitude = 10.0_f32.powf(raw.log10().floor());
    let normalized = raw / magnitude;

    let step = if normalized <= 1.5 {
        1.0
    } else if normalized <= 3.5 {
        2.0
    } else if normalized <= 7.5 {
        5.0
    } else {
        10.0
    };

    step * magnitude
}

/// 보이는 영역을 덮는 그리드 선을 [`RenderCommand`] 로 만들어 `out` 에 넣는다.
pub(crate) fn build(camera: &Camera2d, out: &mut Vec<RenderCommand>) {
    let spacing = pick_spacing(camera.view_height);
    let (min, max) = camera.visible_bounds();

    // 선 두께는 화면상 굵기가 일정하도록 뷰 높이에 비례시킨다.
    let thin = camera.view_height * 0.0012;
    let thick = camera.view_height * 0.0022;

    let width = max.x - min.x;
    let height = max.y - min.y;

    // 세로선 (X 상수)
    let x0 = (min.x / spacing).floor() * spacing;
    let mut x = x0;
    while x <= max.x {
        let is_major = (x / (spacing * 5.0)).fract().abs() < 1e-3;
        let is_axis = x.abs() < spacing * 0.01;

        out.push(RenderCommand::DrawRect {
            center: Vec2::new(x, camera.center.y),
            size: Vec2::new(if is_axis { thick } else { thin }, height),
            z: if is_axis { AXIS_Z } else { GRID_Z },
            color: if is_axis {
                AXIS_Y_COLOR
            } else if is_major {
                MAJOR_COLOR
            } else {
                MINOR_COLOR
            },
        });
        x += spacing;
    }

    // 가로선 (Y 상수)
    let y0 = (min.y / spacing).floor() * spacing;
    let mut y = y0;
    while y <= max.y {
        let is_major = (y / (spacing * 5.0)).fract().abs() < 1e-3;
        let is_axis = y.abs() < spacing * 0.01;

        out.push(RenderCommand::DrawRect {
            center: Vec2::new(camera.center.x, y),
            size: Vec2::new(width, if is_axis { thick } else { thin }),
            z: if is_axis { AXIS_Z } else { GRID_Z },
            color: if is_axis {
                AXIS_X_COLOR
            } else if is_major {
                MAJOR_COLOR
            } else {
                MINOR_COLOR
            },
        });
        y += spacing;
    }
}

/// 속이 빈 사각형 테두리를 그린다. 존 경계(AABB) 표시에 쓴다.
pub(crate) fn build_outline(
    min: Vec2,
    max: Vec2,
    thickness: f32,
    z: f32,
    color: [f32; 4],
    out: &mut Vec<RenderCommand>,
) {
    let size = max - min;
    let center = (min + max) * 0.5;

    // 위 / 아래
    for y in [min.y, max.y] {
        out.push(RenderCommand::DrawRect {
            center: Vec2::new(center.x, y),
            size: Vec2::new(size.x + thickness, thickness),
            z,
            color,
        });
    }
    // 좌 / 우
    for x in [min.x, max.x] {
        out.push(RenderCommand::DrawRect {
            center: Vec2::new(x, center.y),
            size: Vec2::new(thickness, size.y + thickness),
            z,
            color,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spacing_follows_1_2_5_series() {
        // 간격은 항상 1·2·5 × 10ⁿ 형태여야 한다
        for vh in [37.0, 100.0, 250.0, 1000.0, 4321.0, 99_999.0] {
            let s = pick_spacing(vh);
            let mantissa = s / 10.0_f32.powf(s.log10().floor());
            assert!(
                [1.0, 2.0, 5.0].iter().any(|m| (m - mantissa).abs() < 1e-3),
                "view_height {vh} → spacing {s} (mantissa {mantissa})"
            );
        }
    }

    #[test]
    fn spacing_scales_with_zoom() {
        assert!(pick_spacing(10_000.0) > pick_spacing(100.0));
    }

    #[test]
    fn grid_covers_visible_area_without_exploding() {
        let camera = Camera2d {
            center: Vec2::new(1234.0, -567.0),
            view_height: 2000.0,
            viewport: (1920, 1080),
        };
        let mut out = Vec::new();
        build(&camera, &mut out);

        // 목표 선 개수 근처여야 한다 — 너무 적거나 수천 개가 나오면 간격 로직이 깨진 것
        assert!(
            (20..300).contains(&out.len()),
            "선 개수가 비정상: {}",
            out.len()
        );
    }

    #[test]
    fn grid_line_count_is_stable_across_zoom_levels() {
        // 어떤 줌에서도 선 개수가 폭발하지 않아야 한다
        for vh in [60.0, 600.0, 6_000.0, 60_000.0, 180_000.0] {
            let camera = Camera2d {
                center: Vec2::ZERO,
                view_height: vh,
                viewport: (1600, 900),
            };
            let mut out = Vec::new();
            build(&camera, &mut out);
            assert!(out.len() < 400, "view_height {vh} → {} 선", out.len());
        }
    }

    #[test]
    fn outline_emits_four_sides() {
        let mut out = Vec::new();
        build_outline(
            Vec2::new(-100.0, -100.0),
            Vec2::new(100.0, 100.0),
            2.0,
            0.0,
            [1.0; 4],
            &mut out,
        );
        assert_eq!(out.len(), 4);
    }
}
