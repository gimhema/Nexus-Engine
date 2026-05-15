#pragma once


enum class ITEM_TYPE
{
    DEFAULT,
    CONSUMABLE,
    EQUIPMENT,
    SKIN,
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
        ITEM_TYPE itemType;
        int       itemUnique;
};
