#pragma once

#include "Slot.h"
#include "../Consumable/Consumable.h"

// ─────────────────────────────────────────────────────────────────────────────
// ConsumableSlot — 소모품 퀵슬롯 단위 슬롯
//
// Equip   : 퀵슬롯에 소모품 등록
// Unequip : 퀵슬롯에서 소모품 제거
// Use     : 소모품 사용 (HasItem() 확인 후 호출)
// ─────────────────────────────────────────────────────────────────────────────
class ConsumableSlot : public Slot<Consumable>
{
public:
    ConsumableSlot()          = default;
    ~ConsumableSlot() override = default;

    void Equip(Consumable* consumable) override;
    void Unequip()                     override;

    void Use();
};
