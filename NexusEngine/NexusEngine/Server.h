#pragma once
#include "ServerNet.h"
#include "Arbiter/Arbiter.h"
#include "Game/Actors/WorldActor.h"
#include "Game/Actors/ZoneActor.h"
#include "Game/Actors/SessionActor.h"
#include "Game/Logic/GameLogic.h"
#include "Game/Zone/Zone.h"
#include <atomic>
#include <memory>
#include <mutex>
#include <unordered_map>

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
    Arbiter                          m_arbiter;
    std::atomic<bool>                m_running{ false };
    std::atomic<uint32_t>            m_playerCount{ 0 };  // Arbiter 상태 조회용

    WorldActor                       m_world;
    std::shared_ptr<ZoneActor>       m_zone;        // 기본 존 (zoneId=1)
    GameLogic                        m_gameLogic;   // 월드 시간 + 이벤트 루프

    // sessionId → SessionActor (워커 스레드에서 접근하므로 mutex 보호)
    std::mutex                                                   m_sessionActorsMutex;
    std::unordered_map<uint64_t, std::shared_ptr<SessionActor>> m_sessionActors;

    // sessionId → 캐릭터명 (Arbiter GetPlayers 응답용 — WorldActor 콜백에서 갱신)
    std::mutex                                                   m_playerRegistryMutex;
    std::unordered_map<uint64_t, std::string>                   m_playerRegistry;
};

