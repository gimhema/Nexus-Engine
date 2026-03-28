#pragma once

#include "Platform/Platform.h"

#include <cstdint>
#include <functional>
#include <unordered_map>
#include <memory>
#include <vector>
#include <thread>
#include <atomic>
#include <mutex>

#include "Connection.h"
#include "Packets/PacketBase.h"

// ─────────────────────────────────────────────────────────────────────────────
// 기본 상수
// ─────────────────────────────────────────────────────────────────────────────
inline constexpr uint16_t NET_TCP_PORT          = 7070;
inline constexpr uint16_t NET_UDP_PORT          = 7071;
inline constexpr uint32_t NET_WORKER_THREADS    = 0;     // 0 = auto (CPU 코어 수 × 2)
inline constexpr uint32_t NET_ACCEPT_BACKLOG    = 64;
inline constexpr uint32_t NET_RECV_BUFFER_SIZE  = 8192;
inline constexpr uint32_t NET_UDP_MAX_PACKET    = 1400;  // MTU 이하

// ─────────────────────────────────────────────────────────────────────────────
// I/O 작업 종류
// ─────────────────────────────────────────────────────────────────────────────
enum class IOOperation : uint8_t
{
    TCP_ACCEPT,
    TCP_RECV,
    TCP_SEND,
    UDP_RECV,
    UDP_SEND,
};

// ─────────────────────────────────────────────────────────────────────────────
// OverlappedEx / IoContext
//
// 비동기 I/O 완료 이벤트에 연결되는 컨텍스트 구조체.
//
// [Windows - IOCP]
//   GetQueuedCompletionStatus 가 OVERLAPPED* 를 반환.
//   OVERLAPPED 가 첫 번째 필드여야 reinterpret_cast 가 안전.
//
//   복원 패턴:
//     OVERLAPPED* pRaw = ...;
//     OverlappedEx* pOv = reinterpret_cast<OverlappedEx*>(pRaw);
//     Session* session  = reinterpret_cast<Session*>(completionKey);
//
// [Linux - epoll]
//   epoll_event.data.ptr 에 OverlappedEx* 를 저장.
//   epoll_wait 반환 후 data.ptr 을 캐스팅해 복원.
// ─────────────────────────────────────────────────────────────────────────────
struct OverlappedEx
{
#ifdef _WIN32
    OVERLAPPED          overlapped{};           // ← 반드시 첫 번째 필드 (IOCP 패턴)
    WSABUF              wsaBuf{};
    DWORD               flags{ 0 };
#endif

    IOOperation         operation{};

    // UDP 수신 시 원격 주소 (TCP 에서는 미사용)
    sockaddr_storage    remoteAddr{};
#ifdef _WIN32
    INT                 remoteAddrLen{ sizeof(sockaddr_storage) };
#else
    socklen_t           remoteAddrLen{ sizeof(sockaddr_storage) };
#endif

    // AcceptEx / UDP RecvFrom 용 인라인 버퍼
    // TCP 데이터 수신은 Session::RecvBuffer 를 사용
    uint8_t             buffer[NET_UDP_MAX_PACKET]{};

    // accept 완료 전까지 accepted socket 보관
    NxSocket            acceptSocket{ NX_INVALID_SOCKET };

#ifndef _WIN32
    // Linux epoll: epoll_event.data.ptr = OverlappedEx* 로 저장하므로
    // Session 역참조를 위해 sessionId 보관 (listen·UDP 소켓은 0)
    uint64_t            sessionId{ 0 };
#endif
};

// ─────────────────────────────────────────────────────────────────────────────
// PacketHandlerFunc
//
// 등록 예시:
//   net.RegisterHandler(CMSG_PLAYER_MOVE,
//       [this](std::shared_ptr<Session> s, PacketReader& r) {
//           OnPlayerMove(s, r);
//       });
// ─────────────────────────────────────────────────────────────────────────────
using PacketHandlerFunc = std::function<void(std::shared_ptr<Session>, PacketReader&)>;

// ─────────────────────────────────────────────────────────────────────────────
// NetworkManager
//
// 비동기 TCP + UDP 네트워크 레이어.
//   Windows : IOCP (I/O Completion Ports)
//   Linux   : epoll
//
// TCP  : 클라이언트 연결 유지, 로그인/상태/신뢰성 필요 메시지
// UDP  : 위치 동기화, 이동, 스킬 이펙트 등 빈도 높고 손실 허용 가능한 메시지
//
// 스레드 모델:
//   워커 스레드 N개가 완료 이벤트 루프 실행.
//   핸들러 콜백은 워커 스레드에서 호출됨 → 핸들러 내 공유 데이터 보호 필요.
//
// UDP 세션 식별:
//   UDP 패킷 헤더에 sessionToken(uint64) 포함.
//   서버는 토큰으로 Session 을 찾아 핸들러 디스패치.
// ─────────────────────────────────────────────────────────────────────────────
class NetworkManager
{
public:
    NetworkManager();
    ~NetworkManager();

    NetworkManager(const NetworkManager&)            = delete;
    NetworkManager& operator=(const NetworkManager&) = delete;

    // ── 라이프사이클 ──────────────────────────────────────────────────────────
    bool Initialize(uint16_t tcpPort     = NET_TCP_PORT,
                    uint16_t udpPort     = NET_UDP_PORT,
                    uint32_t workerCount = NET_WORKER_THREADS);
    void Shutdown();

    // ── 핸들러 등록 ───────────────────────────────────────────────────────────
    // Initialize() 호출 전 또는 메인 스레드에서만 등록할 것.
    void RegisterHandler(uint16_t opcode, PacketHandlerFunc handler);

    // ── UDP 전송 (fire-and-forget) ────────────────────────────────────────────
    void SendUDP(const sockaddr_storage& dest, const uint8_t* data, uint16_t length);

    // ── 세션 조회 ─────────────────────────────────────────────────────────────
    [[nodiscard]] std::shared_ptr<Session> FindSession(uint64_t sessionId) const;
    void CloseSession(uint64_t sessionId);

private:
    // ── 플랫폼별 I/O 핸들 ────────────────────────────────────────────────────
#ifdef _WIN32
    NxHandle                   m_iocpHandle{ NX_INVALID_HANDLE };
#else
    NxHandle                   m_epollFd{ NX_INVALID_HANDLE };
#endif

    // ── TCP ───────────────────────────────────────────────────────────────────
    NxSocket                   m_tcpListenSocket{ NX_INVALID_SOCKET };

#ifdef _WIN32
    LPFN_ACCEPTEX              m_fnAcceptEx{ nullptr };             // 런타임 로드 (WSAIoctl)
    LPFN_GETACCEPTEXSOCKADDRS  m_fnGetAcceptExSockaddrs{ nullptr }; // 런타임 로드
#endif

    // ── UDP ───────────────────────────────────────────────────────────────────
    NxSocket     m_udpSocket{ NX_INVALID_SOCKET };
    OverlappedEx m_udpRecvOv;   // 수신 컨텍스트 상시 유지

#ifndef _WIN32
    OverlappedEx m_tcpAcceptOv; // Linux epoll: listen 소켓 이벤트 컨텍스트
#endif

    // ── 워커 스레드 풀 ────────────────────────────────────────────────────────
    std::vector<std::thread> m_workerThreads;
    std::atomic<bool>        m_running{ false };

    // ── 패킷 핸들러 맵 ────────────────────────────────────────────────────────
    std::unordered_map<uint16_t, PacketHandlerFunc> m_handlerMap;

    // ── 세션 테이블 ───────────────────────────────────────────────────────────
    mutable std::mutex                                         m_sessionMutex;
    std::unordered_map<uint64_t, std::shared_ptr<Session>>    m_sessions;
    std::atomic<uint64_t>                                      m_nextSessionId{ 1 };

    // ── 내부 초기화 ───────────────────────────────────────────────────────────
#ifdef _WIN32
    bool InitWinsock();
    bool CreateIOCP();
#else
    bool CreateEpoll();
#endif
    bool SetupTCPListener(uint16_t port);
    bool SetupUDPSocket(uint16_t port);
    bool StartWorkerThreads(uint32_t count);

    // ── 비동기 I/O 예약 ───────────────────────────────────────────────────────
#ifdef _WIN32
    void PostAccept();   // AcceptEx 로 비동기 accept 1건 예약
    void PostUDPRecv();  // WSARecvFrom 으로 UDP 수신 상시 예약
#else
    void RegisterAcceptSocket();   // epoll 에 listen 소켓 등록
    void RegisterUDPSocket();      // epoll 에 UDP 소켓 등록
#endif

    // ── I/O 이벤트 루프 ───────────────────────────────────────────────────────
    // 각 워커 스레드가 이 루프를 실행.
    void WorkerLoop();

    // ── 완료 이벤트 핸들러 ────────────────────────────────────────────────────
    void OnTCPAccept(OverlappedEx* pOv, NxDword bytes);
    void OnTCPRecv  (OverlappedEx* pOv, NxDword bytes, Session* session);
    void OnTCPSend  (OverlappedEx* pOv, NxDword bytes, Session* session);
    void OnUDPRecv  (OverlappedEx* pOv, NxDword bytes);
    void OnUDPSend  (OverlappedEx* pOv, NxDword bytes);

    // ── 패킷 디스패치 ─────────────────────────────────────────────────────────
    void DispatchPacket(std::shared_ptr<Session> session, PacketReader& reader);

    // ── 세션 생명주기 ─────────────────────────────────────────────────────────
    std::shared_ptr<Session> CreateSession(NxSocket socket);
    void                     RemoveSession(uint64_t sessionId);
};
