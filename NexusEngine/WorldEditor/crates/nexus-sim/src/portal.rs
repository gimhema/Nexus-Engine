//! 포털 — 밟으면 다른 자리·다른 레벨로 넘어가는 구역 (R6).
//!
//! ```text
//! 유닛이 구역 밖 → 안으로 들어옴 ──조건(requires)──▶ 같은 존: 옮긴다       Event::Teleported
//!                                    │               다른 레벨: 앱에 맡긴다  Event::Traveled
//!                                    └─ 거짓 ────────▶ Event::PortalBlocked (앱이 이유를 보여 준다)
//! ```
//!
//! - **들어오는 순간만 본다** — 구역 안에 서 있는 동안 매 tick 다시 넘기지 않는다. 그래서 도착 자리가
//!   다른 포털 위여도 곧바로 되돌아가지 않는다 (옮겨진 유닛은 그 tick 에 "들어온" 것이 아니다).
//! - **직업이 있는 유닛(플레이어)만** 넘어간다. 쫓아오던 몬스터가 포털을 밟아도 아무 일도 없다 —
//!   스킬 사용 제한(R3)과 같은 기준이다.
//! - 레벨을 여는 것은 앱의 일이다 — NPC 텔레포트(R4-2)와 같은 `Traveled { travel, pos }` 를 내고,
//!   `travel` 번호 ↔ 레벨 대응은 앱이 정한다. 0 이면 지금 존 안.
//! - 입장 조건은 R1 의 [`Condition`] 그대로다 — 던전 입장 레벨·선행 퀘스트·열쇠 아이템 등.
//!   비용이나 열쇠를 **가져가는** 일은 하지 않는다(그건 효과다). 필요하면 트리거로 붙인다.
//! - 판정 순서: tick 끝, 트리거 → 퀘스트 → **포털**. 포털 자리에 `Reach` 목표가 있어도 먼저 센다.

use nexus_core::Vec2;

use crate::trigger::Condition;

/// 포털 하나. 존을 읽을 때 앱이 등록한다 ([`crate::SimWorld::add_portal`]).
#[derive(Clone, Debug, PartialEq)]
pub struct PortalDef {
    /// 구역 중심 (m).
    pub center: Vec2,
    /// 구역 반경 (m).
    pub radius: f32,
    /// 0 이면 지금 존 안. 그 밖은 앱이 레벨과 짝지은 번호.
    pub travel: u32,
    /// 도착 위치 (m).
    pub pos: Vec2,
    /// 넘어갈 수 있는 조건. [`Condition::Always`] 면 누구나.
    pub requires: Condition,
}

impl PortalDef {
    /// `from` 에서 `to` 로 움직이며 구역에 **들어왔는가**.
    #[must_use]
    pub fn entered(&self, from: Vec2, to: Vec2) -> bool {
        let r2 = self.radius * self.radius;
        from.distance_squared(self.center) > r2 && to.distance_squared(self.center) <= r2
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use nexus_core::Entity;

    use super::*;
    use crate::{
        Authority, ClassId, Event, FactionId, Intent, LocalAuthority, SimWorld, Tile, TileMap,
        UnitDef,
    };

    const DT: Duration = Duration::from_millis(50);

    /// 플레이어(직업 있음) + 몬스터(직업 없음) + 포털 셋.
    fn field() -> (LocalAuthority, Entity, Entity) {
        let mut w = SimWorld::new(TileMap::new(40, 40, 1.0, Vec2::ZERO, Tile::default()));
        // 0: 같은 존 안 (10,5) → (30,30)
        w.add_portal(PortalDef {
            center: Vec2::new(10.5, 5.5),
            radius: 1.0,
            travel: 0,
            pos: Vec2::new(30.5, 30.5),
            requires: Condition::Always,
        });
        // 1: 다른 레벨 — 레벨 3 부터
        w.add_portal(PortalDef {
            center: Vec2::new(5.5, 15.5),
            radius: 1.0,
            travel: 2,
            pos: Vec2::new(1.0, 2.0),
            requires: Condition::Level {
                cmp: crate::Cmp::Ge,
                value: 3,
            },
        });
        // 2: 도착 자리에 놓인 포털 — 도착하자마자 되돌아가면 안 된다.
        w.add_portal(PortalDef {
            center: Vec2::new(30.5, 30.5),
            radius: 1.0,
            travel: 0,
            pos: Vec2::new(10.5, 5.5),
            requires: Condition::Always,
        });
        let me = w.spawn_unit(
            Vec2::new(5.5, 5.5),
            0.0,
            UnitDef {
                class: Some(ClassId(1)),
                move_speed: 5.0,
                faction: FactionId(1),
                ..UnitDef::default()
            },
        );
        let slime = w.spawn_unit(
            Vec2::new(10.5, 9.5),
            0.0,
            UnitDef {
                move_speed: 5.0,
                faction: FactionId(2),
                ..UnitDef::default()
            },
        );
        (LocalAuthority::new(w), me, slime)
    }

    /// `unit` 을 `target` 으로 걸려 보내고 tick 마다의 이벤트를 모은다.
    fn walk(auth: &mut LocalAuthority, unit: Entity, target: Vec2, ticks: usize) -> Vec<Event> {
        auth.submit(Intent::MoveTo { unit, target });
        (0..ticks).flat_map(|_| auth.tick(DT)).collect()
    }

    #[test]
    fn stepping_in_moves_the_player_once_and_not_back() {
        let (mut auth, me, _) = field();
        let events = walk(&mut auth, me, Vec2::new(10.5, 5.5), 40);
        let hops = events
            .iter()
            .filter(|e| matches!(e, Event::Teleported { unit, .. } if *unit == me))
            .count();
        assert_eq!(hops, 1, "도착 자리의 포털이 되돌려 보내면 안 된다");
        let pos = auth.world().unit(me).unwrap().pos;
        assert!(pos.distance(Vec2::new(30.5, 30.5)) < 0.01, "{pos}");
    }

    #[test]
    fn monsters_do_not_use_portals() {
        let (mut auth, _, slime) = field();
        let events = walk(&mut auth, slime, Vec2::new(10.5, 5.5), 40);
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::Teleported { .. } | Event::PortalBlocked { .. }))
        );
        let pos = auth.world().unit(slime).unwrap().pos;
        assert!(pos.distance(Vec2::new(10.5, 5.5)) < 0.01, "{pos}");
    }

    #[test]
    fn a_condition_blocks_and_a_level_portal_hands_over_to_the_app() {
        let (mut auth, me, _) = field();
        let events = walk(&mut auth, me, Vec2::new(5.5, 15.5), 60);
        assert!(events.contains(&Event::PortalBlocked {
            unit: me,
            portal: 1
        }));
        assert!(!events.iter().any(|e| matches!(e, Event::Traveled { .. })));

        // 레벨 3 이 되어 다시 들어오면 넘어간다 — 위치는 앱이 옮긴다.
        let (mut auth, me, _) = field();
        auth.world_mut()
            .set_progress(me, crate::Progress::new(3, 0));
        let events = walk(&mut auth, me, Vec2::new(5.5, 15.5), 60);
        assert!(events.contains(&Event::Traveled {
            unit: me,
            travel: 2,
            pos: Vec2::new(1.0, 2.0),
        }));
    }
}
