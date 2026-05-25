#pragma once

#include <array>
#include <algorithm>
#include "Slot.h"

// ─────────────────────────────────────────────────────────────────────────────
// ItemSlot<TSlot, MaxSlots> — 슬롯 배열 베이스
//
// TSlot 은 Slot<T> 를 상속한 구체 슬롯 타입 (EquipmentSlot, SkinSlot 등).
// 공통 배열 접근 / 교환 / 상태 조회 구현을 제공한다.
// Sync() 는 자식 클래스에서 DB 동기화 로직 구현.
// ─────────────────────────────────────────────────────────────────────────────
template<typename TSlot, size_t MaxSlots>
class ItemSlot
{
protected:
    std::array<TSlot, MaxSlots> m_slots{};

public:
    virtual ~ItemSlot() = default;

    void Swap(int a, int b) { std::swap(m_slots[a], m_slots[b]); }

    [[nodiscard]] TSlot&       GetSlot(int pos)       { return m_slots[pos]; }
    [[nodiscard]] const TSlot& GetSlot(int pos) const { return m_slots[pos]; }

    [[nodiscard]] bool IsEmpty(int pos) const { return m_slots[pos].IsEmpty(); }
    [[nodiscard]] bool HasItem(int pos) const { return m_slots[pos].HasItem(); }

    virtual void Sync() = 0;
};
