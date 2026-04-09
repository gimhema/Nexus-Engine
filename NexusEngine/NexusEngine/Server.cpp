#include "Server.h"
#include "Shared/Logger.h"
#include "Actor/ActorSystem.h"
#include "Game/Messages/GameMessages.h"

#include <csignal>
#include <chrono>
#include <thread>

// ─────────────────────────────────────────────────────────────────────────────
// 시그널 핸들러에서 인스턴스 메서드를 호출하기 위한 파일-스코프 포인터.
// Run() 진입 시 설정, 반환 시 해제.
// ─────────────────────────────────────────────────────────────────────────────
static Server* g_server = nullptr;

static void OnSignal(int /*sig*/)
{
    if (g_server)
        g_server->Exit();
}

// ─────────────────────────────────────────────────────────────────────────────
// Windows: Ctrl+C / 창 닫기 이벤트 처리
// ─────────────────────────────────────────────────────────────────────────────
#ifdef _WIN32
#include <windows.h>
static BOOL WINAPI OnConsoleCtrl(DWORD /*type*/)
{
    if (g_server)
        g_server->Exit();
    return TRUE;
}
#endif

Server::Server()  = default;
Server::~Server() = default;

void Server::Run()
{
    g_server = this;

    // ── 종료 시그널 등록 ─────────────────────────────────────────────────────
#ifdef _WIN32
    SetConsoleCtrlHandler(OnConsoleCtrl, TRUE);
#else
    std::signal(SIGINT,  OnSignal);   // Ctrl+C
    std::signal(SIGTERM, OnSignal);   // kill
#endif

    // ── 로거 초기화 ──────────────────────────────────────────────────────────
    Logger::Init("nexus.log", LogLevel::Debug);

    // ── ActorSystem (Pooled Actor 스레드 풀) 시작 ────────────────────────────
    ActorSystem::Instance().Start();

    // ── WorldActor 시작 ──────────────────────────────────────────────────────
    m_world.Start();

    // ── 기본 Zone 생성 및 등록 (zoneId=1) ────────────────────────────────────
    constexpr uint32_t DEFAULT_ZONE_ID      = 1;
    constexpr uint32_t ZONE_TICK_INTERVAL   = 50;   // 50ms = 20Hz

    m_zone = std::make_shared<ZoneActor>(DEFAULT_ZONE_ID, m_world);
    m_zone->StartWithTick(ZONE_TICK_INTERVAL);
    m_world.RegisterZone(DEFAULT_ZONE_ID, m_zone);

    // ── GameLogic 설정 및 시작 ────────────────────────────────────────────────
    // 시간 비율: 실 60초 = 게임 1시간 → 하루 = 실 24분
    constexpr uint32_t GAMELOGIC_TICK_INTERVAL = 1000;  // 1초 tick

    m_gameLogic.SetRealSecondsPerGameHour(60.f);
    m_gameLogic.SetStartHour(8.0f);
    m_gameLogic.SetDayNightHours(6.0f, 20.0f);

    m_gameLogic.RegisterEvent({ WorldEventId::DayBegin,     "낮 시작",   6.0f,  true, {} });
    m_gameLogic.RegisterEvent({ WorldEventId::NightBegin,   "밤 시작",   20.0f, true, {} });
    m_gameLogic.RegisterEvent({ WorldEventId::DungeonOpen,  "던전 개방", 18.0f, true, {} });
    m_gameLogic.RegisterEvent({ WorldEventId::DungeonClose, "던전 폐쇄", 23.0f, true, {} });

    m_gameLogic.RegisterZone(DEFAULT_ZONE_ID, m_zone.get());
    m_gameLogic.StartWithTick(GAMELOGIC_TICK_INTERVAL);

    // ── NetworkManager 콜백 설정 ─────────────────────────────────────────────
    m_net.SetCallbacks(
        // onAccept: 새 Session → SessionActor 생성 + WorldActor에 Post로 등록
        [this](std::shared_ptr<Session> session)
        {
            auto sa = std::make_shared<SessionActor>(session, m_world);
            {
                std::lock_guard lock(m_sessionActorsMutex);
                m_sessionActors[session->GetSessionId()] = sa;
            }
            // RegisterSession 직접 호출 대신 Post — WorldActor 전용 스레드에서 m_sessions 변경
            m_world.Post(MsgServer_RegisterSession{ session->GetSessionId(), sa });
            LOG_INFO("클라이언트 접속: sessionId={}", session->GetSessionId());
        },
        // onDisconnect: SessionActor 해제 + WorldActor에 로그아웃/등록해제 통보
        [this](uint64_t sessionId)
        {
            {
                std::lock_guard lock(m_sessionActorsMutex);
                m_sessionActors.erase(sessionId);
            }
            m_world.Post(MsgServer_UnregisterSession{ sessionId });
            m_world.Post(MsgSession_Logout{ sessionId });
            LOG_INFO("클라이언트 접속 해제: sessionId={}", sessionId);
        },
        // onPacket: 수신 패킷을 SessionActor 메일박스로 전달
        [this](uint64_t sessionId, uint16_t opcode, std::vector<uint8_t> payload)
        {
            std::shared_ptr<SessionActor> sa;
            {
                std::lock_guard lock(m_sessionActorsMutex);
                auto it = m_sessionActors.find(sessionId);
                if (it == m_sessionActors.end()) return;
                sa = it->second;
            }
            MsgNet_PacketReceived msg;
            msg.opcode  = opcode;
            msg.payload = std::move(payload);
            sa->Post(std::move(msg));
        }
    );

    // ── NetworkManager 초기화 ────────────────────────────────────────────────
    if (!m_net.Initialize())
    {
        LOG_ERROR("NetworkManager 초기화 실패");
        m_zone->Stop();
        m_world.Stop();
        ActorSystem::Instance().Stop();
        g_server = nullptr;
        return;
    }

    LOG_INFO("서버 시작 (TCP:{}, UDP:{})", NET_TCP_PORT, NET_UDP_PORT);

    // ── 메인 루프 ────────────────────────────────────────────────────────────
    // 실제 I/O 처리는 워커 스레드가 담당.
    // 메인 스레드는 종료 신호를 기다리며 대기.
    m_running.store(true);
    while (m_running.load())
        std::this_thread::sleep_for(std::chrono::milliseconds(100));

    // ── 종료 (초기화 역순) ────────────────────────────────────────────────────
    LOG_INFO("서버 종료 중...");
    m_net.Shutdown();
    m_gameLogic.Stop();
    m_zone->Stop();
    m_world.Stop();
    ActorSystem::Instance().Stop();
    Logger::Shutdown();

    g_server = nullptr;
}

void Server::Exit()
{
    m_running.store(false);
}