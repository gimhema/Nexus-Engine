//! 단위·좌표 규약. **엔진 전체가 이 규약을 따른다.**
//!
//! | 항목 | 규약 |
//! |---|---|
//! | 길이 | **미터(m)**. `1.0 == 1m` |
//! | 축 | **Z-up**, 오른손 좌표계. 지면은 XY 평면 |
//! | 방향(heading) | **라디안**. `0 = +X`, **반시계(위에서 볼 때)가 +** |
//! | 속도 | m/s |
//!
//! # 왜 미터인가
//!
//! NexusEngine 서버의 게임플레이 수치가 이미 미터로 쓰여 있다 — 근접 사거리 `3.0`,
//! 화살 사거리 `20.0`, 이동 속도 `1.5~2.5`, 어그로 범위 `5.0`, 존 경계 `±1000`.
//! cm 로 읽으면 사거리 3cm·속도 2cm/s 가 되어 말이 되지 않는다.
//! 에디터 표시·glTF(M8)·물리 라이브러리도 미터가 기본이다.
//!
//! UE5 는 cm 를 쓴다. UE5 클라이언트를 붙일 때는 **그쪽 프로토콜 어댑터 한 곳에서 ×100** 한다.
//!
//! # 정밀도
//!
//! f32 는 존 원점에서 ±10km 범위까지 약 1mm 정밀도를 갖는다. 좌표는 존 단위 원점 기준으로 둔다.

use glam::Vec2;

/// 1 센티미터 (m 단위).
pub const CENTIMETER: f32 = 0.01;

/// 1 킬로미터 (m 단위).
pub const KILOMETER: f32 = 1000.0;

/// 방향(라디안)을 지면 위 단위 벡터로. `0 = +X`, 반시계가 +.
#[must_use]
pub fn heading_to_dir(heading: f32) -> Vec2 {
    Vec2::new(heading.cos(), heading.sin())
}

/// 지면 위 벡터를 방향(라디안, `(-π, π]`)으로. 길이 0 이면 0.
#[must_use]
pub fn dir_to_heading(dir: Vec2) -> f32 {
    if dir.length_squared() == 0.0 {
        return 0.0;
    }
    dir.y.atan2(dir.x)
}

/// 방향을 `n` 등분한 인덱스로 바꾼다. 스프라이트 시트의 방향 행을 고르는 데 쓴다.
///
/// `0` 번은 **`+X`(화면 오른쪽)를 한가운데 두는** 구간이고, 거기서 반시계로 증가한다.
/// `n = 4` 면 `0`=동, `1`=북, `2`=서, `3`=남. **스프라이트 시트의 행 순서가 이것과 맞아야 한다.**
///
/// `n` 은 시트마다 다르다 — 4방향 시트를 8방향으로 바꿔도 이 함수는 그대로다.
/// 호출부에서 방향 수를 상수로 박지 말 것.
///
/// 카메라가 회전한다면 `heading - camera_yaw` 를 넘긴다. 지금은 yaw 가 고정
/// ([`crate::Camera2d::YAW`] = 0)이라 방향을 그대로 넘기면 된다.
///
/// `n` 이 0 이면 0 을 반환한다.
#[must_use]
pub fn direction_index(heading: f32, n: u32) -> u32 {
    if n == 0 {
        return 0;
    }
    let step = core::f32::consts::TAU / n as f32;
    // 반 칸 밀어서 자르면 0 번 구간이 +X 를 한가운데 둔다.
    let shifted = normalize_heading(heading + step * 0.5);
    // f32 → u32 변환은 음수·NaN 에서 0 이 되므로 normalize_heading 이 앞에 있어야 한다.
    (shifted / step) as u32 % n
}

/// 방향을 `[0, 2π)` 로 정규화한다. 저장·비교 전에 쓴다.
#[must_use]
pub fn normalize_heading(heading: f32) -> f32 {
    heading.rem_euclid(core::f32::consts::TAU)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f32::consts::{FRAC_PI_2, PI, TAU};

    fn close(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 1e-5
    }

    #[test]
    fn heading_zero_points_along_plus_x() {
        assert!(close(heading_to_dir(0.0), Vec2::X));
    }

    #[test]
    fn heading_increases_counter_clockwise() {
        // 위에서 내려다볼 때 반시계 90° = +Y
        assert!(close(heading_to_dir(FRAC_PI_2), Vec2::Y));
        assert!(close(heading_to_dir(PI), -Vec2::X));
    }

    #[test]
    fn heading_round_trips() {
        for h in [0.0, 0.3, 1.5, 3.0, -2.0] {
            let back = dir_to_heading(heading_to_dir(h));
            assert!((back - h).abs() < 1e-5, "{h} → {back}");
        }
        assert!(dir_to_heading(Vec2::ZERO).abs() < f32::EPSILON);
    }

    #[test]
    fn normalize_wraps_into_one_turn() {
        assert!((normalize_heading(-FRAC_PI_2) - 3.0 * FRAC_PI_2).abs() < 1e-5);
        assert!((normalize_heading(TAU + 0.25) - 0.25).abs() < 1e-5);
    }

    #[test]
    fn direction_index_centres_bucket_zero_on_plus_x() {
        // 0 번은 +X 를 한가운데 둔다 — 경계가 아니라 중심이어야 스프라이트가 안 떨린다.
        for h in [-0.7_f32, -0.1, 0.0, 0.1, 0.7] {
            assert_eq!(direction_index(h, 4), 0, "heading {h}");
        }
    }

    #[test]
    fn direction_index_goes_counter_clockwise() {
        // n=4 → 0=동, 1=북, 2=서, 3=남
        assert_eq!(direction_index(0.0, 4), 0);
        assert_eq!(direction_index(FRAC_PI_2, 4), 1);
        assert_eq!(direction_index(PI, 4), 2);
        assert_eq!(direction_index(-FRAC_PI_2, 4), 3);
    }

    #[test]
    fn direction_index_stays_in_range_for_any_input() {
        for n in [1_u32, 2, 4, 8, 16] {
            for h in [-100.0_f32, -TAU, -0.001, 0.0, 0.001, TAU, 100.0, 1e9] {
                let i = direction_index(h, n);
                assert!(i < n, "n={n} heading={h} → {i}");
            }
        }
        assert_eq!(direction_index(1.0, 0), 0, "0 등분은 0 이어야 한다");
    }

    #[test]
    fn direction_index_partitions_the_circle_evenly() {
        // n 등분이면 각 구간이 정확히 한 번씩 나와야 한다.
        for n in [4_u32, 8] {
            let step = TAU / n as f32;
            for k in 0..n {
                let centre = step * k as f32;
                assert_eq!(direction_index(centre, n), k, "n={n} k={k}");
                // 구간 안쪽 어디를 찍어도 같은 인덱스
                assert_eq!(direction_index(centre + step * 0.45, n), k);
                assert_eq!(direction_index(centre - step * 0.45, n), k);
            }
        }
    }

    #[test]
    fn direction_index_matches_heading_round_trip() {
        // 벡터 → heading → 인덱스 경로가 실제 방향과 맞는지
        assert_eq!(direction_index(dir_to_heading(Vec2::X), 4), 0);
        assert_eq!(direction_index(dir_to_heading(Vec2::Y), 4), 1);
        assert_eq!(direction_index(dir_to_heading(-Vec2::X), 4), 2);
        assert_eq!(direction_index(dir_to_heading(-Vec2::Y), 4), 3);
    }

    #[test]
    fn server_sample_orientation_reads_as_radians() {
        // 서버 샘플의 상인 NPC orientation 1.5 → 약 86°
        assert!((1.5_f32.to_degrees() - 85.94).abs() < 0.01);
    }
}
