#pragma once

#include <chrono>
#include <functional>
#include <string>
#include <unordered_map>
#include <vector>
#include <atomic>

#include "CommonLogicType.h"
#include "PlayerPawn.h"

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

    ItemBasicInfo()
    {
        ownerID = 0;
        itemID = 0;
        itemType = ITEM_TYPE::DEFAULT;
        itemUnique = 0;
    }
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
    private:
        PlayerPawn* Owner = nullptr;
        ItemBasicInfo itemBasicInfo;    

    protected:
        virtual void Create(ItemBasicInfo iInfo) {}
        virtual void Use() {}

    public:
        void SetOwner(PlayerPawn* _ptr) { Owner = _ptr; }
        PlayerPawn* GetOwner() {return Owner;}
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



