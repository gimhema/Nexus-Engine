#include "Server.h"
#include "Shared/Logger.h"

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

    // ── NetworkManager 초기화 ────────────────────────────────────────────────
    if (!m_net.Initialize())
    {
        LOG_ERROR("NetworkManager 초기화 실패");
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

    // ── 종료 ─────────────────────────────────────────────────────────────────
    LOG_INFO("서버 종료 중...");
    m_net.Shutdown();
    Logger::Shutdown();

    g_server = nullptr;
}

void Server::Exit()
{
    m_running.store(false);
}