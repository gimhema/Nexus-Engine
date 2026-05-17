#pragma once

#include <array>
#include "CommonLogicType.h"

#define MAX_CONSUMABLE_BAG_NUM 120
#define MAX_SKIN_BAG_NUM 120
#define MAX_EQUIPMENT_BAG_NUM 120

#define MAX_QUICKSLOT_CONSUMBALE 10

class Skin;
class Equipment;
class Consumbale;

class ItemComponent
{
    public:
        ItemComponent(){}
        ~ItemComponent(){}

    // Bag
    public:
        std::array<Skin*, MAX_SKIN_BAG_NUM> SkinBag;
        std::array<Equipment*, MAX_EQUIPMENT_BAG_NUM> EquipmentBag;
        std::array<Consumbale*, MAX_CONSUMABLE_BAG_NUM> ConsumableBag;

    // Use Slot
    public:
        std::array<Skin*, static_cast<int>(SKIN_PARTS_TYPE::_END)> CurrentSkins;
        std::array<Equipment*, static_cast<int>(EQUIPMENT_POS_TYPE::_END)> CurrentEquipments;
        std::array<Consumbale*, MAX_QUICKSLOT_CONSUMBALE> ConsubaleQuickSlot;        
};