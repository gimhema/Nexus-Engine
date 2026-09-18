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
    fn server_sample_orientation_reads_as_radians() {
        // 서버 샘플의 상인 NPC orientation 1.5 → 약 86°
        assert!((1.5_f32.to_degrees() - 85.94).abs() < 0.01);
    }
}
