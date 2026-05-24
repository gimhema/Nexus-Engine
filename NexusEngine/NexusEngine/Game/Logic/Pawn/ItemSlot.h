#pragma once

#include <array>
#include <algorithm>
#include "Slot.h"
#include "../Equipment/Equipment.h"
#include "../Skin/Skin.h"

constexpr int MAX_SKIN_SLOT      = 6;
constexpr int MAX_EQUIPMENT_SLOT = 10;

// ─────────────────────────────────────────────────────────────────────────────
// ItemSlot<TItem, MaxSlots> — 장착 슬롯 베이스
//
// 각 칸은 Slot<TItem> 으로 관리되어 빈 칸을 nullptr 대신 IsEmpty() 로 구분한다.
// Sync() 는 자식 클래스에서 DB 동기화 로직 구현.
// ─────────────────────────────────────────────────────────────────────────────
template<typename TItem, size_t MaxSlots>
class ItemSlot
{
protected:
    std::array<Slot<TItem>, MaxSlots> m_slots{};

public:
    virtual ~ItemSlot() = default;

    void Swap(int a, int b) { std::swap(m_slots[a], m_slots[b]); }

    [[nodiscard]] Slot<TItem>&       GetSlot(int pos)       { return m_slots[pos]; }
    [[nodiscard]] const Slot<TItem>& GetSlot(int pos) const { return m_slots[pos]; }

    [[nodiscard]] bool IsEmpty(int pos) const { return m_slots[pos].IsEmpty(); }
    [[nodiscard]] bool HasItem(int pos) const { return m_slots[pos].HasItem(); }

    virtual void Sync() = 0;
};

// ─────────────────────────────────────────────────────────────────────────────
// 타입별 슬롯
// ─────────────────────────────────────────────────────────────────────────────
class EquipmentSlot : public ItemSlot<Equipment, MAX_EQUIPMENT_SLOT>
{
public:
    void Sync() override;
};

class SkinSlot : public ItemSlot<Skin, MAX_SKIN_SLOT>
{
public:
    void Sync() override;
};
