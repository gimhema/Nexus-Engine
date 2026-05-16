#pragma once

#include <chrono>
#include <functional>
#include <string>
#include <unordered_map>
#include <vector>
#include <atomic>


// class Singleton {
// private:
//     Singleton() {}
//     Singleton(const Singleton& ref) {}
//     Singleton& operator=(const Singleton& ref) {}
//     ~Singleton() {}
// public:
//     static Singleton& getIncetance() {
//         static Singleton s;
//         return s;
//     }
// };

// int main(void) {
//     Singleton& s = Singleton::getIncetance();
//     return 0;
// }


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
    ItemIDGenerator(){}
    ~ItemIDGenerator(){}
    ItemIDGenerator(const ItemIDGenerator& ref) {}
    ItemIDGenerator& operator=(const ItemIDGenerator& ref) {}

public:
    static ItemIDGenerator& getIncetance() {
        static ItemIDGenerator s;
        return s;
    }

private:
	static std::atomic<uint64_t> s_Sequence;

public:
	static uint64_t Generate(unsigned short serverId)
	{
		uint64_t uid = 0;

		uint64_t timestamp = static_cast<uint64_t>(time(NULL));
		uint64_t seq = s_Sequence.fetch_add(1, std::memory_order_relaxed);

		uid |= (static_cast<uint64_t>(serverId) << 48);
		uid |= ((timestamp & 0xFFFFFFFF) << 16);
		uid |= (seq & 0xFFFF);

		return uid;
	}

    void Init()
    {

    }
    
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


