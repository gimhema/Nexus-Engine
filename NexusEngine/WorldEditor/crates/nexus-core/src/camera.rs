//! 쿼터뷰 정사영 카메라.
//!
//! **투영은 정사영을 유지한다.** 라그나로크 계열 2D 스프라이트 MMORPG 가 목표이고,
//! 그 장르는 원근이 아니라 기울어진 정사영이다. 원근으로 바꾸면 스프라이트가
//! 화면 위치마다 다르게 찌그러져 픽셀아트가 무너진다.
//!
//! 좌표계는 **미터(m), Z-up** 이다 ([`crate::units`]). 지면은 XY 평면이다.
//!
//! # 시점 고정
//!
//! **yaw 는 [`Camera2d::YAW`] 로 고정**이며 필드가 아니다. 회전을 허용하면
//! 스프라이트 방향 선택·타일 배치·컬링이 전부 복잡해지는데, 목표 장르는 시점 고정이라
//! 그 복잡도를 살 이유가 없다. yaw 가 0 이라 다음이 성립한다:
//!
//! - 화면 가로 = 월드 +X, 화면 세로 = 월드 +Y (단, `sin(pitch)` 만큼 눌린다)
//! - 보이는 지면 영역이 **축 정렬 사각형**으로 남는다 → 그리드·컬링이 단순하다
//!
//! # pitch
//!
//! 지면에서 올려다본 각도다. 90° 면 수직 탑다운, 45° 가 쿼터뷰다.
//! 정사영이라 **카메라의 거리는 화면에 영향을 주지 않는다** — 잘림 평면에만 쓰인다.
//!
//! pitch 가 90° 가 아니면 **월드 Z(높이)가 화면 세로 위치를 밀어 올린다.**
//! 이것은 의도된 동작이다(키 큰 것이 위로 솟아 보인다). 그래서 단순히 그리는 순서를
//! 정하려는 용도로 Z 를 쓰면 안 된다 — 그쪽은 `RenderCommand` 의 `depth_bias` 를 쓴다.

use glam::{Mat4, Vec2, Vec3};

/// XY 평면을 내려다보는 쿼터뷰 정사영 카메라.
#[derive(Clone, Copy, Debug)]
pub struct Camera2d {
    /// 화면 중심이 바라보는 지면 좌표 (m).
    pub center: Vec2,
    /// 화면 세로 방향이 담는 월드 길이 (m) — **카메라 up 축 기준**이다.
    ///
    /// 지면에서 보이는 세로 범위는 이것보다 넓다 ([`Camera2d::ground_height`]).
    /// 줌은 이 값을 조절한다.
    pub view_height: f32,
    /// 뷰포트 크기 (물리 픽셀).
    pub viewport: (u32, u32),
    /// 지면으로부터의 올려본 각도 (라디안). [`Camera2d::PITCH_QUARTER`] 참고.
    pub pitch: f32,
}

impl Default for Camera2d {
    fn default() -> Self {
        Self {
            center: Vec2::ZERO,
            view_height: 20.0,
            viewport: (1, 1),
            pitch: Self::PITCH_QUARTER,
        }
    }
}

impl Camera2d {
    /// 고정 yaw. 화면 가로가 월드 +X 가 되는 값이다. **필드가 아니다** — 모듈 문서 참고.
    pub const YAW: f32 = 0.0;

    /// 쿼터뷰 (45°). 목표 장르의 기본 시점이다.
    pub const PITCH_QUARTER: f32 = core::f32::consts::FRAC_PI_4;
    /// 수직 탑다운 (90°). 좌표를 정밀하게 찍어야 하는 편집 작업에 쓴다.
    pub const PITCH_TOPDOWN: f32 = core::f32::consts::FRAC_PI_2;

    /// pitch 하한. 0 에 가까우면 지면이 한 줄로 눌려 편집도 조준도 불가능해진다.
    pub const MIN_PITCH: f32 = 0.175; // 약 10°
    /// pitch 상한 = 수직.
    pub const MAX_PITCH: f32 = Self::PITCH_TOPDOWN;

    /// 줌 한계 — 너무 좁히거나 넓히면 깊이 정밀도와 그리드가 무너진다.
    pub const MIN_VIEW_HEIGHT: f32 = 0.5;
    pub const MAX_VIEW_HEIGHT: f32 = 5_000.0;

    /// 게임 모드 기본 배율 — 월드 1m 가 화면 32px.
    ///
    /// 에디터는 자유 줌을 쓰지만, 게임 화면은 이 값으로 고정한다.
    /// 창 크기가 바뀌면 확대율이 아니라 **보이는 범위**가 달라진다.
    pub const PIXELS_PER_METER: f32 = 32.0;

    /// 잘림 평면 여유 (m). 오브젝트 높이와 near 여유를 함께 덮는다.
    ///
    /// 정사영이라 이 값을 키워도 화면은 변하지 않고 깊이 버퍼 해상도만 낮아진다.
    /// 2D 스프라이트 기준 100m 면 과할 만큼 넉넉하다.
    const DEPTH_MARGIN: f32 = 100.0;

    /// pitch 를 유효 범위로 자른다. 직접 필드를 건드린 뒤에도 안전하도록 항상 이것을 거친다.
    #[must_use]
    pub fn clamped_pitch(&self) -> f32 {
        self.pitch.clamp(Self::MIN_PITCH, Self::MAX_PITCH)
    }

    /// 뷰포트 종횡비. 크기가 0 이면 1.0 을 반환한다.
    #[must_use]
    pub fn aspect(&self) -> f32 {
        let (w, h) = self.viewport;
        if w == 0 || h == 0 {
            return 1.0;
        }
        w as f32 / h as f32
    }

    /// 화면 가로 방향이 담는 월드 너비 (m). yaw 가 0 이라 그대로 월드 X 범위다.
    #[must_use]
    pub fn view_width(&self) -> f32 {
        self.view_height * self.aspect()
    }

    /// 화면에 보이는 **지면의** 세로 범위 (m).
    ///
    /// 기울어질수록 같은 화면에 더 넓은 지면이 담긴다 — `view_height / sin(pitch)`.
    /// 탑다운(90°)에서는 `view_height` 와 같다.
    #[must_use]
    pub fn ground_height(&self) -> f32 {
        self.view_height / self.clamped_pitch().sin()
    }

    /// 카메라 축 `(right, up, forward)`. 모두 단위 벡터이고 서로 직교한다.
    ///
    /// `look_at` 대신 이 기저를 직접 만든다 — `look_at` 은 up 벡터가 시선과 나란해지는
    /// 수직 탑다운에서 퇴화하는데, 이 기저는 어느 pitch 에서도 그런 지점이 없다.
    #[must_use]
    pub fn basis(&self) -> (Vec3, Vec3, Vec3) {
        let (sin_p, cos_p) = self.clamped_pitch().sin_cos();
        let right = Vec3::X;
        let up = Vec3::new(0.0, sin_p, cos_p);
        let forward = Vec3::new(0.0, cos_p, -sin_p);
        (right, up, forward)
    }

    /// 깊이 방향 반경과 카메라 거리 `(half_depth, eye_distance)`.
    fn depth_extent(&self) -> (f32, f32) {
        let pitch = self.clamped_pitch();
        // 지면이 깊이 방향으로 차지하는 반경 = (지면 세로 범위 / 2) · cos(pitch)
        //                                  = view_height / 2 · cot(pitch)
        let ground = self.ground_height() * 0.5 * pitch.cos();
        let half_depth = ground + Self::DEPTH_MARGIN;
        (half_depth, half_depth + Self::DEPTH_MARGIN)
    }

    /// 뷰 × 투영 행렬.
    #[must_use]
    pub fn view_proj(&self) -> Mat4 {
        let half_w = self.view_width() * 0.5;
        let half_h = self.view_height * 0.5;
        let (half_depth, eye_dist) = self.depth_extent();

        // wgpu(=WebGPU) 의 NDC 는 Y-up, 깊이 0..1 이다.
        // glam 에서 이 규약은 `directx` 모듈이다 — `vulkan` 모듈은 NDC Y-down 이라
        // 화면이 위아래로 뒤집힌다 (wgpu 가 내부에서 이미 뒤집어 주기 때문).
        let proj = glam::camera::rh::proj::directx::orthographic(
            -half_w,
            half_w,
            -half_h,
            half_h,
            Self::DEPTH_MARGIN,
            eye_dist + half_depth,
        );

        let (_, up, forward) = self.basis();
        // 정사영이므로 눈 위치는 화면에 영향을 주지 않는다 — 잘림 평면 안에만 들어오면 된다.
        let eye = Vec3::new(self.center.x, self.center.y, 0.0) - forward * eye_dist;
        let view = glam::camera::rh::view::look_to_mat4(eye, forward, up);

        proj * view
    }

    /// 화면 좌표(픽셀, 좌상단 원점)를 **지면(Z=0)** 월드 좌표로 변환한다.
    ///
    /// 정사영이라 지면과의 교차가 선형 사상 하나로 끝난다 — 레이 캐스트가 필요 없다.
    /// 마우스 피킹과 클릭 이동이 이 함수를 쓴다.
    ///
    /// 높이가 있는 대상을 집으려면 그 높이만큼 화면 Y 를 보정해야 한다(S3).
    #[must_use]
    pub fn screen_to_world(&self, screen: Vec2) -> Vec2 {
        let (w, h) = self.viewport;
        if w == 0 || h == 0 {
            return self.center;
        }

        // 픽셀 → [-0.5, 0.5] 정규화. Y 는 화면이 아래로 증가하므로 뒤집는다.
        let nx = screen.x / w as f32 - 0.5;
        let ny = 0.5 - screen.y / h as f32;

        self.center + Vec2::new(nx * self.view_width(), ny * self.ground_height())
    }

    /// 지면 월드 좌표를 화면 좌표(픽셀)로 변환한다. [`screen_to_world`](Self::screen_to_world) 의 역.
    #[must_use]
    pub fn world_to_screen(&self, world: Vec2) -> Vec2 {
        let (w, h) = self.viewport;
        let d = world - self.center;

        let nx = d.x / self.view_width() + 0.5;
        let ny = 0.5 - d.y / self.ground_height();

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
        self.center.y += delta_px.y / h as f32 * self.ground_height();
    }

    /// 커서 아래 지면 지점을 고정한 채 확대/축소한다.
    ///
    /// `steps` 는 휠 노치 수. 양수면 확대.
    pub fn zoom_at(&mut self, steps: f32, cursor_px: Vec2) {
        let anchor_before = self.screen_to_world(cursor_px);

        // 노치당 10% — 지수 배율이라야 줌 레벨과 무관하게 체감이 일정하다.
        let factor = 0.9_f32.powf(steps);
        self.view_height =
            (self.view_height * factor).clamp(Self::MIN_VIEW_HEIGHT, Self::MAX_VIEW_HEIGHT);

        // 줌 후 같은 픽셀이 가리키는 지점이 달라진 만큼 카메라를 보정한다.
        let anchor_after = self.screen_to_world(cursor_px);
        self.center += anchor_before - anchor_after;
    }

    /// 현재 보이는 **지면** 영역 `(min, max)`.
    ///
    /// yaw 가 고정이라 축 정렬 사각형이다. 높이가 있는 대상은 지면 위치가 이 범위 밖이어도
    /// 화면에 걸칠 수 있으므로, 컬링에 쓸 때는 여유를 둘 것.
    #[must_use]
    pub fn visible_bounds(&self) -> (Vec2, Vec2) {
        let half = Vec2::new(self.view_width(), self.ground_height()) * 0.5;
        (self.center - half, self.center + half)
    }

    // ── 고정 줌 (게임 모드) ──────────────────────────────────────────────────

    /// 현재 배율 (화면 픽셀 / 월드 m). 카메라 up 축 기준이다.
    #[must_use]
    pub fn pixels_per_meter(&self) -> f32 {
        let h = self.viewport.1;
        if h == 0 || self.view_height <= 0.0 {
            return 0.0;
        }
        h as f32 / self.view_height
    }

    /// 배율을 고정한다. 창이 커지면 확대율이 아니라 **보이는 범위**가 넓어진다.
    ///
    /// 게임 모드에서 매 프레임(또는 리사이즈마다) 호출한다.
    pub fn set_pixels_per_meter(&mut self, pixels_per_meter: f32) {
        let (_, h) = self.viewport;
        if h == 0 || pixels_per_meter <= 0.0 {
            return;
        }
        self.view_height =
            (h as f32 / pixels_per_meter).clamp(Self::MIN_VIEW_HEIGHT, Self::MAX_VIEW_HEIGHT);
    }

    /// 카메라 중심을 화면 픽셀 격자에 맞춘다.
    ///
    /// 픽셀아트는 스프라이트가 픽셀 사이에 걸치면 카메라가 움직일 때마다 가장자리가
    /// 떨린다. 고정 줌과 짝으로 쓴다.
    ///
    /// 화면 세로는 `sin(pitch)` 만큼 눌리므로 Y 는 그만큼 더 촘촘한 격자에 맞춘다.
    pub fn snap_to_pixel_grid(&mut self) {
        let ppm = self.pixels_per_meter();
        if ppm <= 0.0 || !ppm.is_finite() {
            return;
        }
        let ppm_y = ppm * self.clamped_pitch().sin();
        if ppm_y <= 0.0 {
            return;
        }
        self.center.x = (self.center.x * ppm).round() / ppm;
        self.center.y = (self.center.y * ppm_y).round() / ppm_y;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f32::consts::FRAC_PI_4;

    fn cam() -> Camera2d {
        Camera2d {
            center: Vec2::new(100.0, -50.0),
            view_height: 1000.0,
            viewport: (800, 400),
            pitch: Camera2d::PITCH_QUARTER,
        }
    }

    fn topdown() -> Camera2d {
        Camera2d {
            pitch: Camera2d::PITCH_TOPDOWN,
            ..cam()
        }
    }

    fn depth_at(c: &Camera2d, world: Vec3) -> f32 {
        let clip = c.view_proj() * world.extend(1.0);
        clip.z / clip.w
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
        assert!(c.pixels_per_meter().abs() < f32::EPSILON);
    }

    #[test]
    fn basis_is_orthonormal_at_every_pitch() {
        for deg in [10.0_f32, 30.0, 45.0, 60.0, 90.0] {
            let c = Camera2d {
                pitch: deg.to_radians(),
                ..cam()
            };
            let (r, u, f) = c.basis();
            for v in [r, u, f] {
                assert!((v.length() - 1.0).abs() < 1e-5, "{deg}°: 단위 벡터 아님");
            }
            assert!(r.dot(u).abs() < 1e-5, "{deg}°: right·up");
            assert!(r.dot(f).abs() < 1e-5, "{deg}°: right·forward");
            assert!(u.dot(f).abs() < 1e-5, "{deg}°: up·forward");
        }
    }

    #[test]
    fn topdown_ground_height_equals_view_height() {
        let c = topdown();
        assert!((c.ground_height() - c.view_height).abs() < 1e-3);
    }

    #[test]
    fn tilting_shows_more_ground() {
        // 기울일수록 같은 화면에 더 넓은 지면이 담긴다 — 쿼터뷰의 기본 성질.
        let q = cam();
        let t = topdown();
        assert!(
            q.ground_height() > t.ground_height(),
            "45°({}) 가 90°({}) 보다 넓어야 한다",
            q.ground_height(),
            t.ground_height()
        );
        // 45° 는 1/sin(45°) = √2 배
        assert!((q.ground_height() / q.view_height - core::f32::consts::SQRT_2).abs() < 1e-4);
    }

    #[test]
    fn screen_center_maps_to_camera_center() {
        for c in [cam(), topdown()] {
            let mid = Vec2::new(400.0, 200.0);
            let w = c.screen_to_world(mid);
            assert!((w - c.center).length() < 1e-3, "{w:?} != {:?}", c.center);
        }
    }

    #[test]
    fn screen_world_round_trip() {
        for c in [cam(), topdown()] {
            for px in [
                Vec2::new(0.0, 0.0),
                Vec2::new(800.0, 400.0),
                Vec2::new(123.0, 371.0),
            ] {
                let back = c.world_to_screen(c.screen_to_world(px));
                assert!((back - px).length() < 1e-2, "{px:?} → {back:?}");
            }
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
    fn projection_matches_screen_mapping_on_the_ground() {
        // view_proj 와 world_to_screen 이 같은 사상을 나타내야 한다.
        // 둘이 어긋나면 마우스로 집은 곳과 그려진 곳이 달라진다.
        let c = cam();
        let vp = c.view_proj();
        for world in [
            Vec2::new(100.0, -50.0),
            Vec2::new(500.0, 200.0),
            Vec2::new(-300.0, -400.0),
        ] {
            let clip = vp * Vec3::new(world.x, world.y, 0.0).extend(1.0);
            let ndc = clip.truncate() / clip.w;

            let screen = c.world_to_screen(world);
            let expect_ndc = Vec2::new(screen.x / 800.0 * 2.0 - 1.0, 1.0 - screen.y / 400.0 * 2.0);
            assert!(
                (ndc.x - expect_ndc.x).abs() < 1e-3 && (ndc.y - expect_ndc.y).abs() < 1e-3,
                "{world:?}: 투영 {ndc:?} vs 화면사상 {expect_ndc:?}"
            );
        }
    }

    #[test]
    fn height_pushes_sprites_up_the_screen_when_tilted() {
        // 쿼터뷰의 핵심 성질 — 높은 것이 화면 위로 솟는다. S3 빌보드가 이것에 의존한다.
        let c = cam();
        let vp = c.view_proj();
        let ndc_y = |z: f32| {
            let clip = vp * Vec3::new(c.center.x, c.center.y, z).extend(1.0);
            (clip.y / clip.w) as f64
        };
        assert!(ndc_y(10.0) > ndc_y(0.0), "높이가 화면 위로 가지 않음");

        // 탑다운에서는 높이가 화면 위치를 바꾸지 않아야 한다 (깊이에만 영향).
        let t = topdown();
        let tvp = t.view_proj();
        let t_ndc_y = |z: f32| {
            let clip = tvp * Vec3::new(t.center.x, t.center.y, z).extend(1.0);
            clip.y / clip.w
        };
        assert!(
            (t_ndc_y(10.0) - t_ndc_y(0.0)).abs() < 1e-5,
            "탑다운인데 높이가 화면을 밀었다"
        );
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
    fn pan_is_consistent_with_picking_when_tilted() {
        // 기울어진 상태에서 세로 팬 배율을 틀리면 끌리는 양이 커서와 어긋난다.
        let mut c = cam();
        let anchor = c.screen_to_world(Vec2::new(400.0, 200.0));
        c.pan_by_pixels(Vec2::new(0.0, 60.0));
        let moved = c.world_to_screen(anchor);
        assert!(
            (moved.y - 260.0).abs() < 1e-2,
            "세로 팬이 커서를 따라오지 않음: {moved:?}"
        );
    }

    #[test]
    fn view_proj_maps_center_to_ndc_origin() {
        for c in [cam(), topdown()] {
            let vp = c.view_proj();
            let clip = vp * glam::Vec4::new(c.center.x, c.center.y, 0.0, 1.0);
            let ndc = clip.truncate() / clip.w;

            assert!(ndc.x.abs() < 1e-4 && ndc.y.abs() < 1e-4, "{ndc:?}");
            // wgpu 깊이 범위는 0..1 이므로 지면은 그 안에 있어야 한다
            assert!((0.0..=1.0).contains(&ndc.z), "깊이 범위 이탈: {}", ndc.z);
        }
    }

    #[test]
    fn higher_objects_get_smaller_depth() {
        // 깊이 규약: 일반 Z. 카메라에 가까울수록(월드 Z 가 클수록) 깊이 값이 작다.
        // 렌더러는 이를 전제로 LessEqual 비교를 쓴다. 이 테스트가 깨지면
        // 렌더러의 depth_compare 와 깊이 초기값도 함께 바꿔야 한다.
        for c in [cam(), topdown()] {
            let at = |z: f32| depth_at(&c, Vec3::new(c.center.x, c.center.y, z));
            assert!(at(1.0) < at(0.0), "마커(z=1)가 지면보다 앞이어야 한다");
            assert!(at(0.0) < at(-1.0), "지면이 그리드(z=-1)보다 앞이어야 한다");
        }
    }

    #[test]
    fn whole_visible_ground_stays_inside_the_depth_range() {
        // 기울이면 지면이 깊이 방향으로 퍼진다. 잘림 평면이 좁으면 화면 위아래가 잘려 나간다.
        for view_height in [
            Camera2d::MIN_VIEW_HEIGHT,
            20.0,
            1000.0,
            Camera2d::MAX_VIEW_HEIGHT,
        ] {
            for deg in [10.0_f32, 45.0, 90.0] {
                let c = Camera2d {
                    view_height,
                    pitch: deg.to_radians(),
                    ..cam()
                };
                let (min, max) = c.visible_bounds();
                for corner in [min, max, Vec2::new(min.x, max.y), Vec2::new(max.x, min.y)] {
                    let d = depth_at(&c, Vec3::new(corner.x, corner.y, 0.0));
                    assert!(
                        (0.0..=1.0).contains(&d),
                        "vh={view_height} {deg}° {corner:?}: 깊이 {d} 가 범위 밖"
                    );
                }
            }
        }
    }

    #[test]
    fn visible_bounds_cover_viewport_corners() {
        for c in [cam(), topdown()] {
            let (min, max) = c.visible_bounds();
            let tl = c.screen_to_world(Vec2::ZERO);
            let br = c.screen_to_world(Vec2::new(800.0, 400.0));

            assert!((min.x - tl.x).abs() < 1e-2 && (max.y - tl.y).abs() < 1e-2);
            assert!((max.x - br.x).abs() < 1e-2 && (min.y - br.y).abs() < 1e-2);
        }
    }

    #[test]
    fn pitch_is_clamped_even_if_the_field_is_abused() {
        let c = Camera2d {
            pitch: 0.0,
            ..cam()
        };
        assert!(c.clamped_pitch() >= Camera2d::MIN_PITCH);
        assert!(c.ground_height().is_finite(), "0° 에서 지면이 무한대가 됨");

        let c = Camera2d {
            pitch: 10.0,
            ..cam()
        };
        assert!((c.clamped_pitch() - Camera2d::MAX_PITCH).abs() < 1e-6);
    }

    #[test]
    fn fixed_zoom_keeps_scale_and_widens_the_view() {
        let mut small = Camera2d {
            viewport: (800, 400),
            ..Camera2d::default()
        };
        let mut large = Camera2d {
            viewport: (1600, 1000),
            ..Camera2d::default()
        };
        small.set_pixels_per_meter(Camera2d::PIXELS_PER_METER);
        large.set_pixels_per_meter(Camera2d::PIXELS_PER_METER);

        // 배율은 같고
        assert!((small.pixels_per_meter() - large.pixels_per_meter()).abs() < 1e-3);
        // 큰 창이 더 많이 본다
        assert!(large.ground_height() > small.ground_height());
        assert!((small.view_height - 400.0 / Camera2d::PIXELS_PER_METER).abs() < 1e-3);
    }

    #[test]
    fn fixed_zoom_ignores_nonsense_input() {
        let mut c = cam();
        let before = c.view_height;
        c.set_pixels_per_meter(0.0);
        c.set_pixels_per_meter(-5.0);
        assert!((c.view_height - before).abs() < f32::EPSILON);
    }

    #[test]
    fn pixel_snap_lands_on_whole_screen_pixels() {
        let mut c = Camera2d {
            center: Vec2::new(4.237_81, -1.593_64),
            viewport: (800, 400),
            pitch: FRAC_PI_4,
            ..Camera2d::default()
        };
        c.set_pixels_per_meter(Camera2d::PIXELS_PER_METER);
        c.snap_to_pixel_grid();

        // 원점이 화면에서 정수 픽셀에 놓여야 한다
        let origin = c.world_to_screen(Vec2::ZERO);
        for v in [origin.x, origin.y] {
            assert!((v - v.round()).abs() < 1e-2, "정수 픽셀이 아님: {origin:?}");
        }
    }
}
