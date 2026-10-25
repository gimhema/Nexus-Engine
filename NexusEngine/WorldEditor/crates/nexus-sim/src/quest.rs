//! 퀘스트 — **단계 목록 + 목표 + 보상** (R5).
//!
//! ```text
//! 받기 ── NPC 기능 Quest(N) 에게 AcceptQuest   또는  트리거 효과 StartQuest(N)  (월드 퀘스트)
//!   │
//!   ▼  단계 0: 목표들 (처치·수집·도달·대화·조건)  ── 다 채우면 그 단계의 효과(then) → 다음 단계
//!   ▼  단계 1: …
//!   ▼  마지막 단계를 채우면 "보고 가능"  ── 담당 NPC 에게 TurnInQuest → 보상(rewards)
//!                                         (auto_complete 면 그 자리에서 보상 — 담당 NPC 가 없는 퀘스트)
//! ```
//!
//! # 판정 체계를 새로 만들지 않는다
//!
//! 받을 수 있는 조건(`requires`)과 조건 목표는 R1 의 [`Condition`], 단계 효과·보상은 R1 의 [`Effect`] 다.
//! 목표를 세는 재료도 트리거와 **같은 tick 끝의 같은 이벤트**다(처치·들어옴·말 걸기). 그래서 퀘스트가 할 수
//! 있는 일은 트리거도 할 수 있고, 규칙이 두 벌로 갈라지지 않는다.
//!
//! # 목표는 둘로 나뉜다
//!
//! - **세는 목표** — 처치·도달·대화. 그 단계가 진행 중일 때 일어난 것만 센다(받기 전에 잡은 것은 세지 않는다).
//! - **상태 목표** — 수집(가방에 N 개)·조건. 매 tick 지금 상태를 본다. 수집은 `take` 면 보고할 때 가져간다.
//!
//! # 저장
//!
//! 퀘스트 기록은 캐릭터의 것이다(`Unit::quests`) — 진행 중이면 단계와 센 수, 끝났으면 완료 횟수.
//! MMO 에서는 캐릭터 DB 에 들어갈 자리다.

use std::collections::BTreeMap;

use nexus_core::Vec2;

use crate::item::ItemId;
use crate::trigger::{Condition, Effect};
use crate::unit::ActorType;

/// 퀘스트 번호. 서버와 **같은 ID 공간**이다.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct QuestId(pub u32);

/// 목표 하나.
#[derive(Clone, Debug, PartialEq)]
pub enum Objective {
    /// 그 액터 타입을(없으면 아무거나) `count` 번 쓰러뜨린다.
    Kill {
        actor: Option<ActorType>,
        count: u32,
    },
    /// 두 가방을 합쳐 `count` 개 가지고 있다. `take` 면 보고(완료)할 때 가져간다.
    Collect {
        item: ItemId,
        count: u32,
        take: bool,
    },
    /// 원 안으로 들어간다.
    Reach { center: Vec2, radius: f32 },
    /// 그 액터 타입의 NPC 에게 말을 건다.
    Talk { actor: ActorType },
    /// 조건이 참이다 (R1 Condition — 플래그·레벨·스킬 …).
    Check(Condition),
}

impl Objective {
    /// 세는 목표의 목표 수. 상태 목표는 `None`.
    #[must_use]
    pub fn target(&self) -> Option<u32> {
        match self {
            Self::Kill { count, .. } => Some((*count).max(1)),
            Self::Reach { .. } | Self::Talk { .. } => Some(1),
            Self::Collect { .. } | Self::Check(_) => None,
        }
    }
}

/// 단계 하나 — 목표를 **전부** 채우면 `then` 을 적용하고 다음 단계로.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stage {
    pub objectives: Vec<Objective>,
    pub then: Vec<Effect>,
}

/// 퀘스트 하나.
#[derive(Clone, Debug, PartialEq)]
pub struct QuestDef {
    pub stages: Vec<Stage>,
    /// 받을 수 있는 조건 — 레벨·선행 퀘스트(`Condition::Quest`)·플래그 …
    pub requires: Condition,
    /// 완료할 때의 보상.
    pub rewards: Vec<Effect>,
    /// 끝난 뒤 다시 받을 수 있다. 일일·주간 주기는 단계 2(서버 시계)에서 — 지금은 "언제든 다시".
    pub repeatable: bool,
    /// 마지막 단계를 채우면 NPC 에게 보고하지 않고 그 자리에서 끝난다 — 담당 NPC 가 없는 월드 퀘스트.
    pub auto_complete: bool,
}

impl Default for QuestDef {
    fn default() -> Self {
        Self {
            stages: Vec::new(),
            requires: Condition::Always,
            rewards: Vec::new(),
            repeatable: false,
            auto_complete: false,
        }
    }
}

/// 진행 중인 퀘스트.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Active {
    /// 지금 단계. 단계 수와 같으면 **보고 가능**이다.
    pub stage: u16,
    /// 지금 단계의 목표별 센 수 (상태 목표 자리는 0).
    pub counts: Vec<u32>,
}

/// 퀘스트 하나의 기록.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuestEntry {
    pub active: Option<Active>,
    /// 끝낸 횟수.
    pub completed: u32,
}

/// 퀘스트 상태 — 조건(`Condition::Quest`)과 UI 가 쓴다.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuestState {
    /// 받은 적 없음 (끝낸 적도 없음).
    NotStarted,
    /// 진행 중 (보고 가능 포함).
    Active,
    /// 마지막 단계까지 채웠고 보고만 남았다.
    Ready,
    /// 한 번 이상 끝냈다 (지금 다시 진행 중이 아닐 때).
    Completed,
}

/// 캐릭터 하나의 퀘스트 기록 — 번호 순.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuestLog(pub(crate) BTreeMap<QuestId, QuestEntry>);

impl QuestLog {
    #[must_use]
    pub fn entry(&self, quest: QuestId) -> Option<&QuestEntry> {
        self.0.get(&quest)
    }

    /// 기록 전부 — 저장·UI.
    pub fn iter(&self) -> impl Iterator<Item = (QuestId, &QuestEntry)> {
        self.0.iter().map(|(q, e)| (*q, e))
    }

    /// 퀘스트 상태. `stages` 는 그 퀘스트의 단계 수(보고 가능 판정용).
    #[must_use]
    pub fn state(&self, quest: QuestId, stages: usize) -> QuestState {
        match self.0.get(&quest) {
            Some(QuestEntry {
                active: Some(a), ..
            }) if usize::from(a.stage) >= stages => QuestState::Ready,
            Some(QuestEntry {
                active: Some(_), ..
            }) => QuestState::Active,
            Some(e) if e.completed > 0 => QuestState::Completed,
            _ => QuestState::NotStarted,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use nexus_core::Entity;

    use super::*;
    use crate::trigger::{On, TriggerDef, TriggerId};
    use crate::{
        Authority, BagKind, CurrencyId, Event, FactionId, Intent, ItemDef, ItemKind,
        LocalAuthority, LootEntry, LootTableId, Rejection, Service, SimWorld, SkillDef, SkillId,
        Tile, TileMap, UnitDef,
    };

    const DT: Duration = Duration::from_millis(50);
    const HIT: SkillId = SkillId(1);
    const GOLD: CurrencyId = CurrencyId(1);
    const JELLY: ItemId = ItemId(909);
    const SLIME: ActorType = ActorType(100);
    const ELDER: ActorType = ActorType(20);
    const HUNT: QuestId = QuestId(1);
    const ERRAND: QuestId = QuestId(2);
    const SEQUEL: QuestId = QuestId(3);
    const SCOUT: QuestId = QuestId(4);

    fn gold(amount: u32) -> Effect {
        Effect::GiveCurrency {
            currency: GOLD,
            amount,
        }
    }

    /// 영웅 + 촌장(퀘스트 1·2·3) + 퀘스트 넷.
    fn village() -> (LocalAuthority, Entity, Entity) {
        let mut w = SimWorld::new(TileMap::new(40, 40, 1.0, Vec2::ZERO, Tile::default()));
        w.define_skill(
            HIT,
            SkillDef {
                range: 3.0,
                cooldown_ms: 0,
                damage_mult: 1.0,
                mp_cost: 0,
                radius: 0.0,
                heal: 0,
            },
        );
        w.define_item(
            JELLY,
            ItemDef {
                kind: ItemKind::Consumable { heal: 1, mana: 0 },
                max_stack: 99,
            },
        );
        w.define_loot(
            LootTableId(1),
            vec![LootEntry {
                item: JELLY,
                count: 2,
                chance_per_mille: 1000,
            }],
        );
        // 1: 슬라임 2 → 보고 → 50 골드 + 드롭 표 1(젤리 2).
        w.define_quest(
            HUNT,
            QuestDef {
                stages: vec![Stage {
                    objectives: vec![Objective::Kill {
                        actor: Some(SLIME),
                        count: 2,
                    }],
                    then: Vec::new(),
                }],
                rewards: vec![gold(50), Effect::GiveLoot(LootTableId(1))],
                ..QuestDef::default()
            },
        );
        // 2: 촌장과 이야기 → 젤리 3 을 가져온다(보고할 때 가져감). 반복 가능.
        w.define_quest(
            ERRAND,
            QuestDef {
                stages: vec![
                    Stage {
                        objectives: vec![Objective::Talk { actor: ELDER }],
                        then: vec![gold(1)],
                    },
                    Stage {
                        objectives: vec![Objective::Collect {
                            item: JELLY,
                            count: 3,
                            take: true,
                        }],
                        then: Vec::new(),
                    },
                ],
                rewards: vec![gold(10)],
                repeatable: true,
                ..QuestDef::default()
            },
        );
        // 3: 1 을 끝내야 받는다.
        w.define_quest(
            SEQUEL,
            QuestDef {
                stages: vec![Stage::default()],
                requires: Condition::Quest {
                    quest: HUNT,
                    state: QuestState::Completed,
                },
                ..QuestDef::default()
            },
        );
        // 4: 월드 퀘스트 — 트리거가 시작하고, 구역에 들어가면 그 자리에서 끝난다.
        w.define_quest(
            SCOUT,
            QuestDef {
                stages: vec![Stage {
                    objectives: vec![Objective::Reach {
                        center: Vec2::new(20.5, 5.5),
                        radius: 1.0,
                    }],
                    then: Vec::new(),
                }],
                rewards: vec![gold(7)],
                auto_complete: true,
                ..QuestDef::default()
            },
        );
        w.define_services(
            ELDER,
            vec![
                Service::Quest(HUNT),
                Service::Quest(ERRAND),
                Service::Quest(SEQUEL),
            ],
        );
        let me = w.spawn_unit(
            Vec2::new(5.5, 5.5),
            0.0,
            UnitDef {
                max_hp: 100,
                attack: 100,
                move_speed: 4.0,
                faction: FactionId(1),
                ..UnitDef::default()
            },
        );
        let elder = w.spawn_unit(
            Vec2::new(6.5, 5.5),
            0.0,
            UnitDef {
                actor: ELDER,
                faction: FactionId(100),
                ..UnitDef::default()
            },
        );
        (LocalAuthority::new(w), me, elder)
    }

    fn run(auth: &mut LocalAuthority, intent: Intent) -> Vec<Event> {
        auth.submit(intent);
        auth.tick(DT)
    }

    fn rejected(events: &[Event]) -> Option<Rejection> {
        events.iter().find_map(|e| match e {
            Event::Rejected { reason, .. } => Some(*reason),
            _ => None,
        })
    }

    fn kill_slime(auth: &mut LocalAuthority, me: Entity) -> Vec<Event> {
        let slime = auth.world_mut().spawn_unit(
            Vec2::new(5.5, 6.5),
            0.0,
            UnitDef {
                actor: SLIME,
                ..UnitDef::default()
            },
        );
        run(
            auth,
            Intent::Attack {
                unit: me,
                target: slime,
                skill: HIT,
            },
        )
    }

    fn purse(auth: &LocalAuthority, me: Entity) -> u64 {
        auth.world().unit(me).unwrap().wallet().get(GOLD)
    }

    fn jelly(auth: &LocalAuthority, me: Entity) -> u32 {
        auth.world()
            .unit(me)
            .unwrap()
            .inventory()
            .bag(BagKind::Consumable)
            .count_of(JELLY)
    }

    #[test]
    fn accept_hunt_and_turn_in_for_rewards() {
        let (mut auth, me, elder) = village();
        kill_slime(&mut auth, me);
        let accepted = run(
            &mut auth,
            Intent::AcceptQuest {
                unit: me,
                npc: elder,
                quest: HUNT,
            },
        );
        assert!(accepted.contains(&Event::QuestAccepted {
            unit: me,
            quest: HUNT
        }));
        let first = kill_slime(&mut auth, me);
        assert!(
            first.contains(&Event::QuestProgressed {
                unit: me,
                quest: HUNT,
                objective: 0,
                count: 1
            }),
            "받기 전에 잡은 한 마리는 세지 않는다"
        );
        let second = kill_slime(&mut auth, me);
        assert!(second.contains(&Event::QuestReady {
            unit: me,
            quest: HUNT
        }));
        assert_eq!(auth.world().quest_state(me, HUNT), QuestState::Ready);
        let done = run(
            &mut auth,
            Intent::TurnInQuest {
                unit: me,
                npc: elder,
                quest: HUNT,
            },
        );
        assert!(done.contains(&Event::QuestCompleted {
            unit: me,
            quest: HUNT
        }));
        assert_eq!(
            (purse(&auth, me), jelly(&auth, me)),
            (50, 2),
            "골드 + 드롭 표"
        );
        assert_eq!(
            rejected(&run(
                &mut auth,
                Intent::AcceptQuest {
                    unit: me,
                    npc: elder,
                    quest: HUNT
                }
            )),
            Some(Rejection::QuestUnavailable),
            "반복 불가"
        );
        // 선행 퀘스트를 끝냈으니 3 을 받을 수 있다.
        assert!(auth.world().can_start_quest(me, SEQUEL));
    }

    #[test]
    fn a_prerequisite_blocks_and_turning_in_early_is_refused() {
        let (mut auth, me, elder) = village();
        assert_eq!(
            rejected(&run(
                &mut auth,
                Intent::AcceptQuest {
                    unit: me,
                    npc: elder,
                    quest: SEQUEL
                }
            )),
            Some(Rejection::QuestUnavailable)
        );
        run(
            &mut auth,
            Intent::AcceptQuest {
                unit: me,
                npc: elder,
                quest: HUNT,
            },
        );
        assert_eq!(
            rejected(&run(
                &mut auth,
                Intent::TurnInQuest {
                    unit: me,
                    npc: elder,
                    quest: HUNT
                }
            )),
            Some(Rejection::QuestNotReady)
        );
        let gone = run(
            &mut auth,
            Intent::AbandonQuest {
                unit: me,
                quest: HUNT,
            },
        );
        assert!(gone.contains(&Event::QuestAbandoned {
            unit: me,
            quest: HUNT
        }));
        assert_eq!(auth.world().quest_state(me, HUNT), QuestState::NotStarted);
    }

    #[test]
    fn talk_then_collect_takes_the_items_and_can_repeat() {
        let (mut auth, me, elder) = village();
        run(
            &mut auth,
            Intent::AcceptQuest {
                unit: me,
                npc: elder,
                quest: ERRAND,
            },
        );
        // 말 걸기가 1단계를 채운다 (단계 효과 1 골드).
        let talk = run(
            &mut auth,
            Intent::Interact {
                unit: me,
                npc: elder,
            },
        );
        assert!(talk.contains(&Event::QuestStageDone {
            unit: me,
            quest: ERRAND,
            stage: 0
        }));
        assert_eq!(purse(&auth, me), 1);
        // 젤리 3 개를 가지면 그 tick 에 보고 가능.
        assert!(auth.world_mut().give_item(me, JELLY, 4));
        auth.tick(DT);
        assert_eq!(auth.world().quest_state(me, ERRAND), QuestState::Ready);
        // 보고 직전에 젤리를 내려놓으면 다시 모자라다.
        run(
            &mut auth,
            Intent::DropItem {
                unit: me,
                bag: BagKind::Consumable,
                slot: 0,
            },
        );
        assert_eq!(
            rejected(&run(
                &mut auth,
                Intent::TurnInQuest {
                    unit: me,
                    npc: elder,
                    quest: ERRAND
                }
            )),
            Some(Rejection::QuestNotReady)
        );
        assert!(auth.world_mut().give_item(me, JELLY, 4));
        run(
            &mut auth,
            Intent::TurnInQuest {
                unit: me,
                npc: elder,
                quest: ERRAND,
            },
        );
        assert_eq!(
            (purse(&auth, me), jelly(&auth, me)),
            (11, 1),
            "젤리 3 을 가져갔다"
        );
        assert!(auth.world().can_start_quest(me, ERRAND), "반복 가능");
    }

    #[test]
    fn a_world_quest_starts_from_a_trigger_and_completes_on_the_spot() {
        let (mut auth, me, _) = village();
        auth.world_mut().define_trigger(
            TriggerId(1),
            TriggerDef {
                on: On::Entered {
                    center: Vec2::new(10.5, 5.5),
                    radius: 1.0,
                },
                when: Condition::Always,
                then: vec![Effect::StartQuest(SCOUT)],
            },
        );
        auth.submit(Intent::MoveTo {
            unit: me,
            target: Vec2::new(25.5, 5.5),
        });
        let mut seen = Vec::new();
        for _ in 0..200 {
            seen.extend(auth.tick(DT));
        }
        assert!(seen.contains(&Event::QuestAccepted {
            unit: me,
            quest: SCOUT
        }));
        assert!(seen.contains(&Event::QuestCompleted {
            unit: me,
            quest: SCOUT
        }));
        assert_eq!(purse(&auth, me), 7);
        assert!(
            !seen.iter().any(|e| matches!(e, Event::QuestReady { .. })),
            "보고 없이 끝난다"
        );
    }

    #[test]
    fn states_follow_the_log() {
        let q = QuestId(1);
        let mut log = QuestLog::default();
        assert_eq!(log.state(q, 2), QuestState::NotStarted);
        log.0.insert(
            q,
            QuestEntry {
                active: Some(Active {
                    stage: 1,
                    counts: vec![0],
                }),
                completed: 0,
            },
        );
        assert_eq!(log.state(q, 2), QuestState::Active);
        log.0.get_mut(&q).unwrap().active.as_mut().unwrap().stage = 2;
        assert_eq!(log.state(q, 2), QuestState::Ready);
        log.0.insert(
            q,
            QuestEntry {
                active: None,
                completed: 1,
            },
        );
        assert_eq!(log.state(q, 2), QuestState::Completed);
    }
}
