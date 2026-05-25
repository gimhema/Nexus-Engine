#pragma once

#include "Slot.h"
#include "../Equipment/Equipment.h"

// ─────────────────────────────────────────────────────────────────────────────
// EquipmentSlot — 장비 단위 슬롯
//
// Equip : 장비 장착 + 스탯 적용
// Unequip : 장비 해제 + 스탯 제거
// ─────────────────────────────────────────────────────────────────────────────
class EquipmentSlot : public Slot<Equipment>
{
public:
    EquipmentSlot()          = default;
    ~EquipmentSlot() override = default;

    void Equip(Equipment* equipment) override;
    void Unequip()                   override;
};
