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


class ItemUIDGenerator
{
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


