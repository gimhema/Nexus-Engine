//! 프레임 단위 입력 스냅샷.
//!
//! winit 의 `KeyCode` / `MouseButton` 을 그대로 재수출한다. 자체 열거형으로
//! 다시 매핑하지 않는 이유는 winit 자체가 이미 크로스플랫폼이고, 200개가 넘는
//! 키코드를 손으로 옮기는 것은 이식성에 아무것도 보태지 않기 때문이다.
//! (CLAUDE.md 핵심 원칙 5 — 과도한 추상화 지양)

use std::collections::HashSet;

pub use winit::event::MouseButton;
pub use winit::keyboard::KeyCode;

/// 한 프레임 동안의 입력 상태.
///
/// `held` 계열은 눌려 있는 동안 계속 참이고, `pressed` / `released` 계열은
/// 상태가 바뀐 프레임에만 참이다. 후자는 [`end_frame`](Self::end_frame) 에서
/// 비워지므로, 한 프레임에 여러 고정 스텝이 돌아도 중복 소비되지 않는다.
#[derive(Debug, Default, Clone)]
pub struct Input {
    keys_held: HashSet<KeyCode>,
    keys_pressed: HashSet<KeyCode>,
    keys_released: HashSet<KeyCode>,

    buttons_held: HashSet<MouseButton>,
    buttons_pressed: HashSet<MouseButton>,
    buttons_released: HashSet<MouseButton>,

    cursor: (f32, f32),
    cursor_delta: (f32, f32),
    wheel: f32,
}

impl Input {
    /// 키가 눌려 있는가.
    #[must_use]
    pub fn key_held(&self, key: KeyCode) -> bool {
        self.keys_held.contains(&key)
    }

    /// 이번 프레임에 새로 눌렸는가.
    #[must_use]
    pub fn key_pressed(&self, key: KeyCode) -> bool {
        self.keys_pressed.contains(&key)
    }

    /// 이번 프레임에 떼어졌는가.
    #[must_use]
    pub fn key_released(&self, key: KeyCode) -> bool {
        self.keys_released.contains(&key)
    }

    /// 마우스 버튼이 눌려 있는가.
    #[must_use]
    pub fn button_held(&self, button: MouseButton) -> bool {
        self.buttons_held.contains(&button)
    }

    /// 이번 프레임에 새로 눌렸는가.
    #[must_use]
    pub fn button_pressed(&self, button: MouseButton) -> bool {
        self.buttons_pressed.contains(&button)
    }

    /// 이번 프레임에 떼어졌는가.
    #[must_use]
    pub fn button_released(&self, button: MouseButton) -> bool {
        self.buttons_released.contains(&button)
    }

    /// 커서 위치 (물리 픽셀, 창 좌상단 기준).
    #[must_use]
    pub fn cursor(&self) -> (f32, f32) {
        self.cursor
    }

    /// 직전 프레임 대비 커서 이동량.
    #[must_use]
    pub fn cursor_delta(&self) -> (f32, f32) {
        self.cursor_delta
    }

    /// 이번 프레임의 휠 누적량. 위로 굴리면 양수.
    #[must_use]
    pub fn wheel(&self) -> f32 {
        self.wheel
    }

    /// 눌려 있는 키가 하나도 없는가. (디버그·테스트용)
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.keys_held.is_empty() && self.buttons_held.is_empty()
    }

    // ── 이벤트 루프가 호출하는 갱신 API (크레이트 내부 전용) ──────────────

    pub(crate) fn on_key(&mut self, key: KeyCode, pressed: bool) {
        if pressed {
            // 키 반복(auto-repeat)은 최초 1회만 pressed 로 취급한다.
            if self.keys_held.insert(key) {
                self.keys_pressed.insert(key);
            }
        } else if self.keys_held.remove(&key) {
            self.keys_released.insert(key);
        }
    }

    pub(crate) fn on_button(&mut self, button: MouseButton, pressed: bool) {
        if pressed {
            if self.buttons_held.insert(button) {
                self.buttons_pressed.insert(button);
            }
        } else if self.buttons_held.remove(&button) {
            self.buttons_released.insert(button);
        }
    }

    pub(crate) fn on_cursor(&mut self, x: f32, y: f32) {
        self.cursor_delta = (x - self.cursor.0, y - self.cursor.1);
        self.cursor = (x, y);
    }

    pub(crate) fn on_wheel(&mut self, delta: f32) {
        self.wheel += delta;
    }

    /// 포커스를 잃으면 눌림 상태를 모두 해제한다.
    /// (Alt+Tab 으로 전환했다가 돌아오면 키가 눌린 채로 남는 고전적 버그 방지)
    pub(crate) fn on_focus_lost(&mut self) {
        for key in self.keys_held.drain() {
            self.keys_released.insert(key);
        }
        for button in self.buttons_held.drain() {
            self.buttons_released.insert(button);
        }
    }

    /// 프레임 종료 — 일회성 상태를 비운다.
    pub(crate) fn end_frame(&mut self) {
        self.keys_pressed.clear();
        self.keys_released.clear();
        self.buttons_pressed.clear();
        self.buttons_released.clear();
        self.cursor_delta = (0.0, 0.0);
        self.wheel = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn press_then_hold_then_release() {
        let mut input = Input::default();

        input.on_key(KeyCode::KeyW, true);
        assert!(input.key_pressed(KeyCode::KeyW));
        assert!(input.key_held(KeyCode::KeyW));

        input.end_frame();
        assert!(!input.key_pressed(KeyCode::KeyW), "pressed 는 1프레임만 참");
        assert!(input.key_held(KeyCode::KeyW), "held 는 유지");

        input.on_key(KeyCode::KeyW, false);
        assert!(input.key_released(KeyCode::KeyW));
        assert!(!input.key_held(KeyCode::KeyW));
    }

    #[test]
    fn auto_repeat_fires_pressed_only_once() {
        let mut input = Input::default();

        input.on_key(KeyCode::KeyA, true);
        input.end_frame();
        input.on_key(KeyCode::KeyA, true); // OS 키 반복
        assert!(
            !input.key_pressed(KeyCode::KeyA),
            "키 반복은 pressed 를 다시 발생시키면 안 된다"
        );
    }

    #[test]
    fn focus_loss_releases_everything() {
        let mut input = Input::default();
        input.on_key(KeyCode::ShiftLeft, true);
        input.on_button(MouseButton::Left, true);
        input.end_frame();

        input.on_focus_lost();
        assert!(input.is_idle(), "포커스 상실 후 눌림 상태가 남아 있음");
        assert!(input.key_released(KeyCode::ShiftLeft));
        assert!(input.button_released(MouseButton::Left));
    }

    #[test]
    fn cursor_delta_is_relative_and_cleared() {
        let mut input = Input::default();
        input.on_cursor(10.0, 10.0);
        input.end_frame();

        input.on_cursor(13.0, 7.0);
        assert_eq!(input.cursor(), (13.0, 7.0));
        assert_eq!(input.cursor_delta(), (3.0, -3.0));

        input.end_frame();
        assert_eq!(input.cursor_delta(), (0.0, 0.0));
    }

    #[test]
    fn wheel_accumulates_within_frame() {
        let mut input = Input::default();
        input.on_wheel(1.0);
        input.on_wheel(0.5);
        assert!((input.wheel() - 1.5).abs() < 1e-6);

        input.end_frame();
        assert!(input.wheel().abs() < 1e-6);
    }
}
