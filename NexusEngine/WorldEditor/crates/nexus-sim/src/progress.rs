//! 레벨과 경험치 (단계 2 P3).
//!
//! 서버 `CharacterEntityData` 와 같은 의미다 — 필요 경험치는 **`level² × 100`**,
//! 한 번에 여러 레벨이 오를 수 있다.
//!
//! 레벨이 오르면 **성장치**(`UnitDef::growth`)만큼 최대 HP·공격·방어가 늘어난다. 수치는 코드가
//! 아니라 데이터다 — 성장치를 적지 않은 액터(대부분의 몬스터)는 레벨이 올라도 수치가 그대로다.

/// 레벨 하나를 더 올리는 데 필요한 경험치. 서버와 같은 공식.
#[must_use]
pub fn exp_to_next(level: u32) -> u32 {
    level.saturating_mul(level).saturating_mul(100)
}

/// 레벨이 오를 때마다 더해지는 수치. 전부 0 이면 성장하지 않는다.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Growth {
    pub max_hp: u32,
    pub attack: u32,
    pub defense: u32,
}

/// 유닛 하나의 레벨·경험치.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    level: u32,
    /// 지금 레벨에서 모은 경험치 (다음 레벨까지 남은 양은 `exp_to_next(level) - exp`).
    exp: u32,
}

impl Default for Progress {
    fn default() -> Self {
        Self { level: 1, exp: 0 }
    }
}

impl Progress {
    /// 저장 파일에서 되살릴 때. 레벨 0 은 1 로 보정하고, 남은 경험치는 레벨 구간 안으로 자른다.
    #[must_use]
    pub fn new(level: u32, exp: u32) -> Self {
        let level = level.max(1);
        Self {
            level,
            exp: exp.min(exp_to_next(level).saturating_sub(1)),
        }
    }

    #[must_use]
    pub fn level(self) -> u32 {
        self.level
    }

    #[must_use]
    pub fn exp(self) -> u32 {
        self.exp
    }

    /// 다음 레벨까지 필요한 경험치.
    #[must_use]
    pub fn exp_to_next(self) -> u32 {
        exp_to_next(self.level)
    }

    /// 경험치를 더한다. **오른 레벨 수**를 돌려준다 (0 이면 그대로).
    pub fn add(&mut self, amount: u32) -> u32 {
        self.exp = self.exp.saturating_add(amount);
        let mut gained = 0;
        while self.exp >= self.exp_to_next() {
            self.exp -= self.exp_to_next();
            self.level += 1;
            gained += 1;
        }
        gained
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curve_matches_the_server_formula() {
        assert_eq!(
            [1, 2, 3, 10].map(exp_to_next),
            [100, 400, 900, 10_000],
            "level^2 × 100"
        );
    }

    #[test]
    fn experience_carries_over_and_can_gain_several_levels_at_once() {
        let mut p = Progress::default();
        assert_eq!((p.add(99), p.level(), p.exp()), (0, 1, 99));
        // 100 이면 딱 2레벨, 남는 경험치는 그대로 이어진다.
        assert_eq!((p.add(51), p.level(), p.exp()), (1, 2, 50));
        // 350(2→3 에 필요) + 900(3→4) 을 한 번에.
        assert_eq!((p.add(1250), p.level(), p.exp()), (2, 4, 0));
    }

    #[test]
    fn a_restored_progress_is_clamped_into_its_level() {
        assert_eq!(Progress::new(0, 0).level(), 1, "레벨 0 은 없다");
        let p = Progress::new(3, 5_000);
        assert_eq!((p.level(), p.exp()), (3, 899), "구간 밖 경험치는 잘린다");
    }
}
