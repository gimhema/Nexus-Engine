//! 아이템 — 정의(데이터)와 가방.
//!
//! 서버 `Game/Logic/Item.h` · `ItemBag.h` · `CommonLogicType.h` 의 **구조를 따른다**:
//! 종류별 가방(소모품·장비)이 따로 있고, 칸 수는 서버 `MAX_*_BAG_NUM` 과 같은 120 이며,
//! 장비 자리는 `EQUIPMENT_POS_TYPE` 과 같은 다섯 곳이다.
//!
//! 서버 쪽은 사용·장착 효과가 아직 TODO 라 **효과 규칙은 여기서 정했다** — 소모품은 HP 회복,
//! 장비는 공격력·방어력 가산. 스킨(`SkinBag`)은 외형만 바꾸고 규칙에 영향이 없어 다루지 않는다.

use nexus_core::Vec2;

/// 아이템 **정의** 번호 — "아이템 1041" 처럼 정적 테이블의 키다. 개별 아이템 인스턴스가 아니다.
/// 서버 테이블과 같은 ID 공간 — **ID 공간이 곧 프로토콜.**
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ItemId(pub u32);

/// 장비 자리. 서버 `EQUIPMENT_POS_TYPE` (1~5) 와 같은 순서.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EquipSlot {
    Weapon,
    Head,
    Body,
    Hand,
    Shoes,
}

impl EquipSlot {
    pub const ALL: [Self; 5] = [
        Self::Weapon,
        Self::Head,
        Self::Body,
        Self::Hand,
        Self::Shoes,
    ];

    fn index(self) -> usize {
        self as usize
    }
}

/// 아이템이 하는 일.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ItemKind {
    /// 쓰면 사라지고 HP 를 회복한다.
    Consumable { heal: u32 },
    /// `slot` 에 장착하면 수치가 더해진다.
    Equipment {
        slot: EquipSlot,
        attack: u32,
        defense: u32,
    },
}

/// 아이템 정적 데이터.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemDef {
    pub kind: ItemKind,
    /// 한 칸에 겹칠 수 있는 최대 개수. 0 은 1 로 본다. 장비는 항상 1.
    pub max_stack: u32,
}

impl ItemDef {
    #[must_use]
    pub fn bag(&self) -> BagKind {
        match self.kind {
            ItemKind::Consumable { .. } => BagKind::Consumable,
            ItemKind::Equipment { .. } => BagKind::Equipment,
        }
    }

    /// 실제 겹침 한도.
    #[must_use]
    pub fn stack_limit(&self) -> u32 {
        match self.kind {
            ItemKind::Equipment { .. } => 1,
            ItemKind::Consumable { .. } => self.max_stack.max(1),
        }
    }
}

/// 한 칸에 든 것.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ItemStack {
    pub item: ItemId,
    pub count: u32,
}

/// 가방 종류. 서버는 종류마다 가방을 따로 둔다.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BagKind {
    Consumable,
    Equipment,
}

/// 가방 한 개의 칸 수. 서버 `MAX_CONSUMABLE_BAG_NUM` / `MAX_EQUIPMENT_BAG_NUM` 과 같다.
pub const BAG_SLOTS: usize = 120;

/// 칸 번호가 고정된 가방. 칸 번호는 프로토콜에 실리므로(서버 `pos`) 빈칸을 당겨 채우지 않는다.
///
/// 벡터는 **처음 넣을 때** 늘린다 — 대부분의 몬스터는 가방이 비어 있다.
#[derive(Clone, Debug, Default)]
pub struct Bag {
    slots: Vec<Option<ItemStack>>,
}

impl Bag {
    #[must_use]
    pub fn get(&self, slot: usize) -> Option<ItemStack> {
        self.slots.get(slot).copied().flatten()
    }

    /// 채워진 칸 전부 `(칸 번호, 내용)`.
    pub fn iter(&self) -> impl Iterator<Item = (usize, ItemStack)> + '_ {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.map(|s| (i, s)))
    }

    /// 같은 아이템의 총 개수.
    #[must_use]
    pub fn count_of(&self, item: ItemId) -> u32 {
        self.iter()
            .filter(|(_, s)| s.item == item)
            .map(|(_, s)| s.count)
            .sum()
    }

    /// `count` 개를 **모두** 넣을 수 있는가 — 기존 더미 여유 + 빈칸.
    #[must_use]
    pub(crate) fn fits(&self, item: ItemId, count: u32, limit: u32) -> bool {
        let room_in_stacks: u64 = self
            .iter()
            .filter(|(_, s)| s.item == item)
            .map(|(_, s)| u64::from(limit.saturating_sub(s.count)))
            .sum();
        let empty = (BAG_SLOTS - self.iter().count()) as u64;
        room_in_stacks + empty * u64::from(limit) >= u64::from(count)
    }

    /// 넣는다. 기존 더미를 먼저 채우고, 남으면 앞쪽 빈칸부터. [`fits`](Self::fits) 가 참일 때만 부를 것.
    pub(crate) fn add(&mut self, item: ItemId, mut count: u32, limit: u32) {
        for s in self.slots.iter_mut().flatten() {
            if count == 0 {
                return;
            }
            if s.item == item && s.count < limit {
                let moved = (limit - s.count).min(count);
                s.count += moved;
                count -= moved;
            }
        }
        while count > 0 {
            let moved = count.min(limit);
            let at = self.first_empty().expect("fits() 를 먼저 확인해야 한다");
            self.put(at, Some(ItemStack { item, count: moved }));
            count -= moved;
        }
    }

    fn first_empty(&self) -> Option<usize> {
        (0..BAG_SLOTS).find(|&i| self.get(i).is_none())
    }

    /// 칸을 통째로 바꾼다.
    pub(crate) fn put(&mut self, slot: usize, stack: Option<ItemStack>) {
        if slot >= BAG_SLOTS {
            return;
        }
        if self.slots.len() <= slot {
            if stack.is_none() {
                return;
            }
            self.slots.resize(slot + 1, None);
        }
        self.slots[slot] = stack;
    }

    /// 칸에서 하나를 뺀다. 0 이 되면 비운다.
    pub(crate) fn take_one(&mut self, slot: usize) {
        if let Some(Some(s)) = self.slots.get_mut(slot) {
            s.count -= 1;
            if s.count == 0 {
                self.slots[slot] = None;
            }
        }
    }
}

/// 유닛의 소지품 — 종류별 가방 + 장착 자리.
#[derive(Clone, Debug, Default)]
pub struct Inventory {
    pub(crate) consumables: Bag,
    pub(crate) equipment: Bag,
    pub(crate) equipped: [Option<ItemId>; 5],
}

impl Inventory {
    #[must_use]
    pub fn bag(&self, kind: BagKind) -> &Bag {
        match kind {
            BagKind::Consumable => &self.consumables,
            BagKind::Equipment => &self.equipment,
        }
    }

    pub(crate) fn bag_mut(&mut self, kind: BagKind) -> &mut Bag {
        match kind {
            BagKind::Consumable => &mut self.consumables,
            BagKind::Equipment => &mut self.equipment,
        }
    }

    /// `slot` 에 장착한 아이템.
    #[must_use]
    pub fn equipped(&self, slot: EquipSlot) -> Option<ItemId> {
        self.equipped[slot.index()]
    }

    pub(crate) fn set_equipped(&mut self, slot: EquipSlot, item: Option<ItemId>) {
        self.equipped[slot.index()] = item;
    }
}

/// 땅에 떨어진 아이템.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroundItem {
    pub pos: Vec2,
    pub stack: ItemStack,
}

#[cfg(test)]
mod tests {
    use super::*;

    const POTION: ItemId = ItemId(1);
    const HERB: ItemId = ItemId(2);

    #[test]
    fn add_fills_existing_stacks_before_empty_slots() {
        let mut bag = Bag::default();
        bag.add(POTION, 7, 10);
        bag.add(HERB, 1, 10);
        bag.add(POTION, 5, 10);
        // 7 + 5 = 12 → 앞 더미를 10 까지 채우고 남은 2 는 다음 빈칸(2번).
        assert_eq!(
            bag.get(0),
            Some(ItemStack {
                item: POTION,
                count: 10
            })
        );
        assert_eq!(
            bag.get(1),
            Some(ItemStack {
                item: HERB,
                count: 1
            })
        );
        assert_eq!(
            bag.get(2),
            Some(ItemStack {
                item: POTION,
                count: 2
            })
        );
        assert_eq!(bag.count_of(POTION), 12);
    }

    #[test]
    fn slot_numbers_are_stable_after_removal() {
        let mut bag = Bag::default();
        bag.add(POTION, 1, 1);
        bag.add(HERB, 1, 1);
        bag.take_one(0);
        assert_eq!(bag.get(0), None);
        assert_eq!(bag.get(1).map(|s| s.item), Some(HERB), "칸이 당겨졌다");
        // 새로 넣으면 비어 있는 앞 칸부터.
        bag.add(POTION, 1, 1);
        assert_eq!(bag.get(0).map(|s| s.item), Some(POTION));
    }

    #[test]
    fn fits_counts_room_in_stacks_and_empty_slots() {
        let mut bag = Bag::default();
        for i in 0..BAG_SLOTS as u32 {
            bag.add(ItemId(100 + i), 1, 1);
        }
        assert!(!bag.fits(POTION, 1, 5), "가득 찬 가방");
        let mut bag = Bag::default();
        for _ in 0..BAG_SLOTS - 1 {
            bag.add(HERB, 1, 1);
        }
        bag.add(POTION, 3, 5);
        assert!(bag.fits(POTION, 2, 5), "기존 더미 여유");
        assert!(!bag.fits(POTION, 3, 5));
    }

    #[test]
    fn equipment_never_stacks() {
        let def = ItemDef {
            kind: ItemKind::Equipment {
                slot: EquipSlot::Weapon,
                attack: 5,
                defense: 0,
            },
            max_stack: 99,
        };
        assert_eq!(def.stack_limit(), 1);
        assert_eq!(def.bag(), BagKind::Equipment);
    }
}

/// Authority 를 거친 아이템 흐름 — 드롭·줍기·버리기·사용·장착.
#[cfg(test)]
mod flow_tests {
    use std::time::Duration;

    use nexus_core::{Entity, Vec2};

    use super::*;
    use crate::{
        Authority, Event, Intent, LocalAuthority, LootEntry, LootTableId, Rejection, SimWorld,
        SkillDef, SkillId, Tile, TileMap, UnitDef,
    };

    const DT: Duration = Duration::from_millis(50);
    const POTION: ItemId = ItemId(501);
    const SWORD: ItemId = ItemId(1041);
    const AXE: ItemId = ItemId(1042);
    const JELLY: ItemId = ItemId(909);
    const SLIME_LOOT: LootTableId = LootTableId(1);
    const HIT: SkillId = SkillId(1);

    fn world() -> SimWorld {
        let mut w = SimWorld::new(TileMap::new(20, 20, 1.0, Vec2::ZERO, Tile::default()));
        w.define_skill(
            HIT,
            SkillDef {
                range: 2.0,
                cooldown_ms: 500,
                damage_mult: 1.0,
            },
        );
        w.define_item(
            POTION,
            ItemDef {
                kind: ItemKind::Consumable { heal: 30 },
                max_stack: 10,
            },
        );
        let weapon = |attack| ItemDef {
            kind: ItemKind::Equipment {
                slot: EquipSlot::Weapon,
                attack,
                defense: 0,
            },
            max_stack: 1,
        };
        w.define_item(SWORD, weapon(15));
        w.define_item(AXE, weapon(25));
        w.define_item(
            JELLY,
            ItemDef {
                kind: ItemKind::Consumable { heal: 5 },
                max_stack: 99,
            },
        );
        w.define_loot(
            SLIME_LOOT,
            vec![
                LootEntry {
                    item: JELLY,
                    count: 3,
                    chance_per_mille: 1000,
                },
                LootEntry {
                    item: POTION,
                    count: 1,
                    chance_per_mille: 0,
                },
            ],
        );
        w
    }

    fn hero() -> UnitDef {
        UnitDef {
            move_speed: 2.0,
            max_hp: 100,
            attack: 10,
            ..UnitDef::default()
        }
    }

    fn slime() -> UnitDef {
        UnitDef {
            max_hp: 1,
            loot: Some(SLIME_LOOT),
            ..UnitDef::default()
        }
    }

    fn submit(auth: &mut LocalAuthority, intent: Intent) -> Vec<Event> {
        auth.submit(intent);
        auth.tick(DT)
    }

    fn rejected(events: &[Event]) -> Option<Rejection> {
        events.iter().find_map(|e| match e {
            Event::Rejected { reason, .. } => Some(*reason),
            _ => None,
        })
    }

    fn bag(auth: &LocalAuthority, unit: Entity, kind: BagKind) -> &Bag {
        auth.world().unit(unit).unwrap().inventory().bag(kind)
    }

    fn spawned(events: &[Event]) -> Vec<Entity> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::ItemSpawned { item, .. } => Some(*item),
                _ => None,
            })
            .collect()
    }

    // ── 드롭 · 줍기 ──────────────────────────────────────────────────────────

    #[test]
    fn kill_drops_loot_at_the_corpse_and_it_can_be_picked_up() {
        let mut w = world();
        let h = w.spawn_unit(Vec2::new(5.5, 5.5), 0.0, hero());
        let corpse_at = Vec2::new(6.5, 5.5);
        let s = w.spawn_unit(corpse_at, 0.0, slime());
        let mut auth = LocalAuthority::new(w);

        let events = submit(
            &mut auth,
            Intent::Attack {
                unit: h,
                target: s,
                skill: HIT,
            },
        );
        let drops = spawned(&events);
        assert_eq!(drops.len(), 1, "확률 0 줄은 나오지 않는다: {events:?}");
        let jelly = auth.world().ground_item(drops[0]).copied().unwrap();
        assert_eq!(jelly.pos, corpse_at);
        assert_eq!(
            jelly.stack,
            ItemStack {
                item: JELLY,
                count: 3
            }
        );
        // 드롭은 사망 뒤에 온다.
        let died_at = events
            .iter()
            .position(|e| matches!(e, Event::Died { .. }))
            .unwrap();
        let spawned_at = events
            .iter()
            .position(|e| matches!(e, Event::ItemSpawned { .. }))
            .unwrap();
        assert!(died_at < spawned_at);

        let events = submit(
            &mut auth,
            Intent::PickUp {
                unit: h,
                item: drops[0],
            },
        );
        assert!(events.contains(&Event::PickedUp {
            unit: h,
            item: drops[0],
            stack: jelly.stack,
        }));
        assert_eq!(bag(&auth, h, BagKind::Consumable).count_of(JELLY), 3);
        assert!(auth.world().ground_item(drops[0]).is_none());

        // 이미 주운 것을 또 주울 수 없다.
        let again = submit(
            &mut auth,
            Intent::PickUp {
                unit: h,
                item: drops[0],
            },
        );
        assert_eq!(rejected(&again), Some(Rejection::UnknownItem));
    }

    #[test]
    fn pick_up_needs_to_be_close() {
        let mut w = world();
        let h = w.spawn_unit(Vec2::new(1.5, 1.5), 0.0, hero());
        let far = w.spawn_ground_item(
            Vec2::new(5.5, 1.5),
            ItemStack {
                item: POTION,
                count: 1,
            },
        );
        let mut auth = LocalAuthority::new(w);
        let events = submit(&mut auth, Intent::PickUp { unit: h, item: far });
        assert_eq!(rejected(&events), Some(Rejection::OutOfRange));
        assert!(
            auth.world().ground_item(far).is_some(),
            "거절됐는데 사라졌다"
        );
    }

    #[test]
    fn full_bag_rejects_the_whole_pile() {
        let mut w = world();
        let h = w.spawn_unit(Vec2::new(1.5, 1.5), 0.0, hero());
        // 소모품 가방 120칸을 다른 아이템으로 채우고, 물약 한 칸만 9/10 으로 둔다.
        assert!(w.give_item(h, JELLY, 99 * (BAG_SLOTS as u32 - 1)));
        assert!(w.give_item(h, POTION, 9));
        let pile = w.spawn_ground_item(
            Vec2::new(1.5, 1.5),
            ItemStack {
                item: POTION,
                count: 2,
            },
        );
        let mut auth = LocalAuthority::new(w);
        let events = submit(
            &mut auth,
            Intent::PickUp {
                unit: h,
                item: pile,
            },
        );
        assert_eq!(rejected(&events), Some(Rejection::InventoryFull));
        assert_eq!(bag(&auth, h, BagKind::Consumable).count_of(POTION), 9);
        assert_eq!(
            auth.world().ground_item(pile).unwrap().stack.count,
            2,
            "일부만 주웠다"
        );
    }

    #[test]
    fn drop_puts_the_whole_slot_at_the_feet() {
        let mut w = world();
        let feet = Vec2::new(3.5, 4.5);
        let h = w.spawn_unit(feet, 0.0, hero());
        assert!(w.give_item(h, POTION, 4));
        let mut auth = LocalAuthority::new(w);
        let events = submit(
            &mut auth,
            Intent::DropItem {
                unit: h,
                bag: BagKind::Consumable,
                slot: 0,
            },
        );
        let pile = spawned(&events);
        assert_eq!(pile.len(), 1);
        let g = auth.world().ground_item(pile[0]).unwrap();
        assert_eq!((g.pos, g.stack.count), (feet, 4));
        assert_eq!(bag(&auth, h, BagKind::Consumable).get(0), None);

        let empty = submit(
            &mut auth,
            Intent::DropItem {
                unit: h,
                bag: BagKind::Consumable,
                slot: 0,
            },
        );
        assert_eq!(rejected(&empty), Some(Rejection::EmptySlot));
    }

    // ── 사용 · 장착 ──────────────────────────────────────────────────────────

    #[test]
    fn potion_heals_up_to_max_and_is_consumed() {
        let mut w = world();
        let h = w.spawn_unit(Vec2::new(1.5, 1.5), 0.0, hero());
        let foe = w.spawn_unit(
            Vec2::new(2.5, 1.5),
            0.0,
            UnitDef {
                attack: 90,
                ..hero()
            },
        );
        assert!(w.give_item(h, POTION, 2));
        let mut auth = LocalAuthority::new(w);
        // 90 맞고 HP 10.
        submit(
            &mut auth,
            Intent::Attack {
                unit: foe,
                target: h,
                skill: HIT,
            },
        );
        let events = submit(&mut auth, Intent::UseItem { unit: h, slot: 0 });
        assert!(events.contains(&Event::Healed {
            unit: h,
            amount: 30,
            remaining_hp: 40
        }));
        assert_eq!(bag(&auth, h, BagKind::Consumable).count_of(POTION), 1);

        // 남은 하나로 40 → 70. 그 뒤로는 칸이 비었다.
        submit(&mut auth, Intent::UseItem { unit: h, slot: 0 });
        assert_eq!(auth.world().unit(h).unwrap().hp(), 70);
        let none_left = submit(&mut auth, Intent::UseItem { unit: h, slot: 0 });
        assert_eq!(rejected(&none_left), Some(Rejection::EmptySlot));
    }

    #[test]
    fn heal_never_exceeds_max_hp() {
        let mut w = world();
        let h = w.spawn_unit(Vec2::new(1.5, 1.5), 0.0, hero());
        assert!(w.give_item(h, POTION, 1));
        let mut auth = LocalAuthority::new(w);
        let events = submit(&mut auth, Intent::UseItem { unit: h, slot: 0 });
        assert!(events.contains(&Event::Healed {
            unit: h,
            amount: 0,
            remaining_hp: 100
        }));
        assert_eq!(bag(&auth, h, BagKind::Consumable).count_of(POTION), 0);
    }

    #[test]
    fn equipment_adds_attack_swaps_in_place_and_comes_off() {
        let mut w = world();
        let h = w.spawn_unit(Vec2::new(1.5, 1.5), 0.0, hero());
        assert!(w.give_item(h, SWORD, 1)); // 장비 가방 0번
        assert!(w.give_item(h, AXE, 1)); // 1번
        let mut auth = LocalAuthority::new(w);
        let unit = |auth: &LocalAuthority| auth.world().unit(h).unwrap().attack();
        assert_eq!(unit(&auth), 10);

        submit(&mut auth, Intent::Equip { unit: h, slot: 0 });
        assert_eq!(unit(&auth), 25, "기본 10 + 검 15");
        let inv = auth.world().unit(h).unwrap().inventory();
        assert_eq!(inv.equipped(EquipSlot::Weapon), Some(SWORD));
        assert_eq!(inv.bag(BagKind::Equipment).get(0), None);

        // 도끼를 끼면 검은 도끼가 있던 1번 칸으로 돌아간다.
        submit(&mut auth, Intent::Equip { unit: h, slot: 1 });
        assert_eq!(unit(&auth), 35);
        let inv = auth.world().unit(h).unwrap().inventory();
        assert_eq!(inv.equipped(EquipSlot::Weapon), Some(AXE));
        assert_eq!(
            inv.bag(BagKind::Equipment).get(1).map(|s| s.item),
            Some(SWORD)
        );

        let events = submit(
            &mut auth,
            Intent::Unequip {
                unit: h,
                slot: EquipSlot::Weapon,
            },
        );
        assert!(events.contains(&Event::EquipmentChanged {
            unit: h,
            slot: EquipSlot::Weapon
        }));
        assert_eq!(unit(&auth), 10);
        assert_eq!(bag(&auth, h, BagKind::Equipment).count_of(AXE), 1);
    }

    #[test]
    fn equipment_changes_damage_dealt() {
        let mut w = world();
        let h = w.spawn_unit(Vec2::new(1.5, 1.5), 0.0, hero());
        let dummy = w.spawn_unit(
            Vec2::new(2.5, 1.5),
            0.0,
            UnitDef {
                max_hp: 1000,
                ..UnitDef::default()
            },
        );
        assert!(w.give_item(h, SWORD, 1));
        let mut auth = LocalAuthority::new(w);
        submit(&mut auth, Intent::Equip { unit: h, slot: 0 });
        let events = submit(
            &mut auth,
            Intent::Attack {
                unit: h,
                target: dummy,
                skill: HIT,
            },
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::Damaged { amount: 25, .. })),
            "{events:?}"
        );
    }

    #[test]
    fn unequip_needs_a_free_slot() {
        let mut w = world();
        let h = w.spawn_unit(Vec2::new(1.5, 1.5), 0.0, hero());
        assert!(w.give_item(h, SWORD, 1));
        let mut auth = LocalAuthority::new(w);
        submit(&mut auth, Intent::Equip { unit: h, slot: 0 });
        for _ in 0..BAG_SLOTS {
            assert!(auth.world_mut().give_item(h, AXE, 1));
        }
        let events = submit(
            &mut auth,
            Intent::Unequip {
                unit: h,
                slot: EquipSlot::Weapon,
            },
        );
        assert_eq!(rejected(&events), Some(Rejection::InventoryFull));
        assert_eq!(auth.world().unit(h).unwrap().attack(), 25, "벗겨졌다");
    }

    #[test]
    fn the_dead_cannot_use_items() {
        let mut w = world();
        let h = w.spawn_unit(
            Vec2::new(1.5, 1.5),
            0.0,
            UnitDef {
                max_hp: 1,
                ..hero()
            },
        );
        let foe = w.spawn_unit(Vec2::new(2.5, 1.5), 0.0, hero());
        assert!(w.give_item(h, POTION, 1));
        let mut auth = LocalAuthority::new(w);
        submit(
            &mut auth,
            Intent::Attack {
                unit: foe,
                target: h,
                skill: HIT,
            },
        );
        let events = submit(&mut auth, Intent::UseItem { unit: h, slot: 0 });
        assert_eq!(rejected(&events), Some(Rejection::Dead));
    }

    // ── 결정성 · 핸들 ────────────────────────────────────────────────────────

    #[test]
    fn same_seed_same_drops() {
        let run = |seed: u64| {
            let mut w = world();
            w.set_seed(seed);
            w.define_loot(
                SLIME_LOOT,
                vec![LootEntry {
                    item: JELLY,
                    count: 1,
                    chance_per_mille: 500,
                }],
            );
            let h = w.spawn_unit(Vec2::new(10.0, 10.0), 0.0, hero());
            let slimes: Vec<Entity> = (0..20)
                .map(|_| w.spawn_unit(Vec2::new(10.5, 10.0), 0.0, slime()))
                .collect();
            let mut auth = LocalAuthority::new(w);
            let mut drops = Vec::new();
            for s in slimes {
                let events = submit(
                    &mut auth,
                    Intent::Attack {
                        unit: h,
                        target: s,
                        skill: HIT,
                    },
                );
                drops.push(!spawned(&events).is_empty());
                // 쿨타임이 끝나도록 기다린다.
                for _ in 0..10 {
                    auth.tick(DT);
                }
            }
            drops
        };
        assert_eq!(run(7), run(7));
        assert_ne!(run(7), run(8), "시드가 결과에 영향을 주지 않는다");
    }

    #[test]
    fn ground_items_and_units_share_handles_without_mixing() {
        let mut w = world();
        let h = w.spawn_unit(Vec2::ZERO, 0.0, hero());
        let g = w.spawn_ground_item(
            Vec2::ZERO,
            ItemStack {
                item: POTION,
                count: 1,
            },
        );
        assert_ne!(h, g);
        assert!(w.unit(g).is_none(), "아이템 핸들로 유닛이 조회됐다");
        assert!(w.ground_item(h).is_none(), "유닛 핸들로 아이템이 조회됐다");
        assert_eq!(w.unit_count(), 1);
        assert_eq!(w.ground_items().count(), 1);
    }
}
