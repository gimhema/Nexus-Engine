//! 월드 그리드 생성.
//!
//! 그리드 선을 얇은 쿼드로 만들어 오브젝트와 같은 파이프라인으로 그린다.
//! 별도 셰이더가 필요 없고, 3D 로 가도 같은 방식이 통한다.
//!
//! 간격은 줌에 따라 1·2·5 계열로 자동 전환된다 — 화면에 선이 너무 빽빽하거나
//! 너무 성기지 않도록.

use nexus_core::{Camera2d, Vec2};
use nexus_render::{DEPTH_LAYER, RenderCommand};

/// 그리드 선의 겹침 순서. 지면에 깔리므로 월드 Z 는 0 이고, 깊이 편향만 음수로 둔다.
///
/// 월드 Z 를 음수로 두면 쿼터뷰에서 선이 화면 아래로 밀려 오브젝트와 어긋난다.
const GRID_BIAS: f32 = -2.0 * DEPTH_LAYER;
const AXIS_BIAS: f32 = -DEPTH_LAYER;

/// 화면에 이 개수 정도의 선이 보이도록 간격을 고른다.
const TARGET_LINE_COUNT: f32 = 24.0;

/// 그리드 선 굵기 (화면 픽셀). 1px 미만이면 픽셀 경계에서 사라진다 — `crate::THIN_LINE_PX` 참고.
const THIN_LINE_PX: f32 = crate::THIN_LINE_PX;
/// 축(X=0 / Y=0) 선 굵기 (화면 픽셀). 일반 선보다 굵게 해서 눈에 띄게 한다.
const AXIS_LINE_PX: f32 = 2.5;

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

    // 두께는 **화면 픽셀 기준**으로 정하고 축마다 따로 월드 길이로 바꾼다.
    //
    // 두 가지 이유로 축을 나눠야 한다:
    //  1. 기울어지면 월드 Y 가 화면에서 `sin(pitch)` 만큼 눌린다. 같은 월드 두께를 쓰면
    //     가로선이 세로선보다 얇게 보인다.
    //  2. 1px 미만이 되면 픽셀 경계에 걸렸을 때 어떤 픽셀 중심도 덮지 못해 선이
    //     통째로 사라진다. 고정 줌 + 픽셀 스냅에서는 **모든** 선이 정확히 경계에
    //     놓이므로 전부 사라진다 (실제로 겪은 버그).
    let (vw, vh) = camera.viewport;
    let px_x = camera.view_width() / f32::from(u16::try_from(vw.max(1)).unwrap_or(u16::MAX));
    let px_y = camera.ground_height() / f32::from(u16::try_from(vh.max(1)).unwrap_or(u16::MAX));

    let thin_x = THIN_LINE_PX * px_x;
    let thin_y = THIN_LINE_PX * px_y;
    let thick_x = AXIS_LINE_PX * px_x;
    let thick_y = AXIS_LINE_PX * px_y;

    let width = max.x - min.x;
    let height = max.y - min.y;

    // 세로선 (X 상수)
    let x0 = (min.x / spacing).floor() * spacing;
    let mut x = x0;
    while x <= max.x {
        let is_major = (x / (spacing * 5.0)).fract().abs() < 1e-3;
        let is_axis = x.abs() < spacing * 0.01;

        out.push(RenderCommand::DrawRect {
            rotation: 0.0,
            center: Vec2::new(x, camera.center.y),
            size: Vec2::new(if is_axis { thick_x } else { thin_x }, height),
            z: 0.0,
            depth_bias: if is_axis { AXIS_BIAS } else { GRID_BIAS },
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
            rotation: 0.0,
            center: Vec2::new(camera.center.x, y),
            size: Vec2::new(width, if is_axis { thick_y } else { thin_y }),
            z: 0.0,
            depth_bias: if is_axis { AXIS_BIAS } else { GRID_BIAS },
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

/// 속이 빈 사각형 테두리를 그린다. 존 경계·마커 발판 표시에 쓴다.
///
/// 두께는 **화면 픽셀**로 준다. 축마다 따로 환산해야 기울였을 때 가로변만 얇아지지 않고,
/// 1px 미만으로 내려가 사라지지도 않는다 (`build` 의 주석 참고).
pub(crate) fn build_outline(
    camera: &Camera2d,
    min: Vec2,
    max: Vec2,
    thickness_px: f32,
    depth_bias: f32,
    color: [f32; 4],
    out: &mut Vec<RenderCommand>,
) {
    let (vw, vh) = camera.viewport;
    let tx = thickness_px * camera.view_width()
        / f32::from(u16::try_from(vw.max(1)).unwrap_or(u16::MAX));
    let ty = thickness_px * camera.ground_height()
        / f32::from(u16::try_from(vh.max(1)).unwrap_or(u16::MAX));

    let size = max - min;
    let center = (min + max) * 0.5;

    // 위 / 아래
    for y in [min.y, max.y] {
        out.push(RenderCommand::DrawRect {
            rotation: 0.0,
            center: Vec2::new(center.x, y),
            size: Vec2::new(size.x + tx, ty),
            z: 0.0,
            depth_bias,
            color,
        });
    }
    // 좌 / 우
    for x in [min.x, max.x] {
        out.push(RenderCommand::DrawRect {
            rotation: 0.0,
            center: Vec2::new(x, center.y),
            size: Vec2::new(tx, size.y + ty),
            z: 0.0,
            depth_bias,
            color,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 그리드 선이 화면에서 몇 픽셀 굵기로 나오는지.
    fn screen_thickness_px(camera: &Camera2d) -> (f32, f32) {
        let mut out = Vec::new();
        build(camera, &mut out);

        let (mut vertical, mut horizontal) = (f32::MAX, f32::MAX);
        for cmd in &out {
            if let RenderCommand::DrawRect { size, .. } = cmd {
                // 세로선은 폭이 좁고, 가로선은 높이가 좁다.
                if size.x < size.y {
                    vertical = vertical.min(
                        size.x / camera.view_width()
                            * f32::from(u16::try_from(camera.viewport.0).unwrap_or(u16::MAX)),
                    );
                } else {
                    horizontal = horizontal.min(
                        size.y / camera.ground_height()
                            * f32::from(u16::try_from(camera.viewport.1).unwrap_or(u16::MAX)),
                    );
                }
            }
        }
        (vertical, horizontal)
    }

    #[test]
    fn lines_are_at_least_one_pixel_thick_at_every_pitch() {
        // 1px 미만이면 픽셀 경계에 걸렸을 때 선이 통째로 사라진다. 고정 줌 + 픽셀 스냅에서는
        // 모든 선이 경계에 놓이므로 그리드 전체가 없어진다 — 실제로 겪은 버그다.
        for deg in [15.0_f32, 30.0, 45.0, 60.0, 90.0] {
            for view_height in [1.0_f32, 21.1, 100.0, 2000.0] {
                let camera = Camera2d {
                    center: Vec2::ZERO,
                    view_height,
                    viewport: (1280, 720),
                    pitch: deg.to_radians(),
                };
                let (v, h) = screen_thickness_px(&camera);
                assert!(v >= 1.0, "{deg}° vh={view_height}: 세로선 {v}px");
                assert!(h >= 1.0, "{deg}° vh={view_height}: 가로선 {h}px");
            }
        }
    }

    #[test]
    fn tilting_keeps_both_axes_equally_thick_on_screen() {
        // 축마다 따로 환산하지 않으면 기울였을 때 가로선만 얇아진다.
        let camera = Camera2d {
            center: Vec2::ZERO,
            view_height: 21.1,
            viewport: (1280, 720),
            pitch: Camera2d::PITCH_QUARTER,
        };
        let (v, h) = screen_thickness_px(&camera);
        assert!((v - h).abs() < 0.05, "세로 {v}px vs 가로 {h}px");
    }

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
            pitch: Camera2d::PITCH_QUARTER,
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
                pitch: Camera2d::PITCH_QUARTER,
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
            &Camera2d {
                center: Vec2::ZERO,
                view_height: 100.0,
                viewport: (1280, 720),
                pitch: Camera2d::PITCH_QUARTER,
            },
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
