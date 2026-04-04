#pragma once

#include "../../Connection.h"
#include "../Data/GameDataEntities/EntityBase.h"
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
    std::shared_ptr<Session> m_session; // 연결된 세션 (ID는 Session::GetSessionId()로 참조)
    UserIndentification userIdentification;
    UserProfile userProfile;

    // Public Member
    public:
    UserGameData userGameData;

    // Constructor & Deconstructor
    public:
    explicit User(std::shared_ptr<Session> session);
    ~User();

    // Methods
    public:
    [[nodiscard]] uint64_t GetSessionId() const { return m_session->GetSessionId(); }
};
