#pragma once

#include "../Connection.h"
#include "../GameDataEntities/EntityBase.h"
#include <unordered_map>
#include <string>

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

struct UserIndentification
{
    std::string UserID;
    std::string UserPW;
};

struct UserProfile
{
    std::string UserName;
};

class User
{
    // Private Member
    private:
    uint16_t uId; // 실질적인 유저의 ID, 연결된 소켓의 식별자
    Session UserSession;
    UserIndentification userIdentification;
    UserProfile userProfile;

    // Public Member
    public:
    UserGameData userGameData;

    // Constructor & Deconstructor
    public:
    User();
    ~User();

    // Methods
    public:

};
