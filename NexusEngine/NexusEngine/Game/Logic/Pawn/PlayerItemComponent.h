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

class ItemBagComponent
{
public:
    ItemBagComponent() = default;
    ~ItemBagComponent() = default;

public:
    SkinBag       skinBag;
    EquipmentBag  equipmentBag;
    ConsumableBag consumableBag;
};

class ItemSlotComponent
{
public:
    ItemSlotComponent() = default;
    ~ItemSlotComponent() = default;

    std::array<EquipmentSlot,  static_cast<int>(EQUIPMENT_POS_TYPE::_END)> CurrentEquipments{};
    std::array<SkinSlot,       static_cast<int>(SKIN_PARTS_TYPE::_END)>    CurrentSkins{};
    std::array<ConsumableSlot, MAX_QUICKSLOT_CONSUMABLE>                   ConsumableQuickSlot{};
};

class ItemComponent
{
public:
    ItemComponent()  = default;
    ~ItemComponent() = default;

public:
    // Sub Components

    ItemBagComponent itemBag;
    ItemSlotComponent itemSlot;

public:
    // Bag Action
    


public:
    // Slot Action
    
};
