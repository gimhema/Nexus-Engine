#pragma once

#include <unordered_map>
#include <array>
#include "CommonLogicType.h"

#define MAX_CONSUMABLE_BAG_NUM 120
#define MAX_SKIN_BAG_NUM 120
#define MAX_EQUIPMENT_BAG_NUM 120

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

};