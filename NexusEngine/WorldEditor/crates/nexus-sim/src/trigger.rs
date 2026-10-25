//! 진행 플래그와 트리거 — **언제(트리거) → 만약(조건) → 한다(효과)** (R1).
//!
//! ```text
//! on: Killed(actor: Some(100))   when: Actor(1)              then: [AddFlag(Character, 1, +1)]
//! on: LevelReached(5)            when: Flag(Character, 9 = 0) then: [GiveItem(1101 × 1), SetFlag(Character, 9, 1)]
//! ```
//!
//! 퀘스트 완료 판정·레벨 보상·NPC 기능 노출(그리고 나중의 대화 분기)은 전부 이 모양이다.
//! 그래서 [`Condition`] / [`Effect`] 를 **여기 한 곳에만** 정의하고 모든 메이커가 같은 어휘를 쓴다 —
//! 메이커마다 조건 판정을 따로 만들지 말 것.
//!
//! # 효과는 권한자 안에서만 일어난다
//!
//! 효과를 내는 [`Intent`](crate::Intent) 는 **없다.** 트리거는 AI 처럼 권한자 쪽에서 판정되고
//! (단계 2 에서는 서버), 효과는 거기서 바로 적용된다. 클라이언트가 "아이템을 줘" 라고
//! 보낼 길을 만들면 그대로 치트가 된다. 바뀐 것은 다른 결과처럼 [`Event`] 로 알린다.
//!
//! # 판정 순서 (결정적)
//!
//! - tick 마다 **모든 Intent·AI·스크립트·이동이 끝난 뒤** 그 tick 의 이벤트를 훑는다.
//! - 이벤트 순서대로, 이벤트마다 **트리거 번호 순서대로** 본다. 조건은 그 트리거를 볼 때
//!   판정하므로 앞 트리거가 바꾼 플래그를 뒤 트리거가 본다 (세기 → 보상 순으로 번호를 주면 된다).
//! - 효과가 낸 이벤트(경험치 → 레벨업)도 다시 트리거를 부른다. 다만 [`MAX_ROUNDS`] 번까지 —
//!   서로를 부르는 트리거가 무한히 돌지 않게.
//!
//! # 한 번만 일어나게 하려면
//!
//! 트리거에 "한 번만" 필드는 없다. **플래그로 막는다**: 조건에 `Not(Flag(… = 1))`, 효과에
//! `SetFlag(… 1)`. 그 플래그가 저장 파일에 남으므로 이어 해도 다시 일어나지 않는다.
//!
//! # 아직 없는 것
//!
//! 데이터로 안 되는 경우의 탈출구(`Effect::Script`)는 아직 없다 — 액터 스크립트 호스트는
//! 훅만 부른다. 필요해지면 UI 스크립트(P17)와 같은 `(path, call)` 모양으로 넣는다.

use std::collections::BTreeMap;

use nexus_core::{Entity, Vec2};

use crate::faction::FactionId;
use crate::item::ItemId;
use crate::unit::ActorType;

/// 효과가 효과를 부르는 연쇄의 최대 깊이. 넘는 이벤트는 트리거를 부르지 않는다.
pub const MAX_ROUNDS: u32 = 4;

/// 진행 플래그 번호. 서버와 **같은 ID 공간**이다 (퀘스트·스킬 번호와 같은 규칙).
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FlagId(pub u32);

/// 플래그가 누구의 것인가.
///
/// 단계 1(싱글)에서는 둘 다 같은 저장 파일에 들어가지만 **타입으로는 나눠 둔다** —
/// MMO 에서는 캐릭터 플래그는 캐릭터 DB 에, 월드 플래그는 서버에 있게 된다.
/// 계정 범위·인스턴스 범위는 단계 2 / R6 에서 늘린다.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FlagScope {
    /// 트리거 대상 유닛의 것 — 퀘스트 진행, 처치 수, 받은 보상.
    Character,
    /// 월드 전체의 것 — 다리가 고쳐졌다, 보스가 쓰러졌다.
    World,
}

/// 플래그 묶음. **없는 플래그는 0** 이고, 0 으로 만들면 지운다 — 저장 파일이 작게 유지된다.
///
/// 순서 있는 맵이라 순회(저장)가 결정적이다.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Flags(BTreeMap<FlagId, i32>);

impl Flags {
    #[must_use]
    pub fn get(&self, flag: FlagId) -> i32 {
        self.0.get(&flag).copied().unwrap_or(0)
    }

    /// 0 이 아닌 플래그 전부 — 번호 순.
    pub fn iter(&self) -> impl Iterator<Item = (FlagId, i32)> + '_ {
        self.0.iter().map(|(k, v)| (*k, *v))
    }

    pub(crate) fn set(&mut self, flag: FlagId, value: i32) {
        if value == 0 {
            self.0.remove(&flag);
        } else {
            self.0.insert(flag, value);
        }
    }
}

/// 비교 연산.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cmp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl Cmp {
    #[must_use]
    pub fn test(self, a: i64, b: i64) -> bool {
        match self {
            Self::Eq => a == b,
            Self::Ne => a != b,
            Self::Lt => a < b,
            Self::Le => a <= b,
            Self::Gt => a > b,
            Self::Ge => a >= b,
        }
    }
}

/// 만약 — **대상 유닛**(트리거를 일으킨 쪽)과 월드를 보고 참·거짓을 낸다. 상태를 바꾸지 않는다.
///
/// 대상이 없거나(디스폰) 죽어 있어도 판정한다 — 유닛을 보는 조건만 거짓이 된다.
#[derive(Clone, Debug, PartialEq)]
pub enum Condition {
    /// 늘 참.
    Always,
    Flag {
        scope: FlagScope,
        flag: FlagId,
        cmp: Cmp,
        value: i32,
    },
    /// 대상의 레벨.
    Level {
        cmp: Cmp,
        value: u32,
    },
    /// 대상의 두 가방을 합쳐 `count` 개 이상.
    HasItem {
        item: ItemId,
        count: u32,
    },
    /// 대상의 액터 타입.
    Actor(ActorType),
    /// 대상의 진영.
    Faction(FactionId),
    /// 전부 참 (빈 목록은 참).
    All(Vec<Condition>),
    /// 하나라도 참 (빈 목록은 거짓).
    Any(Vec<Condition>),
    Not(Box<Condition>),
    /// 대상의 그 퀘스트 상태가 `state` 다 (R5) — 선행 퀘스트·진행 중 분기.
    Quest {
        quest: crate::quest::QuestId,
        state: crate::quest::QuestState,
    },
    /// 대상의 지갑에 그 재화가 `amount` 이상 (R4).
    HasCurrency {
        currency: crate::trade::CurrencyId,
        amount: u64,
    },
    /// 대상이 그 스킬을 `value` 레벨과 비교해 (배우지 않았으면 0, R3).
    SkillLevel {
        skill: crate::combat::SkillId,
        cmp: Cmp,
        value: u32,
    },
}

/// 한다 — 대상 유닛(캐릭터 플래그·아이템·경험치)이나 월드(월드 플래그)를 바꾼다.
///
/// 효과는 실패해도 뒤의 효과를 막지 않는다. 그래서 **모자라면 안 되는 것은 조건으로 막는다**
/// (아이템을 받으려면 `TakeItem` 앞에 `HasItem` 조건).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    SetFlag {
        scope: FlagScope,
        flag: FlagId,
        value: i32,
    },
    /// 더한다 (넘치면 끝에서 멈춘다).
    AddFlag {
        scope: FlagScope,
        flag: FlagId,
        delta: i32,
    },
    /// 가방에 넣는다. **다 들어가지 않으면 발밑에 떨어뜨린다** — 보상을 잃지 않게.
    GiveItem { item: ItemId, count: u32 },
    /// 가방에서 뺀다. 모자라면 있는 만큼만 뺀다 (`HasItem` 조건으로 먼저 막을 것).
    TakeItem { item: ItemId, count: u32 },
    /// 경험치 — 레벨업 규칙은 처치 경험치와 같다. 죽은 대상은 받지 않는다.
    GiveExp(u32),
    /// 스킬을 배운다 — 이미 배웠으면 `levels` 만큼 올린다 (최대 레벨에서 멈춘다, R3).
    /// 직업·습득 조건이 맞지 않으면 아무 일도 없다 (다른 효과처럼 조용히).
    GrantSkill {
        skill: crate::combat::SkillId,
        levels: u32,
    },
    /// 스킬을 잊는다 (R3).
    RemoveSkill { skill: crate::combat::SkillId },
    /// 퀘스트를 시작한다 (R5) — 담당 NPC 가 없는 월드 퀘스트의 입구. 받을 수 없으면 조용히 지나간다.
    StartQuest(crate::quest::QuestId),
    /// 드롭 표를 굴려 대상의 가방에 넣는다 (R5) — 보상에 드롭 표를 다시 쓴다. 가방이 차면 발밑에.
    GiveLoot(crate::loot::LootTableId),
    /// 재화를 준다 (R4) — 퀘스트 보상 등.
    GiveCurrency {
        currency: crate::trade::CurrencyId,
        amount: u32,
    },
    /// 재화를 받는다 (R4). 모자라면 아무것도 빼지 않는다 — `HasCurrency` 조건으로 먼저 막을 것.
    TakeCurrency {
        currency: crate::trade::CurrencyId,
        amount: u32,
    },
}

/// 언제 — 이 tick 에 일어난 일 중 무엇에 반응하는가. 반응한 유닛이 **대상**이 된다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum On {
    /// 누군가를 쓰러뜨렸다 — 대상은 **쓰러뜨린 쪽**. `actor` 가 있으면 쓰러진 쪽의 타입이 같을 때만.
    Killed { actor: Option<ActorType> },
    /// 레벨 `n` 에 이르렀다 — 대상은 오른 쪽. 한 번에 여러 레벨이 올라 `n` 을 건너뛰어도 일어난다.
    LevelReached(u32),
    /// 땅의 아이템을 주웠다 — 대상은 주운 쪽. `item` 이 있으면 그 아이템일 때만.
    PickedUp { item: Option<ItemId> },
    /// 원 안으로 걸어 들어왔다 (지난 tick 에는 밖) — 대상은 들어온 유닛. 순간이동은 걸리지 않는다.
    Entered { center: Vec2, radius: f32 },
}

/// 트리거 번호 — 판정 순서이자 [`Event::TriggerFired`](crate::Event::TriggerFired) 의 이름.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TriggerId(pub u32);

/// 트리거 하나.
#[derive(Clone, Debug, PartialEq)]
pub struct TriggerDef {
    pub on: On,
    pub when: Condition,
    pub then: Vec<Effect>,
}

/// 이벤트 하나에서 읽어 낸 "일어난 일" — 트리거와 맞춰 볼 재료.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Happening {
    Killed {
        killer: Entity,
        victim: Option<ActorType>,
    },
    Leveled {
        unit: Entity,
        from: u32,
        to: u32,
    },
    PickedUp {
        unit: Entity,
        item: ItemId,
    },
    /// 이 tick 에 움직인 유닛 — `Entered` 판정용 (`from` = 지난 tick 위치).
    Moved {
        unit: Entity,
        from: Vec2,
        to: Vec2,
    },
    /// NPC 에게 말을 걸었다 — 퀘스트 대화 목표용 (R5). 트리거에는 아직 맞는 `On` 이 없다.
    Talked {
        unit: Entity,
        actor: ActorType,
    },
}

impl Happening {
    /// 이 일이 트리거에 맞으면 대상 유닛.
    pub(crate) fn matches(self, on: On) -> Option<Entity> {
        match (on, self) {
            (On::Killed { actor }, Self::Killed { killer, victim }) => {
                actor.is_none_or(|a| victim == Some(a)).then_some(killer)
            }
            (On::LevelReached(n), Self::Leveled { unit, from, to }) => {
                (from < n && n <= to).then_some(unit)
            }
            (On::PickedUp { item }, Self::PickedUp { unit, item: got }) => {
                item.is_none_or(|i| i == got).then_some(unit)
            }
            (On::Entered { center, radius }, Self::Moved { unit, from, to }) => {
                let r2 = radius * radius;
                (from.distance_squared(center) > r2 && to.distance_squared(center) <= r2)
                    .then_some(unit)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{
        Authority, Event, Intent, ItemDef, ItemKind, LocalAuthority, SimWorld, SkillDef, SkillId,
        Tile, TileMap, UnitDef,
    };

    const DT: Duration = Duration::from_millis(50);
    const HIT: SkillId = SkillId(1);
    const HERO: ActorType = ActorType(1);
    const SLIME: ActorType = ActorType(100);
    const POTION: ItemId = ItemId(501);
    const SWORD: ItemId = ItemId(1101);
    const KILLS: FlagId = FlagId(1);
    const REWARDED: FlagId = FlagId(2);

    fn char_flag(flag: FlagId, cmp: Cmp, value: i32) -> Condition {
        Condition::Flag {
            scope: FlagScope::Character,
            flag,
            cmp,
            value,
        }
    }

    /// 한 방에 쓰러뜨리는 영웅 하나 — 슬라임은 [`slime`] 으로 옆에 세운다.
    fn arena() -> (LocalAuthority, Entity) {
        let mut world = SimWorld::new(TileMap::new(20, 20, 1.0, Vec2::ZERO, Tile::default()));
        world.define_skill(
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
        world.define_item(
            POTION,
            ItemDef {
                kind: ItemKind::Consumable { heal: 10, mana: 0 },
                max_stack: 20,
            },
        );
        world.define_item(
            SWORD,
            ItemDef {
                kind: ItemKind::Equipment {
                    slot: crate::EquipSlot::Weapon,
                    attack: 5,
                    defense: 0,
                },
                max_stack: 1,
            },
        );
        let hero = world.spawn_unit(
            Vec2::new(5.5, 5.5),
            0.0,
            UnitDef {
                actor: HERO,
                max_hp: 100,
                attack: 100,
                move_speed: 4.0,
                ..UnitDef::default()
            },
        );
        (LocalAuthority::new(world), hero)
    }

    fn slime(auth: &mut LocalAuthority) -> Entity {
        auth.world_mut().spawn_unit(
            Vec2::new(6.5, 5.5),
            0.0,
            UnitDef {
                actor: SLIME,
                exp_reward: 10,
                ..UnitDef::default()
            },
        )
    }

    /// 슬라임 하나를 세우고 그 tick 에 쓰러뜨린다.
    fn kill_slime(auth: &mut LocalAuthority, hero: Entity) -> Vec<Event> {
        let target = slime(auth);
        auth.submit(Intent::Attack {
            unit: hero,
            target,
            skill: HIT,
        });
        let events = auth.tick(DT);
        assert!(events.iter().any(|e| matches!(e, Event::Died { .. })));
        events
    }

    fn fired(events: &[Event]) -> Vec<u32> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::TriggerFired { trigger, .. } => Some(trigger.0),
                _ => None,
            })
            .collect()
    }

    /// 1 = 슬라임 처치 세기, 2 = 세 마리째에 한 번만 보상.
    fn count_and_reward(auth: &mut LocalAuthority) {
        let w = auth.world_mut();
        w.define_trigger(
            TriggerId(1),
            TriggerDef {
                on: On::Killed { actor: Some(SLIME) },
                when: Condition::Actor(HERO),
                then: vec![Effect::AddFlag {
                    scope: FlagScope::Character,
                    flag: KILLS,
                    delta: 1,
                }],
            },
        );
        w.define_trigger(
            TriggerId(2),
            TriggerDef {
                on: On::Killed { actor: Some(SLIME) },
                when: Condition::All(vec![
                    char_flag(KILLS, Cmp::Ge, 3),
                    Condition::Not(Box::new(char_flag(REWARDED, Cmp::Eq, 1))),
                ]),
                then: vec![
                    Effect::GiveItem {
                        item: POTION,
                        count: 3,
                    },
                    Effect::SetFlag {
                        scope: FlagScope::Character,
                        flag: REWARDED,
                        value: 1,
                    },
                ],
            },
        );
    }

    #[test]
    fn a_kill_counter_rewards_once_on_the_third_kill() {
        let (mut auth, hero) = arena();
        count_and_reward(&mut auth);
        for kill in 1..=2 {
            let events = kill_slime(&mut auth, hero);
            assert_eq!(fired(&events), [1], "{kill}마리째는 세기만");
        }
        // 트리거 1 이 올린 값을 같은 tick 의 트리거 2 가 본다 (번호 순).
        let third = kill_slime(&mut auth, hero);
        assert_eq!(fired(&third), [1, 2]);
        assert!(third.contains(&Event::ItemReceived {
            unit: hero,
            stack: crate::ItemStack {
                item: POTION,
                count: 3
            },
        }));
        let fourth = kill_slime(&mut auth, hero);
        assert_eq!(
            fired(&fourth),
            [1],
            "보상은 플래그로 막혀 다시 나오지 않는다"
        );

        let u = auth.world().unit(hero).unwrap();
        assert_eq!((u.flag(KILLS), u.flag(REWARDED)), (4, 1));
        assert_eq!(
            u.inventory()
                .bag(crate::BagKind::Consumable)
                .count_of(POTION),
            3
        );
    }

    #[test]
    fn effects_can_chain_into_level_triggers_but_only_so_far() {
        let (mut auth, hero) = arena();
        let w = auth.world_mut();
        // 처치 → 경험치 100(1→2) → LevelReached(2) → 다시 경험치 → … 레벨마다 한 라운드씩.
        w.define_trigger(
            TriggerId(1),
            TriggerDef {
                on: On::Killed { actor: None },
                when: Condition::Always,
                then: vec![Effect::GiveExp(100)],
            },
        );
        for level in 2..=10 {
            w.define_trigger(
                TriggerId(level),
                TriggerDef {
                    on: On::LevelReached(level),
                    when: Condition::Always,
                    then: vec![Effect::GiveExp(crate::exp_to_next(level))],
                },
            );
        }
        let events = kill_slime(&mut auth, hero);
        // 처치 경험치 10 은 레벨을 올리지 않는다. 라운드 1: 처치 → +100 (레벨 2) · 라운드 2~4: 레벨 2·3·4.
        assert_eq!(
            fired(&events),
            [1, 2, 3, 4],
            "연쇄는 {MAX_ROUNDS} 라운드에서 끊긴다"
        );
        assert_eq!(auth.world().unit(hero).unwrap().progress().level(), 5);
    }

    #[test]
    fn a_full_bag_drops_the_reward_at_the_feet() {
        let (mut auth, hero) = arena();
        let w = auth.world_mut();
        assert!(
            w.give_item(hero, SWORD, crate::BAG_SLOTS as u32),
            "장비 가방을 꽉 채운다"
        );
        w.define_trigger(
            TriggerId(1),
            TriggerDef {
                on: On::Killed { actor: None },
                when: Condition::Always,
                then: vec![Effect::GiveItem {
                    item: SWORD,
                    count: 1,
                }],
            },
        );
        let events = kill_slime(&mut auth, hero);
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::ItemReceived { .. }))
        );
        let at_feet = auth.world().unit(hero).unwrap().pos();
        assert!(
            auth.world()
                .ground_items()
                .any(|(_, g)| g.stack.item == SWORD && g.pos == at_feet)
        );
    }

    #[test]
    fn taking_items_is_guarded_by_a_condition() {
        let (mut auth, hero) = arena();
        let w = auth.world_mut();
        assert!(w.give_item(hero, POTION, 2));
        // "포션 2개를 바치면 월드 플래그" — 모자라면 조건에서 막힌다.
        w.define_trigger(
            TriggerId(1),
            TriggerDef {
                on: On::Killed { actor: None },
                when: Condition::HasItem {
                    item: POTION,
                    count: 2,
                },
                then: vec![
                    Effect::TakeItem {
                        item: POTION,
                        count: 2,
                    },
                    Effect::AddFlag {
                        scope: FlagScope::World,
                        flag: FlagId(7),
                        delta: 1,
                    },
                ],
            },
        );
        let first = kill_slime(&mut auth, hero);
        assert!(first.contains(&Event::ItemTaken {
            unit: hero,
            item: POTION,
            count: 2
        }));
        assert!(first.contains(&Event::FlagChanged {
            unit: None,
            flag: FlagId(7),
            value: 1
        }));
        let second = kill_slime(&mut auth, hero);
        assert!(fired(&second).is_empty(), "포션이 없으니 일어나지 않는다");
        assert_eq!(auth.world().world_flag(FlagId(7)), 1);
    }

    #[test]
    fn walking_into_an_area_fires_once() {
        let (mut auth, hero) = arena();
        auth.world_mut().define_trigger(
            TriggerId(1),
            TriggerDef {
                on: On::Entered {
                    center: Vec2::new(12.5, 5.5),
                    radius: 1.0,
                },
                when: Condition::Actor(HERO),
                then: vec![Effect::SetFlag {
                    scope: FlagScope::World,
                    flag: FlagId(3),
                    value: 1,
                }],
            },
        );
        auth.submit(Intent::MoveTo {
            unit: hero,
            target: Vec2::new(14.5, 5.5),
        });
        let mut count = 0;
        for _ in 0..80 {
            count += fired(&auth.tick(DT)).len();
        }
        assert_eq!(
            count, 1,
            "지나가는 동안 한 번 — 원 안에 있는 동안 계속 일어나지 않는다"
        );
        assert_eq!(auth.world().world_flag(FlagId(3)), 1);
    }

    #[test]
    fn conditions_about_a_missing_unit_are_false_but_world_flags_still_count() {
        let (mut auth, hero) = arena();
        auth.world_mut().set_world_flag(FlagId(5), 2);
        auth.world_mut().despawn(hero);
        let w = auth.world();
        assert!(!w.check(&Condition::Actor(HERO), hero));
        assert!(!w.check(&char_flag(KILLS, Cmp::Eq, 0), hero));
        assert!(w.check(
            &Condition::Flag {
                scope: FlagScope::World,
                flag: FlagId(5),
                cmp: Cmp::Gt,
                value: 1
            },
            hero
        ));
        assert!(w.check(&Condition::All(vec![]), hero), "빈 All 은 참");
        assert!(!w.check(&Condition::Any(vec![]), hero), "빈 Any 는 거짓");
    }

    #[test]
    fn zero_flags_are_not_stored() {
        let mut f = Flags::default();
        assert_eq!(f.get(FlagId(3)), 0, "없는 플래그는 0");
        f.set(FlagId(3), 5);
        f.set(FlagId(1), -2);
        assert_eq!(
            f.iter().collect::<Vec<_>>(),
            [(FlagId(1), -2), (FlagId(3), 5)],
            "번호 순"
        );
        f.set(FlagId(3), 0);
        assert_eq!(f.iter().count(), 1, "0 으로 만들면 지운다");
    }

    #[test]
    fn a_level_trigger_fires_even_when_levels_are_skipped() {
        let unit = nexus_core::World::default().spawn();
        let leveled = |from, to| Happening::Leveled { unit, from, to };
        assert_eq!(leveled(1, 2).matches(On::LevelReached(2)), Some(unit));
        assert_eq!(leveled(3, 6).matches(On::LevelReached(5)), Some(unit));
        assert_eq!(
            leveled(5, 6).matches(On::LevelReached(5)),
            None,
            "이미 지났다"
        );
        assert_eq!(leveled(1, 4).matches(On::LevelReached(5)), None);
    }

    #[test]
    fn entering_means_crossing_the_edge() {
        let unit = nexus_core::World::default().spawn();
        let on = On::Entered {
            center: Vec2::ZERO,
            radius: 2.0,
        };
        let moved = |from: Vec2, to: Vec2| Happening::Moved { unit, from, to };
        assert_eq!(
            moved(Vec2::new(3.0, 0.0), Vec2::new(1.5, 0.0)).matches(on),
            Some(unit)
        );
        assert_eq!(
            moved(Vec2::new(1.0, 0.0), Vec2::new(0.5, 0.0)).matches(on),
            None,
            "이미 안"
        );
        assert_eq!(
            moved(Vec2::new(1.0, 0.0), Vec2::new(3.0, 0.0)).matches(on),
            None,
            "나감"
        );
    }
}
