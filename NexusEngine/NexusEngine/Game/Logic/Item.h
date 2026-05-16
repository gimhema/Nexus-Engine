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
	static std::atomic<unsigned int> s_Sequence;

public:
	static unsigned long long Generate(unsigned short serverId)
	{
		unsigned long long uid = 0;

		unsigned long long timestamp = static_cast<unsigned long long>(time(NULL));
		unsigned long long seq = s_Sequence.fetch_add(1, std::memory_order_relaxed);

		uid |= (static_cast<unsigned long long>(serverId) << 48);
		uid |= ((timestamp & 0xFFFFFFFF) << 16);
		uid |= (seq & 0xFFFF);

		return uid;
	}
    
};

// std::atomic<unsigned int> ItemUIDGenerator::s_Sequence(0);

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


