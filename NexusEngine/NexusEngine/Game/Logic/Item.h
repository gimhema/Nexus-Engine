#pragma once

#include <chrono>
#include <functional>
#include <string>
#include <unordered_map>
#include <vector>
#include <atomic>

enum class ITEM_TYPE
{
    DEFAULT,
    CONSUMABLE,
    EQUIPMENT,
    SKIN,
};

struct ItemBasicInfo
{
        uint64_t  ownerID;
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


class ItemIDGenerator
{
    public:
        ItemIDGenerator()
        {
            
        }
        ~ItemIDGenerator()
        {

        }
    public:
};

class ItemCreator
{
    public:
        ItemCreator()
        {
            
        }
        ~ItemCreator()
        {

        }
    public:
};


