//! 2D 정사영 카메라.
//!
//! **이것은 3D 카메라다.** 투영만 정사영일 뿐, view·projection 행렬을 정상적으로
//! 만들어 낸다. M8 에서 3D 로 갈 때 [`Camera2d::view_proj`] 의 `directx::orthographic` 을
//! `directx::perspective` 로 바꾸고 궤도 컨트롤러를 붙이면 되며, 렌더 파이프라인은 손대지 않는다.
//!
//! 좌표계는 서버와 동일하게 **cm 단위, Z-up** 이다. 따라서 지면은 XY 평면이고,
//! 탑다운 카메라는 +Z 에서 −Z 방향을 내려다본다.

use glam::{Mat4, Vec2, Vec3};

/// XY 평면을 내려다보는 정사영 카메라.
#[derive(Clone, Copy, Debug)]
pub struct Camera2d {
    /// 화면 중심이 바라보는 월드 좌표 (cm).
    pub center: Vec2,
    /// 화면 세로 방향이 담는 월드 높이 (cm). 줌은 이 값을 조절한다.
    pub view_height: f32,
    /// 뷰포트 크기 (물리 픽셀).
    pub viewport: (u32, u32),
}

impl Default for Camera2d {
    fn default() -> Self {
        Self {
            center: Vec2::ZERO,
            view_height: 2000.0, // 20m
            viewport: (1, 1),
        }
    }
}

impl Camera2d {
    /// 줌 한계 — 너무 좁히거나 넓히면 깊이 정밀도와 그리드가 무너진다.
    pub const MIN_VIEW_HEIGHT: f32 = 50.0; // 0.5m
    pub const MAX_VIEW_HEIGHT: f32 = 200_000.0; // 2km

    /// 카메라가 내려다보는 높이 (cm). 지면(Z=0) 위 오브젝트를 담도록 충분히 높게 둔다.
    const EYE_HEIGHT: f32 = 10_000.0;

    /// 뷰포트 종횡비. 크기가 0 이면 1.0 을 반환한다.
    #[must_use]
    pub fn aspect(&self) -> f32 {
        let (w, h) = self.viewport;
        if w == 0 || h == 0 {
            return 1.0;
        }
        w as f32 / h as f32
    }

    /// 화면 가로 방향이 담는 월드 너비 (cm).
    #[must_use]
    pub fn view_width(&self) -> f32 {
        self.view_height * self.aspect()
    }

    /// 뷰 × 투영 행렬.
    #[must_use]
    pub fn view_proj(&self) -> Mat4 {
        let half_w = self.view_width() * 0.5;
        let half_h = self.view_height * 0.5;

        // wgpu(=WebGPU) 의 NDC 는 Y-up, 깊이 0..1 이다.
        // glam 에서 이 규약은 `directx` 모듈이다 — `vulkan` 모듈은 NDC Y-down 이라
        // 화면이 위아래로 뒤집힌다 (wgpu 가 내부에서 이미 뒤집어 주기 때문).
        let proj = glam::camera::rh::proj::directx::orthographic(
            -half_w,
            half_w,
            -half_h,
            half_h,
            0.1,
            Self::EYE_HEIGHT * 2.0,
        );

        // +Z 에서 지면을 내려다본다. 화면 위쪽이 월드 +Y 가 되도록 up 을 +Y 로 둔다.
        let eye = Vec3::new(self.center.x, self.center.y, Self::EYE_HEIGHT);
        let target = Vec3::new(self.center.x, self.center.y, 0.0);
        let view = glam::camera::rh::view::look_at_mat4(eye, target, Vec3::Y);

        proj * view
    }

    /// 화면 좌표(픽셀, 좌상단 원점)를 지면(Z=0) 월드 좌표로 변환한다.
    ///
    /// 정사영이므로 레이 캐스트가 필요 없다 — 선형 사상 하나로 끝난다.
    /// M5 의 마우스 피킹이 이 함수를 쓴다.
    #[must_use]
    pub fn screen_to_world(&self, screen: Vec2) -> Vec2 {
        let (w, h) = self.viewport;
        if w == 0 || h == 0 {
            return self.center;
        }

        // 픽셀 → [-0.5, 0.5] 정규화. Y 는 화면이 아래로 증가하므로 뒤집는다.
        let nx = screen.x / w as f32 - 0.5;
        let ny = 0.5 - screen.y / h as f32;

        self.center + Vec2::new(nx * self.view_width(), ny * self.view_height)
    }

    /// 지면 월드 좌표를 화면 좌표(픽셀)로 변환한다. [`screen_to_world`](Self::screen_to_world) 의 역.
    #[must_use]
    pub fn world_to_screen(&self, world: Vec2) -> Vec2 {
        let (w, h) = self.viewport;
        let d = world - self.center;

        let nx = d.x / self.view_width() + 0.5;
        let ny = 0.5 - d.y / self.view_height;

        Vec2::new(nx * w as f32, ny * h as f32)
    }

    /// 화면 픽셀 이동량만큼 카메라를 반대로 민다 (드래그 팬).
    pub fn pan_by_pixels(&mut self, delta_px: Vec2) {
        let (w, h) = self.viewport;
        if w == 0 || h == 0 {
            return;
        }
        // 화면을 오른쪽으로 끌면 월드가 오른쪽으로 따라와야 하므로 카메라는 왼쪽으로.
        self.center.x -= delta_px.x / w as f32 * self.view_width();
        self.center.y += delta_px.y / h as f32 * self.view_height;
    }

    /// 커서 아래 월드 지점을 고정한 채 확대/축소한다.
    ///
    /// `steps` 는 휠 노치 수. 양수면 확대.
    pub fn zoom_at(&mut self, steps: f32, cursor_px: Vec2) {
        let anchor_before = self.screen_to_world(cursor_px);

        // 노치당 10% — 지수 배율이라야 줌 레벨과 무관하게 체감이 일정하다.
        let factor = 0.9_f32.powf(steps);
        self.view_height =
            (self.view_height * factor).clamp(Self::MIN_VIEW_HEIGHT, Self::MAX_VIEW_HEIGHT);

        // 줌 후 같은 픽셀이 가리키는 월드 지점이 달라진 만큼 카메라를 보정한다.
        let anchor_after = self.screen_to_world(cursor_px);
        self.center += anchor_before - anchor_after;
    }

    /// 현재 보이는 월드 영역 `(min, max)`.
    #[must_use]
    pub fn visible_bounds(&self) -> (Vec2, Vec2) {
        let half = Vec2::new(self.view_width(), self.view_height) * 0.5;
        (self.center - half, self.center + half)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam() -> Camera2d {
        Camera2d {
            center: Vec2::new(100.0, -50.0),
            view_height: 1000.0,
            viewport: (800, 400),
        }
    }

    #[test]
    fn aspect_and_width() {
        let c = cam();
        assert!((c.aspect() - 2.0).abs() < 1e-6);
        assert!((c.view_width() - 2000.0).abs() < 1e-3);
    }

    #[test]
    fn zero_viewport_does_not_divide_by_zero() {
        let c = Camera2d {
            viewport: (0, 0),
            ..Camera2d::default()
        };
        assert!((c.aspect() - 1.0).abs() < 1e-6);
        assert_eq!(c.screen_to_world(Vec2::new(10.0, 10.0)), c.center);
    }

    #[test]
    fn screen_center_maps_to_camera_center() {
        let c = cam();
        let mid = Vec2::new(400.0, 200.0);
        let w = c.screen_to_world(mid);
        assert!((w - c.center).length() < 1e-3, "{w:?} != {:?}", c.center);
    }

    #[test]
    fn screen_world_round_trip() {
        let c = cam();
        for px in [
            Vec2::new(0.0, 0.0),
            Vec2::new(800.0, 400.0),
            Vec2::new(123.0, 371.0),
        ] {
            let back = c.world_to_screen(c.screen_to_world(px));
            assert!((back - px).length() < 1e-2, "{px:?} → {back:?}");
        }
    }

    #[test]
    fn screen_y_is_flipped() {
        // 화면 위쪽(y 작음)이 월드 +Y 여야 한다
        let c = cam();
        let top = c.screen_to_world(Vec2::new(400.0, 0.0));
        let bottom = c.screen_to_world(Vec2::new(400.0, 400.0));
        assert!(top.y > bottom.y, "화면 위가 월드 +Y 가 아님");
    }

    #[test]
    fn zoom_keeps_cursor_anchored() {
        let mut c = cam();
        let cursor = Vec2::new(600.0, 120.0);
        let before = c.screen_to_world(cursor);

        c.zoom_at(3.0, cursor);
        let after = c.screen_to_world(cursor);

        assert!(
            (before - after).length() < 1e-2,
            "커서 아래 지점이 움직임: {before:?} → {after:?}"
        );
        assert!(c.view_height < 1000.0, "확대되지 않음");
    }

    #[test]
    fn zoom_is_clamped() {
        let mut c = cam();
        c.zoom_at(1000.0, Vec2::ZERO);
        assert!(c.view_height >= Camera2d::MIN_VIEW_HEIGHT);

        c.zoom_at(-1000.0, Vec2::ZERO);
        assert!(c.view_height <= Camera2d::MAX_VIEW_HEIGHT);
    }

    #[test]
    fn pan_moves_world_with_cursor() {
        let mut c = cam();
        let anchor = c.screen_to_world(Vec2::new(400.0, 200.0));

        // 오른쪽으로 100px 끌면 월드도 오른쪽으로 100px 만큼 따라온다
        c.pan_by_pixels(Vec2::new(100.0, 0.0));
        let moved = c.world_to_screen(anchor);

        assert!(
            (moved.x - 500.0).abs() < 1e-2,
            "팬 방향이 반대이거나 배율이 틀림: {moved:?}"
        );
    }

    #[test]
    fn view_proj_maps_center_to_ndc_origin() {
        let c = cam();
        let vp = c.view_proj();
        let clip = vp * glam::Vec4::new(c.center.x, c.center.y, 0.0, 1.0);
        let ndc = clip.truncate() / clip.w;

        assert!(ndc.x.abs() < 1e-4 && ndc.y.abs() < 1e-4, "{ndc:?}");
        // wgpu 깊이 범위는 0..1 이므로 지면은 그 안에 있어야 한다
        assert!((0.0..=1.0).contains(&ndc.z), "깊이 범위 이탈: {}", ndc.z);
    }

    #[test]
    fn higher_objects_get_smaller_depth() {
        // 깊이 규약: 일반 Z. 카메라에 가까울수록(월드 Z 가 클수록) 깊이 값이 작다.
        // 렌더러는 이를 전제로 LessEqual 비교를 쓴다. 이 테스트가 깨지면
        // 렌더러의 depth_compare 와 깊이 초기값도 함께 바꿔야 한다.
        let c = cam();
        let vp = c.view_proj();
        let depth = |z: f32| {
            let clip = vp * glam::Vec4::new(c.center.x, c.center.y, z, 1.0);
            clip.z / clip.w
        };

        assert!(
            depth(1.0) < depth(0.0),
            "마커(z=1)가 지면보다 앞이어야 한다"
        );
        assert!(
            depth(0.0) < depth(-1.0),
            "지면이 그리드(z=-1)보다 앞이어야 한다"
        );
    }

    #[test]
    fn visible_bounds_cover_viewport_corners() {
        let c = cam();
        let (min, max) = c.visible_bounds();
        let tl = c.screen_to_world(Vec2::ZERO);
        let br = c.screen_to_world(Vec2::new(800.0, 400.0));

        assert!((min.x - tl.x).abs() < 1e-2 && (max.y - tl.y).abs() < 1e-2);
        assert!((max.x - br.x).abs() < 1e-2 && (min.y - br.y).abs() < 1e-2);
    }
}
