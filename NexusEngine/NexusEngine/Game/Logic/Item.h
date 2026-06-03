#pragma once

#include <chrono>
#include <functional>
#include <string>
#include <unordered_map>
#include <vector>
#include <atomic>

#include "CommonLogicType.h"

class PlayerPawn;

struct ItemBasicInfo
{
    uint64_t  ownerID;
    uint64_t  itemID;
    ITEM_TYPE itemType;
    int       itemUnique;
    ITEM_GRADE_TYPE itemGrade;

    ItemBasicInfo()
    {
        ownerID = 0;
        itemID = 0;
        itemType = ITEM_TYPE::DEFAULT;
        itemUnique = 0;
        itemGrade = ITEM_GRADE_TYPE::_DEFAULT;
    }
};

class ItemBase
{
    public:
        explicit ItemBase(ITEM_TYPE type = ITEM_TYPE::DEFAULT)
        {
            itemBasicInfo.itemType = type;
        }
        virtual ~ItemBase() = default;

    private:
        PlayerPawn*   Owner = nullptr;
        ItemBasicInfo itemBasicInfo;

    protected:
        virtual void Create(ItemBasicInfo iInfo) {}
        virtual void Use() {}

    public:
        ITEM_TYPE   GetItemType() const { return itemBasicInfo.itemType; }
        uint64_t    GetItemId()   const { return itemBasicInfo.itemID; }

        void        SetOwner(PlayerPawn* ptr) { Owner = ptr; }
        PlayerPawn* GetOwner()                { return Owner; }
};


class ItemUIDGenerator
{
    // 게임로직 초기화시
    // ItemUIDGenerator::Instance().Initialize(1);
private:

	std::atomic<unsigned int> m_Sequence;

	unsigned short m_ServerId;

	bool m_Initialized;

private:

	ItemUIDGenerator();

	~ItemUIDGenerator();

	ItemUIDGenerator(const ItemUIDGenerator&);
	ItemUIDGenerator& operator=(const ItemUIDGenerator&);


public:

	static ItemUIDGenerator& Instance();

public:

	bool Initialize(unsigned short serverId);

	bool LoadFromLastUID(uint64_t lastUID);

	uint64_t Generate();
};



