#pragma once

#include <array>
#include <algorithm>
#include "Slot.h"
#include "../Consumable/Consumable.h"
#include "../Equipment/Equipment.h"
#include "../Skin/Skin.h"

constexpr int MAX_CONSUMABLE_BAG_NUM = 120;
constexpr int MAX_SKIN_BAG_NUM       = 120;
constexpr int MAX_EQUIPMENT_BAG_NUM  = 120;

// ─────────────────────────────────────────────────────────────────────────────
// ItemBag<TItem, MaxSlots> — 타입별 가방 베이스
//
// 각 칸은 Slot<TItem> 으로 관리되어 빈 칸을 nullptr 대신 IsEmpty() 로 구분한다.
// Add/Get/Drop/Swap 은 공통 구현 제공.
// Sync() 는 자식 클래스에서 DB 동기화 로직 구현.
// ─────────────────────────────────────────────────────────────────────────────
template<typename TItem, size_t MaxSlots>
class ItemBag
{
protected:
    std::array<Slot<TItem>, MaxSlots> m_slots{};

public:
    virtual ~ItemBag() = default;

    void Add(TItem* item, int pos) { m_slots[pos].Equip(item); }
    void Drop(int pos)             { m_slots[pos].Unequip(); }
    void Swap(int a, int b)        { std::swap(m_slots[a], m_slots[b]); }

    [[nodiscard]] Slot<TItem>&       GetSlot(int pos)       { return m_slots[pos]; }
    [[nodiscard]] const Slot<TItem>& GetSlot(int pos) const { return m_slots[pos]; }

    [[nodiscard]] bool IsEmpty(int pos) const { return m_slots[pos].IsEmpty(); }
    [[nodiscard]] bool HasItem(int pos) const { return m_slots[pos].HasItem(); }

    virtual void Sync() = 0;
};

// ─────────────────────────────────────────────────────────────────────────────
// 타입별 가방
// ─────────────────────────────────────────────────────────────────────────────
class SkinBag : public ItemBag<Skin, MAX_SKIN_BAG_NUM>
{
public:
    void Sync() override;
};

class EquipmentBag : public ItemBag<Equipment, MAX_EQUIPMENT_BAG_NUM>
{
public:
    void Sync() override;
};

class ConsumableBag : public ItemBag<Consumable, MAX_CONSUMABLE_BAG_NUM>
{
public:
    void Sync() override;
};
