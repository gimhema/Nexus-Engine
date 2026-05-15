#pragma once


enum class ITEM_TYPE
{
    DEFAULT,
    CONSUMABLE,
    EQUIPMENT,
    SKIN,
};

struct ItemBasicInfo
{
        uint64_t  itemID;
        ITEM_TYPE itemType;
        int       itemUnique;
};

class ItemBase
{
    public:
        ItemBase()
        {

        }
        ~ItemBase()
        {

        }
    public:
        ItemBasicInfo itemBasicInfo;
};
