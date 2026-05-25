#pragma once

#include <array>
#include "../CommonLogicType.h"
#include "ItemBag.h"
#include "EquipmentSlot.h"
#include "SkinSlot.h"
#include "ConsumableSlot.h"

constexpr int MAX_QUICKSLOT_CONSUMABLE = 10;

// ─────────────────────────────────────────────────────────────────────────────
// ItemComponent — PlayerPawn 이 보유하는 아이템 컴포넌트
// ─────────────────────────────────────────────────────────────────────────────
class ItemComponent
{
public:
    ItemComponent()  = default;
    ~ItemComponent() = default;

    // ── 가방 ─────────────────────────────────────────────────────────────────
    SkinBag       skinBag;
    EquipmentBag  equipmentBag;
    ConsumableBag consumableBag;

    // ── 장착 슬롯 ─────────────────────────────────────────────────────────────
    std::array<EquipmentSlot,  static_cast<int>(EQUIPMENT_POS_TYPE::_END)> CurrentEquipments{};
    std::array<SkinSlot,       static_cast<int>(SKIN_PARTS_TYPE::_END)>    CurrentSkins{};
    std::array<ConsumableSlot, MAX_QUICKSLOT_CONSUMABLE>                   ConsumableQuickSlot{};
};
