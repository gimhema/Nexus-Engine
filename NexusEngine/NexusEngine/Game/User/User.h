#pragma once

#include "../Connection.h"
#include "../GameDataEntities/EntityBase.h"
#include <unordered_map>

namespace EUser
{
    enum USER_STATUS
    {
        _DEFAULT = 0,
        _CONNETED = 1,
        _WAIT_DISCONNECTED = 2
    };
}

struct UserGameData
{
    std::unordered_map<EEntity::EID, GameDataEnttiyBase> gameDataContainer;
};

struct UserProfile
{

};

class User
{
    // User Connection Info
    private:
    uint16_t uId; // 실질적인 유저의 ID, 연결된 소켓의 식별자
    Session UserSession;

    // User Game Logic Info
    public:
    UserGameData userGameData;

    // Constructor & Deconstructor
    public:
    User();
    ~User();

    // Methods
    public:

};
