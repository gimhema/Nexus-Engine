//! 엔진 코어 — 수학, 카메라, 엔티티, 시뮬레이션 타이밍.
//!
//! **이 크레이트는 GPU API 도, 네트워크 계층도 알지 못한다.** 순수 로직만 두어
//! 단위 테스트가 가능하게 유지한다.

#![forbid(unsafe_code)]

pub mod camera;
pub mod time;
pub mod units;

/// 수학 타입은 `glam` 을 그대로 쓴다.
///
/// 직접 구현하면 연산자·행렬·쿼터니언·SIMD 로 끝없이 이어지고, 그래픽 생태계와의
/// 호환도 잃는다. 자체 엔진에서 직접 만들 가치가 있는 것은 씬·렌더러이지
/// 벡터 연산이 아니다. (CLAUDE.md 핵심 원칙 5)
///
/// 월드 좌표계는 **미터(m), Z-up** 이다. 전체 규약은 [`units`] 모듈 참고.
pub use glam::{self, Mat4, Quat, Vec2, Vec3, Vec4};

pub use camera::Camera2d;

/// 씬 엔티티 핸들.
///
/// `generation` 은 슬롯 재사용 시 옛 핸들을 무효화하기 위한 것이다.
/// 이것이 없으면 despawn 후 같은 인덱스가 재할당됐을 때 옛 핸들이 엉뚱한
/// 엔티티를 가리킨다 — 나중에 고치려면 엔티티를 만지는 모든 코드가 영향을 받는다.
///
/// 서버가 발급하는 `pawnId` / `sessionId` 는 이 타입이 아니라 별도의
/// `NetworkId(u64)` 로 다룬다 (M9). 로컬 핸들과 서버 ID 를 섞지 않는다.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Entity {
    index: u32,
    generation: u32,
}

impl Entity {
    /// 저장소 슬롯 번호.
    #[must_use]
    pub fn index(self) -> u32 {
        self.index
    }

    /// 슬롯 재사용 횟수.
    #[must_use]
    pub fn generation(self) -> u32 {
        self.generation
    }
}

/// 엔티티 저장소.
///
/// M6 에서 컴포넌트 저장소(직접 구현 또는 `hecs` 도입)로 확장한다.
/// 지금은 핸들 발급과 유효성 검사만 담당한다.
#[derive(Debug, Default)]
pub struct World {
    /// 슬롯별 세대. 홀수 = 살아 있음, 짝수 = 비어 있음.
    generations: Vec<u32>,
    /// 재사용 가능한 슬롯 번호.
    free: Vec<u32>,
    alive: u32,
}

impl World {
    /// 새 엔티티를 만든다.
    pub fn spawn(&mut self) -> Entity {
        self.alive += 1;

        if let Some(index) = self.free.pop() {
            // `gen` 은 edition 2024 예약어라 쓸 수 없다.
            let slot = &mut self.generations[index as usize];
            *slot += 1; // 짝수 → 홀수: 살아남
            return Entity {
                index,
                generation: *slot,
            };
        }

        let index =
            u32::try_from(self.generations.len()).expect("엔티티 슬롯이 u32 범위를 넘었습니다");
        self.generations.push(1);
        Entity {
            index,
            generation: 1,
        }
    }

    /// 엔티티를 제거한다. 이미 죽었거나 낡은 핸들이면 `false`.
    pub fn despawn(&mut self, entity: Entity) -> bool {
        if !self.is_alive(entity) {
            return false;
        }
        self.generations[entity.index as usize] += 1; // 홀수 → 짝수: 죽음
        self.free.push(entity.index);
        self.alive -= 1;
        true
    }

    /// 핸들이 아직 유효한가.
    #[must_use]
    pub fn is_alive(&self, entity: Entity) -> bool {
        self.generations
            .get(entity.index as usize)
            .is_some_and(|&g| g == entity.generation && g % 2 == 1)
    }

    /// 살아 있는 엔티티 수.
    #[must_use]
    pub fn entity_count(&self) -> u32 {
        self.alive
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_then_despawn_updates_count() {
        let mut w = World::default();
        let a = w.spawn();
        let b = w.spawn();
        assert_eq!(w.entity_count(), 2);

        assert!(w.despawn(a));
        assert_eq!(w.entity_count(), 1);
        assert!(w.is_alive(b));
    }

    #[test]
    fn stale_handle_is_rejected_after_slot_reuse() {
        let mut w = World::default();
        let old = w.spawn();
        w.despawn(old);

        let new = w.spawn(); // 같은 슬롯 재사용
        assert_eq!(
            old.index(),
            new.index(),
            "슬롯이 재사용되어야 테스트가 의미 있음"
        );
        assert_ne!(old.generation(), new.generation());

        assert!(!w.is_alive(old), "낡은 핸들이 살아 있다고 보고됨");
        assert!(w.is_alive(new));
    }

    #[test]
    fn double_despawn_is_rejected() {
        let mut w = World::default();
        let e = w.spawn();
        assert!(w.despawn(e));
        assert!(!w.despawn(e), "중복 despawn 이 통과됨");
        assert_eq!(w.entity_count(), 0);
    }

    #[test]
    fn handles_are_unique_across_many_spawns() {
        let mut w = World::default();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..1000 {
            assert!(seen.insert(w.spawn()), "중복 핸들 발급");
        }
    }
}
