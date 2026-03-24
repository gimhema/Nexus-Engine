#pragma once

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <winsock2.h>
#include <mswsock.h>   // AcceptEx, GetAcceptExSockaddrs
#include <windows.h>
#include <ws2tcpip.h>

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
// OverlappedEx
//
// Windows OVERLAPPED 확장 구조체.
// IOCP는 OVERLAPPED* 를 반환하므로 이 구조체의 첫 번째 필드여야 함.
//
// 워커 스레드 복원 패턴:
//   OVERLAPPED* pRaw = ...;                                   // GetQueuedCompletionStatus 반환값
//   OverlappedEx* pOv = reinterpret_cast<OverlappedEx*>(pRaw); // ← 안전: 첫 필드 보장
//   Session* session  = reinterpret_cast<Session*>(completionKey);
// ─────────────────────────────────────────────────────────────────────────────
struct OverlappedEx
{
    OVERLAPPED          overlapped{};           // ← 반드시 첫 번째 필드
    IOOperation         operation{};
    WSABUF              wsaBuf{};
    DWORD               flags{ 0 };

    // UDP 수신 시 원격 주소 (TCP 에서는 미사용)
    SOCKADDR_STORAGE    remoteAddr{};
    INT                 remoteAddrLen{ sizeof(SOCKADDR_STORAGE) };

    // AcceptEx / UDP RecvFrom 용 인라인 버퍼
    // TCP 데이터 수신은 Session::RecvBuffer 를 사용
    uint8_t             buffer[NET_UDP_MAX_PACKET]{};

    // AcceptEx: accept 완료 전까지 accepted socket 보관
    SOCKET              acceptSocket{ INVALID_SOCKET };
};

// ─────────────────────────────────────────────────────────────────────────────
// PacketHandlerFunc
//
// std::function 을 사용하는 이유:
//   - 람다 (캡처 포함) 등록 가능
//   - 멤버 함수를 [this](auto s, auto& r){ OnXxx(s,r); } 형태로 등록 가능
//   - 핸들러 교체·확장 용이
//   - 패킷 디스패치 빈도 대비 std::function 오버헤드는 무시 가능한 수준
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
// IOCP 기반 비동기 TCP + UDP 네트워크 레이어.
//
// TCP  : 클라이언트 연결 유지, 로그인/상태/신뢰성 필요 메시지
// UDP  : 위치 동기화, 이동, 스킬 이펙트 등 빈도 높고 손실 허용 가능한 메시지
//
// 스레드 모델:
//   워커 스레드 N개가 GetQueuedCompletionStatus 루프 실행.
//   핸들러 콜백은 워커 스레드에서 호출됨 → 핸들러 내 공유 데이터 보호 필요.
//
// UDP 세션 식별:
//   UDP 패킷 헤더에 sessionToken(uint64) 포함.
//   서버는 토큰으로 Session 을 찾아 핸들러 디스패치.
//   클라이언트는 TCP 로그인 완료 후 발급받은 토큰을 UDP 헤더에 삽입.
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
    // 위치 동기화, 스킬 이펙트처럼 빠른 전송이 필요한 경우 사용.
    // TCP 전송은 Session::Send() 를 통해 처리.
    void SendUDP(const SOCKADDR_STORAGE& dest, const uint8_t* data, uint16_t length);

    // ── 세션 조회 ─────────────────────────────────────────────────────────────
    [[nodiscard]] std::shared_ptr<Session> FindSession(uint64_t sessionId) const;
    void CloseSession(uint64_t sessionId);

private:
    // ── IOCP ──────────────────────────────────────────────────────────────────
    HANDLE m_iocpHandle{ INVALID_HANDLE_VALUE };

    // ── TCP ───────────────────────────────────────────────────────────────────
    SOCKET                     m_tcpListenSocket{ INVALID_SOCKET };
    LPFN_ACCEPTEX              m_fnAcceptEx{ nullptr };             // 런타임 로드 (WSAIoctl)
    LPFN_GETACCEPTEXSOCKADDRS  m_fnGetAcceptExSockaddrs{ nullptr }; // 런타임 로드

    // ── UDP ───────────────────────────────────────────────────────────────────
    SOCKET       m_udpSocket{ INVALID_SOCKET };
    OverlappedEx m_udpRecvOv;  // 항상 pending 상태 유지 (RecvFrom 루프)

    // ── 워커 스레드 풀 ────────────────────────────────────────────────────────
    std::vector<std::thread> m_workerThreads;
    std::atomic<bool>        m_running{ false };

    // ── 패킷 핸들러 맵 ────────────────────────────────────────────────────────
    // opcode(uint16) → PacketHandlerFunc
    std::unordered_map<uint16_t, PacketHandlerFunc> m_handlerMap;

    // ── 세션 테이블 ───────────────────────────────────────────────────────────
    mutable std::mutex                                         m_sessionMutex;
    std::unordered_map<uint64_t, std::shared_ptr<Session>>    m_sessions;
    std::atomic<uint64_t>                                      m_nextSessionId{ 1 };

    // ── 내부 초기화 ───────────────────────────────────────────────────────────
    bool InitWinsock();
    bool CreateIOCP();
    bool SetupTCPListener(uint16_t port);
    bool SetupUDPSocket(uint16_t port);
    bool StartWorkerThreads(uint32_t count);

    // ── 비동기 I/O 예약 ───────────────────────────────────────────────────────
    void PostAccept();   // AcceptEx 로 비동기 accept 1건 예약
    void PostUDPRecv();  // WSARecvFrom 으로 UDP 수신 상시 예약

    // ── IOCP 워커 루프 ────────────────────────────────────────────────────────
    // 각 워커 스레드가 이 루프를 실행.
    // GetQueuedCompletionStatus → OverlappedEx.operation 분기 → On* 호출.
    void WorkerLoop();

    // ── 완료 이벤트 핸들러 ────────────────────────────────────────────────────
    void OnTCPAccept(OverlappedEx* pOv, DWORD bytes);
    void OnTCPRecv  (OverlappedEx* pOv, DWORD bytes, Session* session);
    void OnTCPSend  (OverlappedEx* pOv, DWORD bytes, Session* session);
    void OnUDPRecv  (OverlappedEx* pOv, DWORD bytes);
    void OnUDPSend  (OverlappedEx* pOv, DWORD bytes);

    // ── 패킷 디스패치 ─────────────────────────────────────────────────────────
    // opcode 로 m_handlerMap 조회 후 핸들러 호출.
    // 등록된 핸들러가 없으면 경고 로그 후 무시.
    void DispatchPacket(std::shared_ptr<Session> session, PacketReader& reader);

    // ── 세션 생명주기 ─────────────────────────────────────────────────────────
    std::shared_ptr<Session> CreateSession(SOCKET socket);
    void                     RemoveSession(uint64_t sessionId);
};
