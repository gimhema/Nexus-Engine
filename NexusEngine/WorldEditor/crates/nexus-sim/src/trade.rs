//! 재화와 NPC 기능 — 지갑·상점·서비스 (R4).
//!
//! ```text
//! 플레이어 ──Interact──▶ NPC(액터 타입 → 기능 목록)          Event::Interacted → 앱이 기능 메뉴를 연다
//!          ──Buy(상점, 물건 번호, 개수)──▶ 지갑 −  가방 +     Event::Bought · CurrencyChanged
//!          ──Sell(상점, 가방 칸, 개수)──▶ 가방 −  지갑 +      Event::Sold · CurrencyChanged
//!          ──UseService(기능 번호)──▶ 치료 등                 Event::Healed …
//!          ──Craft(제작 표, 레시피, 횟수)──▶ 재료·수수료 − 결과물 +   Event::Crafted (R4-2)
//!          ──Teleport(목적지 표, 목적지)──▶ 비용 − 위치 이동          Event::Teleported / Traveled (R4-2)
//!          ──Deposit / Withdraw──▶ 가방 ↔ 창고                      Event::Deposited / Withdrew (R4-2)
//! ```
//!
//! # 다른 레벨로 가는 텔레포트
//!
//! 시뮬레이션은 **지금 존 하나만 안다.** 목적지가 다른 레벨이면(`travel` ≠ 0) 비용만 받고
//! `Event::Traveled { travel, pos }` 를 낸다 — 레벨을 열고 그 자리에 세우는 것은 앱의 일이다.
//! `travel` 은 앱이 레벨 이름과 짝지은 번호다(시뮬레이션은 문자열을 모른다). 단계 2 에서는 서버가 존을 옮긴다.
//!
//! # NPC 역할은 종류가 아니라 기능의 조합
//!
//! "상인 NPC" 라는 타입은 없다. **액터 타입에 붙은 기능 목록**([`Service`])이 그 NPC 가 하는 일이다 —
//! `Shop(1)` 이 붙으면 상인, `Heal` 이 붙으면 치료사, 둘 다 붙으면 둘 다 한다. 기능마다 필요한 것은
//! 데이터 표(상점의 물건 목록 등)뿐이다. 대화(R9)가 생기면 이 목록 앞에 끼운다 — 기능 쪽은 바뀌지 않는다.
//!
//! # 사고팔기는 Intent 다
//!
//! 플레이어가 하는 일이라 [`Intent`](crate::Intent) 로 들어오고 권한자가 판정한다(거리·기능·재화·가방 자리).
//! 단계 2 에서는 서버가 같은 판정을 한다. 트리거가 재화를 주는 것(`Effect::GiveCurrency`)은 R1 원칙대로
//! 권한자 안의 효과이고 Intent 가 없다.
//!
//! # 재화
//!
//! 아이템과 **별개인 지갑**이다 — 가방 칸을 차지하지 않고 겹침 한도도 없다. 종류는 번호([`CurrencyId`])로
//! 여럿 둘 수 있다(골드·명예 점수 …). 이름은 표시 데이터다. 금액은 `u64`, 넘치면 끝에서 멈춘다.

use std::collections::BTreeMap;

use crate::item::ItemId;

/// 재화 번호. 서버와 **같은 ID 공간**이다.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CurrencyId(pub u32);

/// 값 — 어느 재화로 얼마.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Price {
    pub currency: CurrencyId,
    pub amount: u32,
}

/// 유닛 하나의 지갑. 없는 재화는 0 이고, 0 이 되면 지운다 (저장이 작게).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Wallet(BTreeMap<CurrencyId, u64>);

impl Wallet {
    #[must_use]
    pub fn get(&self, currency: CurrencyId) -> u64 {
        self.0.get(&currency).copied().unwrap_or(0)
    }

    /// 0 이 아닌 재화 전부 — 번호 순.
    pub fn iter(&self) -> impl Iterator<Item = (CurrencyId, u64)> + '_ {
        self.0.iter().map(|(c, a)| (*c, *a))
    }

    pub(crate) fn set(&mut self, currency: CurrencyId, amount: u64) {
        if amount == 0 {
            self.0.remove(&currency);
        } else {
            self.0.insert(currency, amount);
        }
    }

    /// 더한다 (넘치면 끝에서 멈춘다). 새 금액.
    pub(crate) fn add(&mut self, currency: CurrencyId, amount: u64) -> u64 {
        let total = self.get(currency).saturating_add(amount);
        self.set(currency, total);
        total
    }

    /// 뺀다 — 모자라면 아무것도 빼지 않고 `None`.
    pub(crate) fn take(&mut self, currency: CurrencyId, amount: u64) -> Option<u64> {
        let total = self.get(currency).checked_sub(amount)?;
        self.set(currency, total);
        Some(total)
    }
}

/// 상점 번호. 서버와 같은 ID 공간.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ShopId(pub u32);

/// 상점이 파는 물건 하나.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Goods {
    pub item: ItemId,
    /// 한 개 값.
    pub price: Price,
}

/// 상점 하나 — 파는 물건과 사들이는 비율.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShopDef {
    /// 파는 물건 — **목록 순서가 번호**다(`Intent::Buy` 의 `goods`).
    pub goods: Vec<Goods>,
    /// 플레이어가 파는 아이템을 사들이는 비율 — 아이템 기본 값([`SimWorld::define_item_price`])의 천분율.
    /// 0 이면 사들이지 않는다.
    ///
    /// [`SimWorld::define_item_price`]: crate::SimWorld::define_item_price
    pub buy_rate_per_mille: u32,
}

/// NPC 가 하는 일 하나.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Service {
    /// 상점을 연다 — 사고팔기는 `Intent::Buy`/`Sell`.
    Shop(ShopId),
    /// 값을 받고 HP·MP 를 가득 채운다 (`Intent::UseService`). 값이 0 이면 무료.
    Heal { price: Price },
    /// 제작 표를 연다 — `Intent::Craft` (R4-2).
    Craft(CraftId),
    /// 텔레포트 목적지 표를 연다 — `Intent::Teleport` (R4-2).
    Teleport(TeleportId),
    /// 창고 — `Intent::Deposit`/`Withdraw` (R4-2). 창고는 캐릭터마다 하나이고 어느 창고지기에게서나 같다.
    Storage,
}

/// 제작 표 번호. 서버와 같은 ID 공간.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CraftId(pub u32);

/// 레시피 하나 — 재료(와 수수료)를 내고 결과물을 받는다.
#[derive(Clone, Debug, PartialEq)]
pub struct Recipe {
    /// `(아이템, 개수)` — 두 가방을 합쳐 센다.
    pub inputs: Vec<(ItemId, u32)>,
    /// 수수료. `None` 이면 무료.
    pub fee: Option<Price>,
    pub output: ItemId,
    pub count: u32,
    /// 만들 수 있는 조건 (R1 Condition) — 만드는 쪽이 대상.
    pub requires: crate::trigger::Condition,
}

/// 제작 표 — 레시피 목록. **목록 순서가 레시피 번호**다.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CraftDef {
    pub recipes: Vec<Recipe>,
}

/// 텔레포트 목적지 표 번호.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TeleportId(pub u32);

/// 목적지 하나.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Destination {
    /// 도착 위치 (m).
    pub pos: nexus_core::Vec2,
    /// 0 이면 지금 존 안. 그 밖은 앱이 레벨과 짝지은 번호 — 시뮬레이션은 이동하지 않고 `Traveled` 를 낸다.
    pub travel: u32,
    /// 비용. `None` 이면 무료.
    pub price: Option<Price>,
}

/// 텔레포트 목적지 표 — **목록 순서가 목적지 번호**다.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TeleportDef {
    pub destinations: Vec<Destination>,
}

/// 상호작용 거리 기본값 (m) — 줍기보다 조금 넓다. [`SimWorld::set_interact_range`] 로 바꾼다.
///
/// [`SimWorld::set_interact_range`]: crate::SimWorld::set_interact_range
pub const DEFAULT_INTERACT_RANGE: f32 = 3.0;

/// 값 × 개수 — 넘치면 `None`.
pub(crate) fn total(price: Price, count: u32) -> Option<u64> {
    u64::from(price.amount).checked_mul(u64::from(count))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use nexus_core::{Entity, Vec2};

    use super::*;
    use crate::trigger::{Condition, Effect, On, TriggerDef, TriggerId};
    use crate::{
        ActorType, Authority, BagKind, Event, FactionId, Intent, ItemDef, ItemKind, LocalAuthority,
        Rejection, Relation, SimWorld, SkillDef, SkillId, Tile, TileMap, UnitDef,
    };

    const DT: Duration = Duration::from_millis(50);
    const GOLD: CurrencyId = CurrencyId(1);
    const POTION: ItemId = ItemId(501);
    const JELLY: ItemId = ItemId(909);
    const SHOP: ShopId = ShopId(1);
    const MERCHANT: ActorType = ActorType(10);

    fn gold(amount: u32) -> Price {
        Price {
            currency: GOLD,
            amount,
        }
    }

    /// 플레이어(골드 100) + 2m 옆 상인(물약 30골드, 반값에 사들임, 치료 20골드).
    fn town() -> (LocalAuthority, Entity, Entity) {
        let mut w = SimWorld::new(TileMap::new(30, 30, 1.0, Vec2::ZERO, Tile::default()));
        for id in [POTION, JELLY] {
            w.define_item(
                id,
                ItemDef {
                    kind: ItemKind::Consumable { heal: 10, mana: 0 },
                    max_stack: 20,
                },
            );
        }
        w.define_item_price(POTION, gold(40));
        w.define_shop(
            SHOP,
            ShopDef {
                goods: vec![Goods {
                    item: POTION,
                    price: gold(30),
                }],
                buy_rate_per_mille: 500,
            },
        );
        w.define_services(
            MERCHANT,
            vec![Service::Shop(SHOP), Service::Heal { price: gold(20) }],
        );
        let me = w.spawn_unit(
            Vec2::new(5.5, 5.5),
            0.0,
            UnitDef {
                max_hp: 100,
                faction: FactionId(1),
                ..UnitDef::default()
            },
        );
        w.set_currency(me, GOLD, 100);
        let npc = w.spawn_unit(
            Vec2::new(7.5, 5.5),
            0.0,
            UnitDef {
                actor: MERCHANT,
                faction: FactionId(100),
                ..UnitDef::default()
            },
        );
        (LocalAuthority::new(w), me, npc)
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

    fn purse(auth: &LocalAuthority, me: Entity) -> u64 {
        auth.world().unit(me).unwrap().wallet().get(GOLD)
    }

    fn potions(auth: &LocalAuthority, me: Entity) -> u32 {
        auth.world()
            .unit(me)
            .unwrap()
            .inventory()
            .bag(BagKind::Consumable)
            .count_of(POTION)
    }

    #[test]
    fn talking_buying_and_running_out_of_gold() {
        let (mut auth, me, npc) = town();
        let hello = run(&mut auth, Intent::Interact { unit: me, npc });
        assert!(hello.contains(&Event::Interacted { unit: me, npc }));
        let buy = |count| Intent::Buy {
            unit: me,
            npc,
            shop: SHOP,
            goods: 0,
            count,
        };
        let events = run(&mut auth, buy(3));
        assert!(events.contains(&Event::Bought {
            unit: me,
            item: POTION,
            count: 3
        }));
        assert!(events.contains(&Event::CurrencyChanged {
            unit: me,
            currency: GOLD,
            delta: -90,
            total: 10
        }));
        assert_eq!((purse(&auth, me), potions(&auth, me)), (10, 3));
        assert_eq!(
            rejected(&run(&mut auth, buy(1))),
            Some(Rejection::NotEnoughCurrency)
        );
        assert_eq!(potions(&auth, me), 3, "모자라면 아무것도 바뀌지 않는다");
    }

    #[test]
    fn selling_pays_the_shop_rate_and_unpriced_items_are_refused() {
        let (mut auth, me, npc) = town();
        let w = auth.world_mut();
        assert!(w.give_item(me, POTION, 5));
        assert!(w.give_item(me, JELLY, 1));
        // 물약 기본 값 40 × 반값 = 20, 3개 → 60.
        let events = run(
            &mut auth,
            Intent::Sell {
                unit: me,
                npc,
                shop: SHOP,
                bag: BagKind::Consumable,
                slot: 0,
                count: 3,
            },
        );
        assert!(events.contains(&Event::Sold {
            unit: me,
            item: POTION,
            count: 3
        }));
        assert_eq!((purse(&auth, me), potions(&auth, me)), (160, 2));
        // 칸보다 많이 팔려 해도 있는 만큼만.
        run(
            &mut auth,
            Intent::Sell {
                unit: me,
                npc,
                shop: SHOP,
                bag: BagKind::Consumable,
                slot: 0,
                count: 99,
            },
        );
        assert_eq!((purse(&auth, me), potions(&auth, me)), (200, 0));
        // 젤리는 값이 없다 — 상점이 사지 않는다.
        let jelly_slot = 1;
        let refused = run(
            &mut auth,
            Intent::Sell {
                unit: me,
                npc,
                shop: SHOP,
                bag: BagKind::Consumable,
                slot: jelly_slot,
                count: 1,
            },
        );
        assert_eq!(rejected(&refused), Some(Rejection::NotSellable));
    }

    #[test]
    fn distance_hostility_and_missing_services_are_refused() {
        let (mut auth, me, npc) = town();
        let w = auth.world_mut();
        let stranger = w.spawn_unit(Vec2::new(6.5, 6.5), 0.0, UnitDef::default());
        let far = w.spawn_unit(
            Vec2::new(20.5, 20.5),
            0.0,
            UnitDef {
                actor: MERCHANT,
                ..UnitDef::default()
            },
        );
        assert_eq!(
            rejected(&run(
                &mut auth,
                Intent::Interact {
                    unit: me,
                    npc: stranger
                }
            )),
            Some(Rejection::NoService)
        );
        assert_eq!(
            rejected(&run(&mut auth, Intent::Interact { unit: me, npc: far })),
            Some(Rejection::OutOfRange)
        );
        auth.world_mut()
            .set_relation(FactionId(1), FactionId(100), Relation::Hostile);
        assert_eq!(
            rejected(&run(&mut auth, Intent::Interact { unit: me, npc })),
            Some(Rejection::InvalidTarget),
            "적대 진영과는 거래하지 않는다"
        );
    }

    #[test]
    fn a_paid_heal_fills_hp() {
        let (mut auth, me, npc) = town();
        // 다치게 한다 — 설정 작업으로 HP 를 심는다.
        auth.world_mut().set_hp(me, 30);
        let events = run(
            &mut auth,
            Intent::UseService {
                unit: me,
                npc,
                index: 1,
            },
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::Healed { amount: 70, .. }))
        );
        assert_eq!(purse(&auth, me), 80);
        assert_eq!(
            rejected(&run(
                &mut auth,
                Intent::UseService {
                    unit: me,
                    npc,
                    index: 0
                }
            )),
            Some(Rejection::NoService),
            "상점은 Buy/Sell 로 쓴다"
        );
    }

    #[test]
    fn bounties_and_trigger_rewards_fill_the_wallet() {
        let (mut auth, me, _) = town();
        let w = auth.world_mut();
        w.define_skill(
            SkillId(1),
            SkillDef {
                range: 3.0,
                cooldown_ms: 0,
                damage_mult: 1.0,
                mp_cost: 0,
                radius: 0.0,
                heal: 0,
            },
        );
        w.define_trigger(
            TriggerId(1),
            TriggerDef {
                on: On::Killed { actor: None },
                when: Condition::HasCurrency {
                    currency: GOLD,
                    amount: 115,
                },
                then: vec![Effect::GiveCurrency {
                    currency: GOLD,
                    amount: 1000,
                }],
            },
        );
        let slime = w.spawn_unit(
            Vec2::new(5.5, 6.5),
            0.0,
            UnitDef {
                bounty: Some(gold(15)),
                ..UnitDef::default()
            },
        );
        run(
            &mut auth,
            Intent::Attack {
                unit: me,
                target: slime,
                skill: SkillId(1),
            },
        );
        // 처치 보상 15 → 115, 트리거가 그것을 보고 1000 더.
        assert_eq!(purse(&auth, me), 1115);
    }

    // ── 제작·텔레포트·창고 (R4-2) ────────────────────────────────────────────

    const SMITH: ActorType = ActorType(12);
    const CRAFT: CraftId = CraftId(1);
    const GATE: TeleportId = TeleportId(1);

    /// 마을 상인 옆에 대장장이(제작 + 텔레포트 + 창고)를 세운다.
    fn smithy() -> (LocalAuthority, Entity, Entity) {
        let (mut auth, me, _) = town();
        let w = auth.world_mut();
        w.define_craft(
            CRAFT,
            CraftDef {
                recipes: vec![
                    // 젤리 3 + 10골드 → 물약 1
                    Recipe {
                        inputs: vec![(JELLY, 3)],
                        fee: Some(gold(10)),
                        output: POTION,
                        count: 1,
                        requires: Condition::Always,
                    },
                    // 레벨 5 부터
                    Recipe {
                        inputs: vec![(JELLY, 1)],
                        fee: None,
                        output: POTION,
                        count: 5,
                        requires: Condition::Level {
                            cmp: crate::Cmp::Ge,
                            value: 5,
                        },
                    },
                ],
            },
        );
        w.define_teleport(
            GATE,
            TeleportDef {
                destinations: vec![
                    Destination {
                        pos: Vec2::new(20.5, 20.5),
                        travel: 0,
                        price: Some(gold(25)),
                    },
                    Destination {
                        pos: Vec2::new(3.0, 4.0),
                        travel: 7,
                        price: None,
                    },
                ],
            },
        );
        w.define_services(
            SMITH,
            vec![
                Service::Craft(CRAFT),
                Service::Teleport(GATE),
                Service::Storage,
            ],
        );
        let smith = w.spawn_unit(
            Vec2::new(5.5, 7.5),
            0.0,
            UnitDef {
                actor: SMITH,
                faction: FactionId(100),
                ..UnitDef::default()
            },
        );
        (auth, me, smith)
    }

    #[test]
    fn crafting_takes_materials_and_a_fee() {
        let (mut auth, me, smith) = smithy();
        assert!(auth.world_mut().give_item(me, JELLY, 7));
        let craft = |recipe, times| Intent::Craft {
            unit: me,
            npc: smith,
            craft: CRAFT,
            recipe,
            times,
        };
        let events = run(&mut auth, craft(0, 2));
        assert!(events.contains(&Event::Crafted {
            unit: me,
            item: POTION,
            count: 2
        }));
        let u = auth.world().unit(me).unwrap();
        assert_eq!(u.inventory().bag(BagKind::Consumable).count_of(JELLY), 1);
        assert_eq!((potions(&auth, me), purse(&auth, me)), (2, 80));
        assert_eq!(
            rejected(&run(&mut auth, craft(0, 1))),
            Some(Rejection::MissingMaterials)
        );
        assert_eq!(
            rejected(&run(&mut auth, craft(1, 1))),
            Some(Rejection::RequirementNotMet),
            "레벨 5 레시피"
        );
    }

    #[test]
    fn teleporting_inside_the_zone_or_to_another_level() {
        let (mut auth, me, smith) = smithy();
        let go = |dest| Intent::Teleport {
            unit: me,
            npc: smith,
            teleport: GATE,
            dest,
        };
        let here = run(&mut auth, go(0));
        assert!(here.contains(&Event::Teleported {
            unit: me,
            pos: Vec2::new(20.5, 20.5)
        }));
        assert_eq!(auth.world().unit(me).unwrap().pos(), Vec2::new(20.5, 20.5));
        assert_eq!(purse(&auth, me), 75);
        // 멀어졌으니 다시 쓰려면 돌아와야 한다.
        assert_eq!(
            rejected(&run(&mut auth, go(1))),
            Some(Rejection::OutOfRange)
        );
        auth.world_mut().set_position(me, Vec2::new(5.5, 6.5));
        let away = run(&mut auth, go(1));
        assert!(away.contains(&Event::Traveled {
            unit: me,
            travel: 7,
            pos: Vec2::new(3.0, 4.0)
        }));
        assert_eq!(
            auth.world().unit(me).unwrap().pos(),
            Vec2::new(5.5, 6.5),
            "다른 레벨은 앱이 옮긴다"
        );
    }

    #[test]
    fn storage_keeps_items_between_bags() {
        let (mut auth, me, smith) = smithy();
        assert!(auth.world_mut().give_item(me, POTION, 5));
        let events = run(
            &mut auth,
            Intent::Deposit {
                unit: me,
                npc: smith,
                bag: BagKind::Consumable,
                slot: 0,
                count: 3,
            },
        );
        assert!(events.contains(&Event::Deposited {
            unit: me,
            item: POTION,
            count: 3
        }));
        let u = auth.world().unit(me).unwrap();
        assert_eq!((potions(&auth, me), u.storage().count_of(POTION)), (2, 3));
        run(
            &mut auth,
            Intent::Withdraw {
                unit: me,
                npc: smith,
                slot: 0,
                count: 99,
            },
        );
        let u = auth.world().unit(me).unwrap();
        assert_eq!((potions(&auth, me), u.storage().count_of(POTION)), (5, 0));
        // 상인에게는 창고가 없다.
        let (mut auth, me, merchant) = town();
        assert!(auth.world_mut().give_item(me, POTION, 1));
        let refused = run(
            &mut auth,
            Intent::Deposit {
                unit: me,
                npc: merchant,
                bag: BagKind::Consumable,
                slot: 0,
                count: 1,
            },
        );
        assert_eq!(rejected(&refused), Some(Rejection::NoService));
    }

    #[test]
    fn a_wallet_never_goes_negative_and_forgets_zero() {
        let gold = CurrencyId(1);
        let mut w = Wallet::default();
        assert_eq!(w.add(gold, 50), 50);
        assert_eq!(w.take(gold, 80), None, "모자라면 빼지 않는다");
        assert_eq!(w.get(gold), 50);
        assert_eq!(w.take(gold, 50), Some(0));
        assert_eq!(w.iter().count(), 0, "0 이면 지운다");
        w.set(gold, u64::MAX);
        assert_eq!(w.add(gold, 1), u64::MAX, "넘치면 끝에서 멈춘다");
    }
}
