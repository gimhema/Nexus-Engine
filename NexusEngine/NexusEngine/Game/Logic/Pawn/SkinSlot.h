#pragma once

#include "Slot.h"
#include "../Skin/Skin.h"

// ─────────────────────────────────────────────────────────────────────────────
// SkinSlot — 외형 단위 슬롯
//
// Equip : 외형 장착 + 렌더링 반영
// Unequip : 외형 해제 + 렌더링 원복
// ─────────────────────────────────────────────────────────────────────────────
class SkinSlot : public Slot<Skin>
{
public:
    SkinSlot()          = default;
    ~SkinSlot() override = default;

    void Equip(Skin* skin) override;
    void Unequip()         override;
};
