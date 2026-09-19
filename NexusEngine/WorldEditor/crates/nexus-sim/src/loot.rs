//! 드롭 테이블과 난수.
//!
//! **시뮬레이션은 결정적이어야 한다** — 같은 시드·같은 입력이면 같은 결과. 리플레이·자동화 테스트가
//! 그것을 요구하고, 단계 2 에서 서버와 결과를 대조할 때도 필요하다. 그래서:
//!
//! - 난수는 `SimWorld` 가 가진 시드 있는 생성기 하나만 쓴다 (전역·시간 기반 난수 금지).
//! - 확률은 **정수 천분율**이다. `f32` 비교는 플랫폼·컴파일러에 따라 경계에서 갈릴 수 있다.
//!
//! 서버에는 드롭 테이블이 아직 없다 (C++ 로드맵 Phase 3 "몬스터 드롭테이블" 미완).

use crate::item::ItemId;

/// 드롭 테이블 번호.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LootTableId(pub u32);

/// 드롭 한 줄. 줄마다 **따로** 굴린다 — 여러 줄이 한꺼번에 나올 수 있다.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LootEntry {
    pub item: ItemId,
    pub count: u32,
    /// 나올 확률, 천분율 (`1000` = 반드시). 1000 을 넘으면 1000 으로 본다.
    pub chance_per_mille: u32,
}

/// SplitMix64 — 작고 빠르고 품질이 충분하며, 의존성이 없다.
/// 암호용이 아니다. 게임 규칙의 재현성만 보장한다.
#[derive(Clone, Debug)]
pub(crate) struct Rng {
    state: u64,
}

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// `[0, 1000)` 균등. 나머지 연산의 치우침은 2⁶⁴ 에 비해 무시할 만하다.
    pub(crate) fn per_mille(&mut self) -> u32 {
        (self.next_u64() % 1000) as u32
    }
}

/// 테이블을 굴려 나온 것들.
pub(crate) fn roll(table: &[LootEntry], rng: &mut Rng) -> Vec<(ItemId, u32)> {
    table
        .iter()
        .filter(|e| e.count > 0)
        .filter(|e| rng.per_mille() < e.chance_per_mille.min(1000))
        .map(|e| (e.item, e.count))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(item: u32, chance: u32) -> LootEntry {
        LootEntry {
            item: ItemId(item),
            count: 1,
            chance_per_mille: chance,
        }
    }

    #[test]
    fn same_seed_same_sequence() {
        let (mut a, mut b) = (Rng::new(42), Rng::new(42));
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        assert_ne!(Rng::new(1).next_u64(), Rng::new(2).next_u64());
    }

    #[test]
    fn always_and_never() {
        let mut rng = Rng::new(7);
        let table = [entry(1, 1000), entry(2, 0), entry(3, 5000)];
        for _ in 0..200 {
            let got = roll(&table, &mut rng);
            assert_eq!(got, vec![(ItemId(1), 1), (ItemId(3), 1)]);
        }
    }

    #[test]
    fn chance_is_roughly_honoured() {
        let mut rng = Rng::new(123);
        let table = [entry(1, 250)];
        let hits = (0..10_000)
            .filter(|_| !roll(&table, &mut rng).is_empty())
            .count();
        assert!((2_300..2_700).contains(&hits), "25% 기대, {hits}/10000");
    }
}
