#pragma once

#include <array>
#include "../CommonLogicType.h"
#include "../Consumable/Consumable.h"
#include "../Equipment/Equipment.h"
#include "../Skin/Skin.h"
#include "Slot.h"
#include "ItemBag.h"
#include "ItemSlot.h"

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
    std::array<Slot<Skin>,       static_cast<int>(SKIN_PARTS_TYPE::_END)>    CurrentSkins{};
    std::array<Slot<Equipment>,  static_cast<int>(EQUIPMENT_POS_TYPE::_END)> CurrentEquipments{};
    std::array<Slot<Consumable>, MAX_QUICKSLOT_CONSUMABLE>                   ConsumableQuickSlot{};
};
