#pragma once

#include "Platform/Platform.h"

#include <cstdint>
#include <atomic>
#include <memory>
#include <mutex>
#include <queue>
#include <vector>

// ─────────────────────────────────────────────────────────────────────────────
// Session state
// ─────────────────────────────────────────────────────────────────────────────
enum class SessionState : uint8_t
{
    Connected,
    Closing,   // 종료 중 (미전송 데이터 플러시 대기)
    Closed,
};

// ─────────────────────────────────────────────────────────────────────────────
// RecvBuffer
//
// TCP는 스트림이므로 수신 데이터가 패킷 경계와 일치하지 않을 수 있음.
// 불완전한 데이터를 누적했다가 완전한 패킷이 모이면 처리.
//
// 레이아웃: [ ... consumed ... | readable data | ... free space ... ]
//                               ^m_readPos      ^m_writePos
// ─────────────────────────────────────────────────────────────────────────────
class RecvBuffer
{
public:
    explicit RecvBuffer(uint32_t capacity = 8192);

    // WSARecv 에 넘길 쓰기 포인터 / 여유 공간
    uint8_t* WritePtr();
    uint32_t FreeSpace() const;
    void     OnWritten(uint32_t bytes);

    // 패킷 파서가 읽을 포인터 / 읽을 수 있는 바이트 수
    const uint8_t* ReadPtr() const;
    uint32_t       Readable() const;
    void           Consume(uint32_t bytes);

    // 읽기 커서가 버퍼 중간 이상이면 앞으로 당김 (realloc 없음)
    void Compact();

private:
    std::vector<uint8_t> m_buf;
    uint32_t             m_readPos{ 0 };
    uint32_t             m_writePos{ 0 };
};

// ─────────────────────────────────────────────────────────────────────────────
// OverlappedEx (forward — 정의는 ServerNet.h)
// ─────────────────────────────────────────────────────────────────────────────
struct OverlappedEx;

// ─────────────────────────────────────────────────────────────────────────────
// Session
//
// TCP 클라이언트 연결 하나를 표현.
// NetworkManager와 패킷 핸들러 콜백 사이에서 shared_ptr로 공유.
//
// IOCP 패턴:
//   CreateIoCompletionPort(socket, iocpHandle, (ULONG_PTR)session.get(), 0)
//   → GetQueuedCompletionStatus의 completionKey = Session*
// ─────────────────────────────────────────────────────────────────────────────
class Session : public std::enable_shared_from_this<Session>
{
public:
    Session(NxSocket socket, uint64_t sessionId);
    ~Session();

    Session(const Session&)            = delete;
    Session& operator=(const Session&) = delete;

    // ── Identity ─────────────────────────────────────────────────────────────
    [[nodiscard]] uint64_t     GetSessionId() const { return m_sessionId; }
    [[nodiscard]] NxSocket     GetSocket()    const { return m_socket; }

    // ── State ─────────────────────────────────────────────────────────────────
    [[nodiscard]] SessionState GetState()     const { return m_state.load(); }
    [[nodiscard]] bool         IsConnected()  const { return m_state.load() == SessionState::Connected; }
    void                       SetState(SessionState s) { m_state.store(s); }

    // ── 송신 (스레드-세이프) ──────────────────────────────────────────────────
    // 내부 전송 큐에 복사 후, WSASend 가 idle 상태이면 즉시 전송 시작.
    void Send(const uint8_t* data, uint16_t length);
    void Send(const std::vector<uint8_t>& packet) { Send(packet.data(), static_cast<uint16_t>(packet.size())); }

    // ── 수신 버퍼 (NetworkManager 워커 스레드에서 접근) ──────────────────────
    RecvBuffer& GetRecvBuffer() { return m_recvBuffer; }

    // ── OVERLAPPED 접근자 ─────────────────────────────────────────────────────
    OverlappedEx* GetRecvOv() { return m_recvOv.get(); }
    OverlappedEx* GetSendOv() { return m_sendOv.get(); }

    // 최초 WSARecv 를 예약해 수신 루프를 시작
    void PostRecv(NxHandle iocpHandle);

    // OnTCPSend 완료 후 전송 큐에 남은 데이터를 드레인
    void FlushSendQueue(NxHandle iocpHandle);

private:
    NxSocket                   m_socket{ NX_INVALID_SOCKET };
    uint64_t                   m_sessionId{ 0 };
    std::atomic<SessionState>  m_state{ SessionState::Connected };

    // 수신
    RecvBuffer                   m_recvBuffer;
    std::unique_ptr<OverlappedEx> m_recvOv;

    // 송신
    std::mutex                        m_sendMutex;
    std::queue<std::vector<uint8_t>>  m_sendQueue;
    std::atomic<bool>                 m_sendPending{ false };
    std::unique_ptr<OverlappedEx>     m_sendOv;
};
