#include "Arbiter.h"
#include "../Shared/Logger.h"
#include "../Packets/PacketBase.h"
#include "../protocol_shared/ArbiterOpcodes.h"

#include <algorithm>
#include <cerrno>
#include <chrono>
#include <cstring>

#ifndef _WIN32
#include <poll.h>
#endif

// socklen_t 플랫폼 분기
#ifdef _WIN32
using SockLen = int;
#else
using SockLen = socklen_t;
#endif

Arbiter::Arbiter() = default;

Arbiter::~Arbiter()
{
    Stop();
}

// ─────────────────────────────────────────────────────────────────────────────
// Run — 비동기 시작
// ─────────────────────────────────────────────────────────────────────────────
void Arbiter::Run()
{
#ifdef _WIN32
    WSADATA wsa;
    WSAStartup(MAKEWORD(2, 2), &wsa);
#endif

    m_listenSocket = ::socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
    if (m_listenSocket == NX_INVALID_SOCKET)
    {
        LOG_ERROR("Arbiter: 소켓 생성 실패");
        return;
    }

    // 재시작 시 포트 즉시 재사용 (TIME_WAIT 상태 포트 재사용 허용)
    int reuse = 1;
    ::setsockopt(m_listenSocket, SOL_SOCKET, SO_REUSEADDR,
                 reinterpret_cast<const char*>(&reuse), sizeof(reuse));
#ifdef SO_REUSEPORT
    ::setsockopt(m_listenSocket, SOL_SOCKET, SO_REUSEPORT,
                 reinterpret_cast<const char*>(&reuse), sizeof(reuse));
#endif

    sockaddr_in addr{};
    addr.sin_family      = AF_INET;
    addr.sin_addr.s_addr = INADDR_ANY;
    addr.sin_port        = htons(ARBITER_PORT);

    if (::bind(m_listenSocket, reinterpret_cast<sockaddr*>(&addr), sizeof(addr)) != 0)
    {
        char errBuf[128]{};
#ifdef _WIN32
        strerror_s(errBuf, sizeof(errBuf), errno);
#else
        strerror_r(errno, errBuf, sizeof(errBuf));
#endif
        LOG_ERROR("Arbiter: bind 실패 (port={}) — {}", ARBITER_PORT, errBuf);
        closesocket(m_listenSocket);
        m_listenSocket = NX_INVALID_SOCKET;
        return;
    }

    if (::listen(m_listenSocket, 4) != 0)
    {
        LOG_ERROR("Arbiter: listen 실패");
        closesocket(m_listenSocket);
        m_listenSocket = NX_INVALID_SOCKET;
        return;
    }

    m_running.store(true);
    m_startTime    = std::chrono::steady_clock::now();
    m_acceptThread = std::thread([this] { AcceptLoop(); });

    LOG_INFO("Arbiter 시작 (port={})", ARBITER_PORT);
}

// ─────────────────────────────────────────────────────────────────────────────
// Stop — 종료
// ─────────────────────────────────────────────────────────────────────────────
void Arbiter::Stop()
{
    if (!m_running.exchange(false)) return;

    // m_running = false → AcceptLoop가 poll 타임아웃(200ms) 후 자연 탈출
    if (m_acceptThread.joinable())
        m_acceptThread.join();

    // 수락 스레드 종료 후 리슨 소켓 닫기
    if (m_listenSocket != NX_INVALID_SOCKET)
    {
        closesocket(m_listenSocket);
        m_listenSocket = NX_INVALID_SOCKET;
    }

    // 세션 목록을 로컬로 이동 후 소켓 닫기
    // — 잠금 해제 후 소멸자(recv 스레드 join)가 실행되므로 데드락 없음
    std::vector<std::shared_ptr<ArbiterSession>> sessions;
    {
        std::lock_guard lock(m_sessionsMutex);
        sessions = std::move(m_sessions);
    }

    for (auto& s : sessions)
        s->Close();

    // sessions 소멸 → 각 세션 소멸자에서 recv 스레드 join
    sessions.clear();

#ifdef _WIN32
    WSACleanup();
#endif

    LOG_INFO("Arbiter 종료");
}

// ─────────────────────────────────────────────────────────────────────────────
// 수락 루프
// ─────────────────────────────────────────────────────────────────────────────
void Arbiter::AcceptLoop()
{
    while (m_running.load())
    {
        // poll로 연결 대기 — 200ms 타임아웃마다 m_running을 재확인해 Stop() 시 즉시 탈출
#ifdef _WIN32
        WSAPOLLFD pfd{};
        pfd.fd      = m_listenSocket;
        pfd.events  = POLLIN;
        int ret = ::WSAPoll(&pfd, 1, 200);
#else
        struct pollfd pfd{};
        pfd.fd     = m_listenSocket;
        pfd.events = POLLIN;
        int ret = ::poll(&pfd, 1, 200);
#endif
        if (ret <= 0) continue;                   // 타임아웃 또는 오류 → 플래그 재확인
        if (!(pfd.revents & POLLIN)) continue;

        sockaddr_storage clientAddr{};
        SockLen          addrLen = static_cast<SockLen>(sizeof(clientAddr));

        NxSocket client = ::accept(m_listenSocket,
                                   reinterpret_cast<sockaddr*>(&clientAddr),
                                   &addrLen);
        if (client == NX_INVALID_SOCKET) break;  // 소켓 오류

        LOG_INFO("Arbiter: 런처 연결됨");

        auto session = std::make_shared<ArbiterSession>(client);
        {
            std::lock_guard lock(m_sessionsMutex);
            m_sessions.push_back(session);
        }

        session->Start(
            [this](ArbiterSession& s, uint16_t op, std::vector<uint8_t> payload)
            {
                OnPacket(s, op, std::move(payload));
            },
            [this](ArbiterSession& s)
            {
                OnClose(s);
            });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 패킷 수신 디스패치
// ─────────────────────────────────────────────────────────────────────────────
void Arbiter::OnPacket(ArbiterSession& session, uint16_t opcode, std::vector<uint8_t> payload)
{
    // 인증 전에는 LMSG_AUTH 만 허용
    if (!session.IsAuthenticated() && opcode != LMSG_AUTH)
    {
        LOG_WARN("Arbiter: 인증 전 패킷 수신 opcode={:#x} — 연결 종료", opcode);
        session.Close();
        return;
    }

    switch (static_cast<ArbiterOpcode>(opcode))
    {
        case LMSG_AUTH:         HandleAuth(session, payload);        break;
        case LMSG_GET_STATUS:   HandleGetStatus(session);            break;
        case LMSG_KICK_PLAYER:  HandleKickPlayer(session, payload);  break;
        case LMSG_GET_PLAYERS:  HandleGetPlayers(session);           break;
        default:
            LOG_WARN("Arbiter: 알 수 없는 opcode={:#x}", opcode);
            break;
    }
}

void Arbiter::OnClose(ArbiterSession& session)
{
    std::lock_guard lock(m_sessionsMutex);
    m_sessions.erase(
        std::remove_if(m_sessions.begin(), m_sessions.end(),
                       [&session](const std::shared_ptr<ArbiterSession>& s)
                       { return s.get() == &session; }),
        m_sessions.end());

    LOG_INFO("Arbiter: 런처 연결 해제");
}

// ─────────────────────────────────────────────────────────────────────────────
// 패킷 핸들러
// ─────────────────────────────────────────────────────────────────────────────

void Arbiter::HandleAuth(ArbiterSession& session, std::vector<uint8_t>& payload)
{
    // TODO: 설정 파일에서 secret 로드 후 비교 (현재는 항상 성공 처리)
    PacketReader r = PacketReader::FromPayload(
        LMSG_AUTH, payload.data(), static_cast<uint32_t>(payload.size()));
    [[maybe_unused]] std::string secret = r.ReadString();

    session.SetAuthenticated(true);
    LOG_INFO("Arbiter: 런처 인증 성공");

    PacketWriter w(AMSG_AUTH_RESULT);
    w.WriteUInt8(1);
    w.WriteString("OK");
    session.Send(w.Finalize());
}

void Arbiter::HandleGetStatus(ArbiterSession& session)
{
    const uint32_t playerCount = m_getPlayerCount ? m_getPlayerCount() : 0;

    const auto     now        = std::chrono::steady_clock::now();
    const uint32_t uptimeSecs = static_cast<uint32_t>(
        std::chrono::duration_cast<std::chrono::seconds>(now - m_startTime).count());

    PacketWriter w(AMSG_STATUS);
    w.WriteUInt32(playerCount);
    w.WriteUInt32(uptimeSecs);
    session.Send(w.Finalize());
}

void Arbiter::HandleKickPlayer(ArbiterSession& session, std::vector<uint8_t>& payload)
{
    PacketReader r = PacketReader::FromPayload(
        LMSG_KICK_PLAYER, payload.data(), static_cast<uint32_t>(payload.size()));
    const uint64_t    targetSessionId = r.ReadUInt64();
    const std::string reason          = r.ReadString();

    const bool success = m_kickPlayer ? m_kickPlayer(targetSessionId, reason) : false;
    LOG_INFO("Arbiter: 킥 요청 sessionId={} reason=\"{}\" success={}",
             targetSessionId, reason, success);

    PacketWriter w(AMSG_KICK_RESULT);
    w.WriteUInt8(success ? 1 : 0);
    w.WriteString(success ? "OK" : "킥 콜백이 등록되지 않았습니다");
    session.Send(w.Finalize());
}

// ─────────────────────────────────────────────────────────────────────────────
// 이벤트 발행
// ─────────────────────────────────────────────────────────────────────────────

void Arbiter::HandleGetPlayers(ArbiterSession& session)
{
    const auto players = m_getPlayers ? m_getPlayers() : std::vector<std::pair<uint64_t, std::string>>{};

    PacketWriter w(AMSG_PLAYERS);
    w.WriteUInt32(static_cast<uint32_t>(players.size()));
    for (const auto& [sessionId, name] : players)
    {
        w.WriteUInt64(sessionId);
        w.WriteString(name);
    }
    session.Send(w.Finalize());
}

void Arbiter::PublishServerReady()
{
    PacketWriter w(AMSG_EVENT_SERVER_READY);
    Broadcast(w.Finalize());
}

void Arbiter::PublishPlayerJoin(uint64_t sessionId, const std::string& name)
{
    LOG_DEBUG("Arbiter: PublishPlayerJoin sessionId={} name=\"{}\"", sessionId, name);
    PacketWriter w(AMSG_EVENT_PLAYER_JOIN);
    w.WriteUInt64(sessionId);
    w.WriteString(name);
    Broadcast(w.Finalize());
}

void Arbiter::PublishPlayerLeave(uint64_t sessionId)
{
    LOG_DEBUG("Arbiter: PublishPlayerLeave sessionId={}", sessionId);
    PacketWriter w(AMSG_EVENT_PLAYER_LEAVE);
    w.WriteUInt64(sessionId);
    Broadcast(w.Finalize());
}

void Arbiter::Broadcast(const std::vector<uint8_t>& packet)
{
    std::lock_guard lock(m_sessionsMutex);
    LOG_DEBUG("Arbiter: Broadcast — 세션 수={}", m_sessions.size());
    for (auto& s : m_sessions)
    {
        if (s->IsAuthenticated())
            s->Send(packet);
    }
}
