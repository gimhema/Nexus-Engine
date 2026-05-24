#pragma once

#include <array>
#include <algorithm>
#include "../Equipment/Equipment.h"
#include "../Skin/Skin.h"

constexpr int MAX_SKIN_SLOT      = 6;
constexpr int MAX_EQUIPMENT_SLOT = 10;

// ─────────────────────────────────────────────────────────────────────────────
// ItemSlot<TItem, MaxSlots> — 장착 슬롯 베이스
//
// Get/Swap 은 공통 구현 제공.
// Sync() 는 자식 클래스에서 DB 동기화 로직 구현.
// ─────────────────────────────────────────────────────────────────────────────
template<typename TItem, size_t MaxSlots>
class ItemSlot
{
protected:
    std::array<TItem*, MaxSlots> m_slots{};

public:
    virtual ~ItemSlot() = default;

    TItem* Get(int pos)     const { return m_slots[pos]; }
    void   Swap(int a, int b)     { std::swap(m_slots[a], m_slots[b]); }
    bool   IsEmpty(int pos) const { return m_slots[pos] == nullptr; }

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
