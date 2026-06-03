#pragma once

#include <array>
#include "../CommonLogicType.h"
#include "ItemBag.h"
#include "EquipmentSlot.h"
#include "SkinSlot.h"
#include "ConsumableSlot.h"

constexpr int MAX_QUICKSLOT_CONSUMABLE = 10;

// SwapSkin 결과 — ZoneActor가 브로드캐스트 시 사용
struct SkinSwapResult
{
    bool     success{ false };
    uint8_t  partsType{ 0 };   // SKIN_PARTS_TYPE (uint8_t 캐스트)
    uint64_t itemId{ 0 };      // 장착된 스킨 아이템 ID
};

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

private:
    // Sub Components
    ItemBagComponent itemBag;
    ItemSlotComponent itemSlot;

public:
    // Call by Player Action

    // Item CRUD
    void Store(ItemBase* pickuped);
    void DropItem(ITEM_TYPE bagType, int pos);

    // ItemBag Interaction
    void           SwapEquip(int pos);
    SkinSwapResult SwapSkin(int pos);
    void           UseConsumbale(int pos);

    // ItemSlot Interaction
    

private:
    // Pure Inner Action
    
};
