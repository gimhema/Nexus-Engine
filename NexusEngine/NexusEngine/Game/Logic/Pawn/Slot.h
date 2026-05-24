#pragma once

#include <cassert>

// ─────────────────────────────────────────────────────────────────────────────
// Slot<TItem> — 단위 슬롯
//
// nullptr 직접 노출 없이 비어있음/채워짐 상태를 명시적으로 표현한다.
//
// 사용 패턴:
//   if (slot.HasItem())
//       slot.Item().DoSomething();
//
//   slot.Equip(&item);
//   slot.Unequip();
// ─────────────────────────────────────────────────────────────────────────────
template<typename TItem>
class Slot
{
public:
    Slot()  = default;
    ~Slot() = default;

    Slot(const Slot&)            = default;
    Slot& operator=(const Slot&) = default;

    // ── 상태 조회 ─────────────────────────────────────────────────────────────
    [[nodiscard]] bool HasItem() const { return m_item != nullptr; }
    [[nodiscard]] bool IsEmpty() const { return m_item == nullptr; }

    // ── 아이템 접근 ───────────────────────────────────────────────────────────
    // HasItem() 확인 후 호출해야 한다.
    [[nodiscard]] TItem&       Item()       { assert(HasItem()); return *m_item; }
    [[nodiscard]] const TItem& Item() const { assert(HasItem()); return *m_item; }

    // ── 장착 / 해제 ───────────────────────────────────────────────────────────
    void Equip(TItem* item) { m_item = item; }
    void Unequip()          { m_item = nullptr; }

private:
    TItem* m_item = nullptr;
};
