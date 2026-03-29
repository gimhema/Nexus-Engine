#pragma once

#include "../Connection.h"
#include "../GameDataEntities/EntityBase.h"

class User
{
    // User Connection Info
    private:
    uint16_t uId; // 실질적인 유저의 ID, 연결된 소켓의 식별자
    Session UserSession;

    // User Game Logic Info
    public:

    // Constructor & Deconstructor
    public:
    User();
    ~User();

    // Methods
    public:

};
