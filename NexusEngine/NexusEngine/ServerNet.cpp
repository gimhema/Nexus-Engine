#include "ServerNet.h"

#include <cstring>
#include <algorithm>
#include <thread>

// ─────────────────────────────────────────────────────────────────────────────
// NetworkManager — 공통 구현
// ─────────────────────────────────────────────────────────────────────────────

NetworkManager::NetworkManager()  = default;
NetworkManager::~NetworkManager() { Shutdown(); }

void NetworkManager::SetCallbacks(OnAcceptFn     onAccept,
                                  OnDisconnectFn onDisconnect,
                                  OnPacketFn     onPacket)
{
    m_onAccept     = std::move(onAccept);
    m_onDisconnect = std::move(onDisconnect);
    m_onPacket     = std::move(onPacket);
}

void NetworkManager::RegisterHandler(uint16_t opcode, PacketHandlerFunc handler)
{
    m_handlerMap[opcode] = std::move(handler);
}

std::shared_ptr<Session> NetworkManager::FindSession(uint64_t sessionId) const
{
    std::lock_guard lock(m_sessionMutex);
    auto it = m_sessions.find(sessionId);
    return it != m_sessions.end() ? it->second : nullptr;
}

void NetworkManager::CloseSession(uint64_t sessionId)
{
    // find+erase를 단일 잠금 내에서 처리 — 두 워커 스레드가 동시에 진입해도 콜백 중복 호출 방지
    std::shared_ptr<Session> session;
    {
        std::lock_guard lock(m_sessionMutex);
        auto it = m_sessions.find(sessionId);
        if (it == m_sessions.end()) return;
        session = it->second;
        session->SetState(SessionState::Closing);
        m_sessions.erase(it);
    }

#ifndef _WIN32
    ::epoll_ctl(m_epollFd, EPOLL_CTL_DEL, session->GetSocket(), nullptr);
#endif

    if (m_onDisconnect)
        m_onDisconnect(sessionId);
}

void NetworkManager::DispatchPacket(std::shared_ptr<Session> session, PacketReader& reader)
{
    // Actor 콜백이 설정된 경우 SessionActor로 라우팅
    if (m_onPacket)
    {
        m_onPacket(session->GetSessionId(), reader.GetOpcode(), reader.GetPayload());
        return;
    }
    // 레거시 fallback: opcode 핸들러 맵
    auto it = m_handlerMap.find(reader.GetOpcode());
    if (it != m_handlerMap.end())
        it->second(session, reader);
}

std::shared_ptr<Session> NetworkManager::CreateSession(NxSocket socket)
{
    uint64_t id = m_nextSessionId.fetch_add(1);

#ifdef _WIN32
    auto session = std::make_shared<Session>(socket, id, m_iocpHandle);
#else
    auto session = std::make_shared<Session>(socket, id, m_epollFd);
#endif

    std::lock_guard lock(m_sessionMutex);
    m_sessions[id] = session;
    return session;
}

void NetworkManager::RemoveSession(uint64_t sessionId)
{
    // CloseSession과 동일하게 단일 잠금 내에서 find+erase 처리
    std::shared_ptr<Session> session;
    {
        std::lock_guard lock(m_sessionMutex);
        auto it = m_sessions.find(sessionId);
        if (it == m_sessions.end()) return;
        session = it->second;
        m_sessions.erase(it);
    }
#ifndef _WIN32
    ::epoll_ctl(m_epollFd, EPOLL_CTL_DEL, session->GetSocket(), nullptr);
#endif
}

bool NetworkManager::StartWorkerThreads(uint32_t count)
{
    if (count == 0)
        count = std::thread::hardware_concurrency() * 2;
    if (count == 0)
        count = 4;

    m_running.store(true);
    m_workerThreads.reserve(count);
    for (uint32_t i = 0; i < count; ++i)
        m_workerThreads.emplace_back([this] { WorkerLoop(); });
    return true;
}

// OnTCPSend: 양 플랫폼 동일 — 다음 패킷 드레인
void NetworkManager::OnTCPSend(OverlappedEx* /*pOv*/, NxDword /*bytes*/, Session* session)
{
    session->FlushSendQueue();
}

// ─────────────────────────────────────────────────────────────────────────────
// ██  Windows IOCP 구현
// ─────────────────────────────────────────────────────────────────────────────
#ifdef _WIN32

bool NetworkManager::InitWinsock()
{
    WSADATA wsa{};
    return ::WSAStartup(MAKEWORD(2, 2), &wsa) == 0;
}

bool NetworkManager::CreateIOCP()
{
    m_iocpHandle = ::CreateIoCompletionPort(INVALID_HANDLE_VALUE, nullptr, 0, 0);
    return m_iocpHandle != NX_INVALID_HANDLE;
}

bool NetworkManager::SetupTCPListener(uint16_t port)
{
    m_tcpListenSocket = ::WSASocket(AF_INET, SOCK_STREAM, IPPROTO_TCP,
                                    nullptr, 0, WSA_FLAG_OVERLAPPED);
    if (m_tcpListenSocket == NX_INVALID_SOCKET) return false;

    int opt = 1;
    ::setsockopt(m_tcpListenSocket, SOL_SOCKET, SO_REUSEADDR,
                 reinterpret_cast<const char*>(&opt), sizeof(opt));

    sockaddr_in addr{};
    addr.sin_family      = AF_INET;
    addr.sin_addr.s_addr = INADDR_ANY;
    addr.sin_port        = htons(port);
    if (::bind(m_tcpListenSocket, reinterpret_cast<sockaddr*>(&addr), sizeof(addr)) != 0)
        return false;
    if (::listen(m_tcpListenSocket, static_cast<int>(NET_ACCEPT_BACKLOG)) != 0)
        return false;

    // listen 소켓을 IOCP 에 등록 (completionKey = 0)
    if (!::CreateIoCompletionPort(reinterpret_cast<HANDLE>(m_tcpListenSocket),
                                  m_iocpHandle, 0, 0))
        return false;

    // AcceptEx / GetAcceptExSockaddrs 런타임 로드
    DWORD bytes = 0;
    GUID guidAex = WSAID_ACCEPTEX;
    ::WSAIoctl(m_tcpListenSocket, SIO_GET_EXTENSION_FUNCTION_POINTER,
               &guidAex, sizeof(guidAex),
               &m_fnAcceptEx, sizeof(m_fnAcceptEx), &bytes, nullptr, nullptr);

    GUID guidGaes = WSAID_GETACCEPTEXSOCKADDRS;
    ::WSAIoctl(m_tcpListenSocket, SIO_GET_EXTENSION_FUNCTION_POINTER,
               &guidGaes, sizeof(guidGaes),
               &m_fnGetAcceptExSockaddrs, sizeof(m_fnGetAcceptExSockaddrs),
               &bytes, nullptr, nullptr);

    PostAccept();
    return true;
}

bool NetworkManager::SetupUDPSocket(uint16_t port)
{
    m_udpSocket = ::WSASocket(AF_INET, SOCK_DGRAM, IPPROTO_UDP,
                              nullptr, 0, WSA_FLAG_OVERLAPPED);
    if (m_udpSocket == NX_INVALID_SOCKET) return false;

    sockaddr_in addr{};
    addr.sin_family      = AF_INET;
    addr.sin_addr.s_addr = INADDR_ANY;
    addr.sin_port        = htons(port);
    if (::bind(m_udpSocket, reinterpret_cast<sockaddr*>(&addr), sizeof(addr)) != 0)
        return false;

    if (!::CreateIoCompletionPort(reinterpret_cast<HANDLE>(m_udpSocket),
                                  m_iocpHandle, 0, 0))
        return false;

    PostUDPRecv();
    return true;
}

bool NetworkManager::Initialize(uint16_t tcpPort, uint16_t udpPort, uint32_t workerCount)
{
    if (!InitWinsock())                   return false;
    if (!CreateIOCP())                    return false;
    if (!SetupTCPListener(tcpPort))       return false;
    if (!SetupUDPSocket(udpPort))         return false;
    if (!StartWorkerThreads(workerCount)) return false;
    return true;
}

void NetworkManager::Shutdown()
{
    if (!m_running.exchange(false)) return;

    // 워커 스레드마다 종료 신호 전달 (overlapped=nullptr → WorkerLoop break)
    for (size_t i = 0; i < m_workerThreads.size(); ++i)
        ::PostQueuedCompletionStatus(m_iocpHandle, 0, 0, nullptr);

    for (auto& t : m_workerThreads)
        if (t.joinable()) t.join();
    m_workerThreads.clear();

    { std::lock_guard lock(m_sessionMutex); m_sessions.clear(); }

    if (m_tcpListenSocket != NX_INVALID_SOCKET)
    { ::closesocket(m_tcpListenSocket); m_tcpListenSocket = NX_INVALID_SOCKET; }
    if (m_udpSocket != NX_INVALID_SOCKET)
    { ::closesocket(m_udpSocket); m_udpSocket = NX_INVALID_SOCKET; }
    if (m_iocpHandle != NX_INVALID_HANDLE)
    { ::CloseHandle(m_iocpHandle); m_iocpHandle = NX_INVALID_HANDLE; }

    ::WSACleanup();
}

void NetworkManager::PostAccept()
{
    auto* pOv = new OverlappedEx{};
    pOv->operation    = IOOperation::TCP_ACCEPT;
    pOv->acceptSocket = ::WSASocket(AF_INET, SOCK_STREAM, IPPROTO_TCP,
                                    nullptr, 0, WSA_FLAG_OVERLAPPED);
    if (pOv->acceptSocket == NX_INVALID_SOCKET) { delete pOv; return; }

    DWORD bytes = 0;
    BOOL ok = m_fnAcceptEx(
        m_tcpListenSocket, pOv->acceptSocket,
        pOv->buffer, 0,
        sizeof(SOCKADDR_IN) + 16, sizeof(SOCKADDR_IN) + 16,
        &bytes, &pOv->overlapped);

    if (!ok && ::WSAGetLastError() != WSA_IO_PENDING)
    {
        ::closesocket(pOv->acceptSocket);
        delete pOv;
    }
}

void NetworkManager::PostUDPRecv()
{
    m_udpRecvOv.operation     = IOOperation::UDP_RECV;
    m_udpRecvOv.wsaBuf.buf    = reinterpret_cast<CHAR*>(m_udpRecvOv.buffer);
    m_udpRecvOv.wsaBuf.len    = NET_UDP_MAX_PACKET;
    m_udpRecvOv.remoteAddrLen = sizeof(sockaddr_storage);

    DWORD flags = 0, bytes = 0;
    ::WSARecvFrom(m_udpSocket,
                  &m_udpRecvOv.wsaBuf, 1,
                  &bytes, &flags,
                  reinterpret_cast<SOCKADDR*>(&m_udpRecvOv.remoteAddr),
                  &m_udpRecvOv.remoteAddrLen,
                  &m_udpRecvOv.overlapped, nullptr);
}

void NetworkManager::WorkerLoop()
{
    while (true)
    {
        DWORD       bytes    = 0;
        ULONG_PTR   key      = 0;
        OVERLAPPED* pOverlap = nullptr;

        ::GetQueuedCompletionStatus(m_iocpHandle, &bytes, &key, &pOverlap, INFINITE);

        // Shutdown 이 보낸 종료 신호
        if (pOverlap == nullptr) break;

        auto* pOv = reinterpret_cast<OverlappedEx*>(pOverlap);

        // I/O 오류 처리 — bytes=0은 연결 종료 신호
        if (bytes == 0 && pOv->operation != IOOperation::TCP_ACCEPT)
        {
            if (pOv->operation == IOOperation::TCP_RECV ||
                pOv->operation == IOOperation::TCP_SEND)
            {
                auto* raw = reinterpret_cast<Session*>(key);
                if (raw) CloseSession(raw->GetSessionId());
            }
            continue;
        }

        switch (pOv->operation)
        {
        case IOOperation::TCP_ACCEPT:
            OnTCPAccept(pOv, bytes);
            break;
        case IOOperation::TCP_RECV:
            OnTCPRecv(pOv, bytes, reinterpret_cast<Session*>(key));
            break;
        case IOOperation::TCP_SEND:
            OnTCPSend(pOv, bytes, reinterpret_cast<Session*>(key));
            break;
        case IOOperation::UDP_RECV:
            OnUDPRecv(pOv, bytes);
            break;
        case IOOperation::UDP_SEND:
            OnUDPSend(pOv, bytes);
            break;
        }
    }
}

void NetworkManager::OnTCPAccept(OverlappedEx* pOv, NxDword /*bytes*/)
{
    NxSocket accepted  = pOv->acceptSocket;
    pOv->acceptSocket  = NX_INVALID_SOCKET;
    delete pOv;

    ::setsockopt(accepted, SOL_SOCKET, SO_UPDATE_ACCEPT_CONTEXT,
                 reinterpret_cast<const char*>(&m_tcpListenSocket),
                 sizeof(m_tcpListenSocket));

    auto session = CreateSession(accepted);
    if (!session)
    {
        ::closesocket(accepted);
    }
    else
    {
        ::CreateIoCompletionPort(reinterpret_cast<HANDLE>(accepted),
                                 m_iocpHandle,
                                 reinterpret_cast<ULONG_PTR>(session.get()),
                                 0);
        session->PostRecv();
        if (m_onAccept)
            m_onAccept(session);
    }

    PostAccept();
}

void NetworkManager::OnTCPRecv(OverlappedEx* /*pOv*/, NxDword bytes, Session* session)
{
    auto& buf = session->GetRecvBuffer();
    buf.OnWritten(bytes);

    while (buf.Readable() >= PACKET_HEADER_SIZE)
    {
        auto* hdr = reinterpret_cast<const PacketHeader*>(buf.ReadPtr());
        if (hdr->size < PACKET_HEADER_SIZE || buf.Readable() < hdr->size) break;

        PacketReader reader(buf.ReadPtr(), hdr->size);
        DispatchPacket(session->shared_from_this(), reader);
        buf.Consume(hdr->size);
    }

    if (session->IsConnected())
        session->PostRecv();
}

void NetworkManager::OnUDPRecv(OverlappedEx* pOv, NxDword bytes)
{
    if (bytes >= UDP_HEADER_SIZE)
    {
        PacketReader reader = PacketReader::FromUDP(pOv->buffer, bytes);
        auto session = FindSession(reader.GetSessionToken());
        if (session)
            DispatchPacket(session, reader);
    }
    PostUDPRecv();
}

void NetworkManager::OnUDPSend(OverlappedEx* pOv, NxDword /*bytes*/)
{
    delete pOv;
}

void NetworkManager::SendUDP(const sockaddr_storage& dest,
                             const uint8_t* data, uint16_t length)
{
    auto* pOv = new OverlappedEx{};
    pOv->operation  = IOOperation::UDP_SEND;
    uint16_t copyLen = std::min<uint16_t>(length, NET_UDP_MAX_PACKET);
    std::memcpy(pOv->buffer, data, copyLen);
    pOv->wsaBuf.buf = reinterpret_cast<CHAR*>(pOv->buffer);
    pOv->wsaBuf.len = copyLen;

    DWORD sent = 0;
    int result = ::WSASendTo(m_udpSocket, &pOv->wsaBuf, 1, &sent, 0,
                             reinterpret_cast<const SOCKADDR*>(&dest),
                             static_cast<int>(sizeof(dest)),
                             &pOv->overlapped, nullptr);
    if (result == SOCKET_ERROR && ::WSAGetLastError() != WSA_IO_PENDING)
        delete pOv;
}

#else
// ─────────────────────────────────────────────────────────────────────────────
// ██  Linux epoll 구현
// ─────────────────────────────────────────────────────────────────────────────

bool NetworkManager::CreateEpoll()
{
    m_epollFd = ::epoll_create1(EPOLL_CLOEXEC);
    return m_epollFd != NX_INVALID_HANDLE;
}

bool NetworkManager::SetupTCPListener(uint16_t port)
{
    m_tcpListenSocket = ::socket(AF_INET, SOCK_STREAM | SOCK_NONBLOCK, IPPROTO_TCP);
    if (m_tcpListenSocket == NX_INVALID_SOCKET) return false;

    int opt = 1;
    ::setsockopt(m_tcpListenSocket, SOL_SOCKET, SO_REUSEADDR,
                 reinterpret_cast<const char*>(&opt), sizeof(opt));
#ifdef SO_REUSEPORT
    ::setsockopt(m_tcpListenSocket, SOL_SOCKET, SO_REUSEPORT,
                 reinterpret_cast<const char*>(&opt), sizeof(opt));
#endif

    sockaddr_in addr{};
    addr.sin_family      = AF_INET;
    addr.sin_addr.s_addr = INADDR_ANY;
    addr.sin_port        = htons(port);
    if (::bind(m_tcpListenSocket, reinterpret_cast<sockaddr*>(&addr), sizeof(addr)) != 0)
        return false;
    if (::listen(m_tcpListenSocket, static_cast<int>(NET_ACCEPT_BACKLOG)) != 0)
        return false;

    RegisterAcceptSocket();
    return true;
}

bool NetworkManager::SetupUDPSocket(uint16_t port)
{
    m_udpSocket = ::socket(AF_INET, SOCK_DGRAM | SOCK_NONBLOCK, IPPROTO_UDP);
    if (m_udpSocket == NX_INVALID_SOCKET) return false;

    sockaddr_in addr{};
    addr.sin_family      = AF_INET;
    addr.sin_addr.s_addr = INADDR_ANY;
    addr.sin_port        = htons(port);
    if (::bind(m_udpSocket, reinterpret_cast<sockaddr*>(&addr), sizeof(addr)) != 0)
        return false;

    RegisterUDPSocket();
    return true;
}

bool NetworkManager::Initialize(uint16_t tcpPort, uint16_t udpPort, uint32_t workerCount)
{
    if (!CreateEpoll())                   return false;
    if (!SetupTCPListener(tcpPort))       return false;
    if (!SetupUDPSocket(udpPort))         return false;
    if (!StartWorkerThreads(workerCount)) return false;
    return true;
}

void NetworkManager::Shutdown()
{
    if (!m_running.exchange(false)) return;

    // listen 소켓을 먼저 닫아 신규 accept를 차단 (워커가 OnTCPAccept에서 EBADF로 즉시 탈출)
    if (m_tcpListenSocket != NX_INVALID_SOCKET)
    { closesocket(m_tcpListenSocket); m_tcpListenSocket = NX_INVALID_SOCKET; }
    if (m_udpSocket != NX_INVALID_SOCKET)
    { closesocket(m_udpSocket); m_udpSocket = NX_INVALID_SOCKET; }

    // 워커 스레드 종료 대기 (epoll_wait 100ms 타임아웃 내 자연 탈출)
    for (auto& t : m_workerThreads)
        if (t.joinable()) t.join();
    m_workerThreads.clear();

    // 워커 종료 후 남은 클라이언트 소켓을 강제 종료 — TIME_WAIT 회피 + 포트 즉시 해제
    {
        std::lock_guard lock(m_sessionMutex);
        for (auto& [id, session] : m_sessions)
        {
            ::epoll_ctl(m_epollFd, EPOLL_CTL_DEL, session->GetSocket(), nullptr);
            session->ForceClose();
        }
        m_sessions.clear();
    }

    if (m_epollFd != NX_INVALID_HANDLE)
    { ::close(m_epollFd); m_epollFd = NX_INVALID_HANDLE; }
}

void NetworkManager::RegisterAcceptSocket()
{
    m_tcpAcceptOv.operation = IOOperation::TCP_ACCEPT;

    epoll_event ev{};
    ev.events   = EPOLLIN;
    ev.data.ptr = &m_tcpAcceptOv;
    ::epoll_ctl(m_epollFd, EPOLL_CTL_ADD, m_tcpListenSocket, &ev);
}

void NetworkManager::RegisterUDPSocket()
{
    m_udpRecvOv.operation = IOOperation::UDP_RECV;

    epoll_event ev{};
    ev.events   = EPOLLIN;
    ev.data.ptr = &m_udpRecvOv;
    ::epoll_ctl(m_epollFd, EPOLL_CTL_ADD, m_udpSocket, &ev);
}

void NetworkManager::WorkerLoop()
{
    constexpr int MAX_EVENTS = 64;
    epoll_event events[MAX_EVENTS];

    while (m_running.load())
    {
        int count = ::epoll_wait(m_epollFd, events, MAX_EVENTS, 100);
        if (count < 0)
        {
            if (errno == EINTR) continue;
            break;
        }

        for (int i = 0; i < count; ++i)
        {
            auto* pOv    = static_cast<OverlappedEx*>(events[i].data.ptr);
            const auto ev = events[i].events;
            if (!pOv) continue;

            if (ev & (EPOLLERR | EPOLLHUP))
            {
                if (pOv->operation == IOOperation::TCP_RECV)
                    CloseSession(pOv->sessionId);
                continue;
            }

            if (ev & EPOLLIN)
            {
                switch (pOv->operation)
                {
                case IOOperation::TCP_ACCEPT:
                    OnTCPAccept(pOv, 0);
                    break;
                case IOOperation::TCP_RECV:
                {
                    auto session = FindSession(pOv->sessionId);
                    if (session && session->IsConnected())
                        OnTCPRecv(pOv, 0, session.get());
                    break;
                }
                case IOOperation::UDP_RECV:
                    OnUDPRecv(pOv, 0);
                    break;
                default: break;
                }
            }

            if (ev & EPOLLOUT)
            {
                // 소켓 쓰기 가능 → 대기 중인 전송 재개
                if (pOv->operation == IOOperation::TCP_RECV)
                {
                    auto session = FindSession(pOv->sessionId);
                    if (session && session->IsConnected())
                        OnTCPSend(pOv, 0, session.get());
                }
            }
        }
    }
}

void NetworkManager::OnTCPAccept(OverlappedEx* /*pOv*/, NxDword /*bytes*/)
{
    while (true)
    {
        sockaddr_storage addr{};
        socklen_t addrLen = sizeof(addr);
        NxSocket accepted = ::accept4(m_tcpListenSocket,
                                      reinterpret_cast<sockaddr*>(&addr),
                                      &addrLen, SOCK_NONBLOCK);
        if (accepted == NX_INVALID_SOCKET)
        {
            if (errno == EAGAIN || errno == EWOULDBLOCK) break;
            break;
        }

        auto session = CreateSession(accepted);
        if (!session)
        {
            closesocket(accepted);
        }
        else
        {
            session->PostRecv();
            if (m_onAccept)
                m_onAccept(session);
        }
    }
}

void NetworkManager::OnTCPRecv(OverlappedEx* /*pOv*/, NxDword /*bytes*/, Session* session)
{
    auto& buf = session->GetRecvBuffer();

    while (true)
    {
        buf.Compact();
        if (buf.FreeSpace() == 0) break;

        ssize_t n = ::recv(session->GetSocket(),
                           buf.WritePtr(), buf.FreeSpace(), 0);
        if (n > 0)
        {
            buf.OnWritten(static_cast<uint32_t>(n));
        }
        else if (n == 0)
        {
            CloseSession(session->GetSessionId());
            return;
        }
        else
        {
            if (errno == EAGAIN || errno == EWOULDBLOCK) break;
            CloseSession(session->GetSessionId());
            return;
        }
    }

    while (buf.Readable() >= PACKET_HEADER_SIZE)
    {
        auto* hdr = reinterpret_cast<const PacketHeader*>(buf.ReadPtr());
        if (hdr->size < PACKET_HEADER_SIZE || buf.Readable() < hdr->size) break;

        PacketReader reader(buf.ReadPtr(), hdr->size);
        DispatchPacket(session->shared_from_this(), reader);
        buf.Consume(hdr->size);
    }
}

void NetworkManager::OnUDPRecv(OverlappedEx* pOv, NxDword /*bytes*/)
{
    while (true)
    {
        sockaddr_storage addr{};
        socklen_t addrLen = sizeof(addr);
        ssize_t n = ::recvfrom(m_udpSocket,
                               pOv->buffer, NET_UDP_MAX_PACKET, 0,
                               reinterpret_cast<sockaddr*>(&addr), &addrLen);
        if (n <= 0)
        {
            if (n < 0 && (errno == EAGAIN || errno == EWOULDBLOCK)) break;
            break;
        }

        if (static_cast<uint32_t>(n) >= UDP_HEADER_SIZE)
        {
            PacketReader reader = PacketReader::FromUDP(
                pOv->buffer, static_cast<uint32_t>(n));
            auto session = FindSession(reader.GetSessionToken());
            if (session)
                DispatchPacket(session, reader);
        }
    }
}

void NetworkManager::OnUDPSend(OverlappedEx* /*pOv*/, NxDword /*bytes*/)
{
    // Linux: sendto() 동기 처리이므로 완료 이벤트 없음
}

void NetworkManager::SendUDP(const sockaddr_storage& dest,
                             const uint8_t* data, uint16_t length)
{
    ::sendto(m_udpSocket, data,
             std::min<size_t>(length, NET_UDP_MAX_PACKET), 0,
             reinterpret_cast<const sockaddr*>(&dest), sizeof(dest));
}

#endif
