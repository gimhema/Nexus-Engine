#pragma once
#include "ServerNet.h"
#include "Game/Actors/WorldActor.h"
#include "Game/Actors/ZoneActor.h"
#include <atomic>
#include <memory>

class Server
{
public:
    Server();
    ~Server();

    Server(const Server&)            = delete;
    Server& operator=(const Server&) = delete;

    // 서버를 시작하고 종료 신호가 올 때까지 블로킹.
    void Run();

    // 실행 중인 서버를 종료하도록 신호.
    // 시그널 핸들러나 다른 스레드에서 호출 가능.
    void Exit();

private:
    NetworkManager                   m_net;
    std::atomic<bool>                m_running{ false };

    WorldActor                       m_world;
    std::shared_ptr<ZoneActor>       m_zone;   // 기본 존 (zoneId=1)
};

