//! 진영과 관계 — 서버 `FactionTypes.h` / `FactionRegistry` 와 같은 규칙.
//!
//! - 관계는 **대칭**이다: `(A, B)` 를 정하면 `(B, A)` 도 같다.
//! - 같은 진영은 항상 우호다 — 단 [`FactionId::NONE`] 끼리는 제외 (진영 없음은 "같은 편" 이 아니다).
//! - 정하지 않은 쌍은 **중립**이다.
//!
//! 서버와 다른 점: 싱글턴이 아니라 [`SimWorld`](crate::SimWorld) 가 소유하는 데이터다.
//! 테스트마다 다른 관계표를 쓸 수 있고, 존마다 달리 둘 수도 있다.

use std::collections::HashMap;

/// 진영 번호. 서버 `EFactionId` 와 같은 ID 공간 — 숫자 범위에 특별한 의미는 없다.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FactionId(pub u32);

impl FactionId {
    /// 진영 없음. 서버 `EFactionId::NONE`.
    pub const NONE: Self = Self(0);
}

/// 두 진영의 관계. 서버 `EFactionRelation`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Relation {
    /// 공격 가능. 공격형 AI 가 먼저 덤빈다.
    Hostile,
    /// 먼저 덤비지 않는다. 공격받으면 반격한다(AI 종류에 따라).
    Neutral,
    /// 공격 불가.
    Friendly,
}

/// 진영 관계표.
#[derive(Clone, Debug, Default)]
pub struct FactionTable {
    /// `(작은 번호, 큰 번호)` 한 쌍만 저장해 대칭을 보장한다.
    relations: HashMap<(FactionId, FactionId), Relation>,
}

impl FactionTable {
    fn key(a: FactionId, b: FactionId) -> (FactionId, FactionId) {
        if a <= b { (a, b) } else { (b, a) }
    }

    /// 관계를 정한다. 같은 진영끼리는 정할 수 없다 (항상 우호, 또는 NONE 끼리는 중립).
    pub fn set(&mut self, a: FactionId, b: FactionId, relation: Relation) {
        if a == b {
            return;
        }
        self.relations.insert(Self::key(a, b), relation);
    }

    #[must_use]
    pub fn get(&self, a: FactionId, b: FactionId) -> Relation {
        if a == b && a != FactionId::NONE {
            return Relation::Friendly;
        }
        self.relations
            .get(&Self::key(a, b))
            .copied()
            .unwrap_or(Relation::Neutral)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALLIANCE: FactionId = FactionId(1);
    const HORDE: FactionId = FactionId(2);
    const WILD: FactionId = FactionId(201);

    #[test]
    fn relations_are_symmetric() {
        let mut t = FactionTable::default();
        t.set(ALLIANCE, HORDE, Relation::Hostile);
        assert_eq!(t.get(ALLIANCE, HORDE), Relation::Hostile);
        assert_eq!(t.get(HORDE, ALLIANCE), Relation::Hostile);
        // 뒤집어 다시 정하면 같은 칸을 덮어쓴다.
        t.set(HORDE, ALLIANCE, Relation::Friendly);
        assert_eq!(t.get(ALLIANCE, HORDE), Relation::Friendly);
    }

    #[test]
    fn unset_pairs_are_neutral() {
        let t = FactionTable::default();
        assert_eq!(t.get(ALLIANCE, WILD), Relation::Neutral);
    }

    #[test]
    fn same_faction_is_friendly_except_none() {
        let mut t = FactionTable::default();
        assert_eq!(t.get(WILD, WILD), Relation::Friendly);
        assert_eq!(t.get(FactionId::NONE, FactionId::NONE), Relation::Neutral);
        // 같은 진영 관계는 바꿀 수 없다.
        t.set(WILD, WILD, Relation::Hostile);
        assert_eq!(t.get(WILD, WILD), Relation::Friendly);
    }
}
