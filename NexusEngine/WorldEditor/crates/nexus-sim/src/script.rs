//! 스크립트 훅 — 액터 스크립트를 돌리는 쪽과 시뮬레이션의 경계 (단계 2 P2).
//!
//! 시뮬레이션은 스크립트 언어를 모른다. [`ScriptHost`] 트레이트만 두고, 구현(Rhai)은
//! 다른 크레이트(`nexus-script`)가 한다 — `nexus-sim` 의 의존성이 늘지 않는다.
//!
//! # 규칙
//!
//! - 스크립트는 **월드를 읽기만 한다.** 하고 싶은 일은 AI 와 똑같이 [`Intent`] 로 낸다 —
//!   그래서 사거리·쿨타임·진영 판정을 똑같이 받고, 권한자가 원격이 되어도 구조가 같다.
//! - [`LocalAuthority`](crate::LocalAuthority) 의 tick 순서:
//!   플레이어 Intent → AI → **스크립트** → 이동. 같은 유닛에 대해서는 나중 Intent 가 이기므로
//!   스크립트가 AI 의 결정을 덮을 수 있다.
//! - 호스트는 **이전 tick 의 이벤트**를 받는다 (이번 tick 의 이동·전투는 아직 일어나지 않았다).
//! - 스크립트 Intent 의 거절은 호출자에게 알리지 않는다 — AI Intent 와 같다.
//! - 결정성: 호스트는 월드의 유닛 순서대로 돌고, 난수는 시드 있는 것만 쓴다.

use std::fmt::Debug;
use std::time::Duration;

use crate::authority::{Event, Intent};
use crate::world::SimWorld;

/// 액터 스크립트를 돌리는 쪽.
pub trait ScriptHost: Debug {
    /// tick 마다 한 번. `events` 는 이전 tick 에 일어난 일, 결과는 `out` 에 Intent 로 담는다.
    fn run(&mut self, world: &SimWorld, dt: Duration, events: &[Event], out: &mut Vec<Intent>);

    /// 쌓인 스크립트 메시지(`print`·오류)를 꺼낸다. 화면에 띄우는 것은 호출한 쪽의 일이다.
    fn drain_log(&mut self) -> Vec<ScriptLog> {
        Vec::new()
    }
}

/// 스크립트가 남긴 메시지 하나.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptLog {
    /// 오류면 `true` — 그 스크립트는 꺼졌다.
    pub error: bool,
    pub message: String,
}
