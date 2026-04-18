#pragma once

#include "../Platform/Platform.h"
#include "ArbiterSession.h"

#include <atomic>
#include <chrono>
#include <functional>
#include <memory>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

// 아비터 관리 채널 포트 (게임 포트 7070/7071과 분리)
inline constexpr uint16_t ARBITER_PORT = 7072;

// ─────────────────────────────────────────────────────────────────────────────
// Arbiter — 런처↔서버 관리 채널
//
// 게임 NetworkManager와 완전히 분리된 독립 TCP 서버.
// 기존 PacketBase / Platform.h 인프라를 재사용하며 서드파티 없이 동작한다.
//
// Run() 은 별도 스레드에서 accept 루프를 시작하고 즉시 반환한다 (비동기).
// Server::Run() 과 무관하게 독립 실행 가능 — 연동하지 않아도 영향 없음.
//
// 선택적 콜백 (Run() 전에 등록, 없으면 기본값 반환):
//   SetPlayerCountCallback — LMSG_GET_STATUS 응답 시 접속자 수 조회
//   SetKickCallback        — LMSG_KICK_PLAYER 처리 시 킥 실행
//
// 이벤트 발행 (외부에서 선택적으로 호출):
//   PublishServerReady / PublishPlayerJoin / PublishPlayerLeave
//   → 인증된 런처 전체에 푸시 패킷 전송
//   → main.cpp에서 Server 콜백과 연결하거나, 연결하지 않아도 무방
// ─────────────────────────────────────────────────────────────────────────────
class Arbiter
{
public:
    Arbiter();
    ~Arbiter();

    Arbiter(const Arbiter&)            = delete;
    Arbiter& operator=(const Arbiter&) = delete;

    // ── 생명주기 ─────────────────────────────────────────────────────────────

    // 비동기 시작 — accept 스레드 생성 후 즉시 반환
    void Run();

    // 종료 — accept 스레드 및 모든 세션 정리 후 반환
    void Stop();

    // ── 이벤트 발행 (스레드 안전) ────────────────────────────────────────────
    // 인증된 런처 전체에 이벤트 패킷을 푸시한다.
    // Server / WorldActor 수정 없이 main.cpp에서 선택적으로 호출 가능.
    void PublishServerReady();
    void PublishPlayerJoin(uint64_t sessionId, const std::string& name);
    void PublishPlayerLeave(uint64_t sessionId);

    // ── 선택적 콜백 등록 ─────────────────────────────────────────────────────
    void SetPlayerCountCallback(std::function<uint32_t()> fn)
    {
        m_getPlayerCount = std::move(fn);
    }
    void SetKickCallback(std::function<bool(uint64_t, const std::string&)> fn)
    {
        m_kickPlayer = std::move(fn);
    }

private:
    // ── 내부 루프 / 핸들러 ───────────────────────────────────────────────────
    void AcceptLoop();
    void OnPacket(ArbiterSession& session, uint16_t opcode, std::vector<uint8_t> payload);
    void OnClose (ArbiterSession& session);

    // 인증된 세션 전체에 패킷 브로드캐스트
    void Broadcast(const std::vector<uint8_t>& packet);

    // ── 패킷 핸들러 ──────────────────────────────────────────────────────────
    void HandleAuth      (ArbiterSession& session, std::vector<uint8_t>& payload);
    void HandleGetStatus (ArbiterSession& session);
    void HandleKickPlayer(ArbiterSession& session, std::vector<uint8_t>& payload);

    // ── 소켓 ─────────────────────────────────────────────────────────────────
    NxSocket          m_listenSocket{ NX_INVALID_SOCKET };
    std::thread       m_acceptThread;
    std::atomic<bool> m_running{ false };

    // ── 세션 관리 ────────────────────────────────────────────────────────────
    std::mutex                                   m_sessionsMutex;
    std::vector<std::shared_ptr<ArbiterSession>> m_sessions;

    // ── 서버 시작 시각 (uptime 계산) ─────────────────────────────────────────
    std::chrono::steady_clock::time_point m_startTime;

    // ── 선택적 콜백 ──────────────────────────────────────────────────────────
    std::function<uint32_t()>                         m_getPlayerCount;
    std::function<bool(uint64_t, const std::string&)> m_kickPlayer;
};
