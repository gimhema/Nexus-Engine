#pragma once

#include "../Platform/Platform.h"
#include "../Packets/PacketBase.h"

#include <atomic>
#include <functional>
#include <mutex>
#include <thread>
#include <vector>

// ─────────────────────────────────────────────────────────────────────────────
// ArbiterSession — 런처 연결 1개
//
// 전용 수신 스레드에서 패킷을 누적·조립해 Arbiter 콜백으로 전달한다.
// 송신은 mutex로 보호되어 스레드 안전하다.
//
// 생명주기:
//   Arbiter가 shared_ptr<ArbiterSession>으로 소유.
//   소멸자에서 recv 스레드 join을 보장하므로
//   Arbiter::Stop() 이후 shared_ptr 해제 시 안전하게 정리된다.
// ─────────────────────────────────────────────────────────────────────────────
class ArbiterSession
{
public:
    // opcode + payload(헤더 제외) 수신 콜백
    using OnPacketFunc = std::function<void(ArbiterSession&, uint16_t, std::vector<uint8_t>)>;
    // 연결 종료 콜백
    using OnCloseFunc  = std::function<void(ArbiterSession&)>;

    explicit ArbiterSession(NxSocket socket);
    ~ArbiterSession();

    ArbiterSession(const ArbiterSession&)            = delete;
    ArbiterSession& operator=(const ArbiterSession&) = delete;

    // 수신 스레드 시작 — Start() 이후 콜백이 다른 스레드에서 호출될 수 있음
    void Start(OnPacketFunc onPacket, OnCloseFunc onClose);

    // 패킷 전송 (스레드 안전)
    void Send(const std::vector<uint8_t>& packet);

    // 소켓 닫기 — recv 스레드가 자연스럽게 탈출하도록 유도
    void Close();

    [[nodiscard]] bool IsAuthenticated() const { return m_authenticated; }
    void SetAuthenticated(bool v) { m_authenticated = v; }

    [[nodiscard]] bool IsClosed() const { return m_closed.load(); }

private:
    void RecvLoop(OnPacketFunc onPacket, OnCloseFunc onClose);

    NxSocket             m_socket{ NX_INVALID_SOCKET };
    std::thread          m_recvThread;
    std::mutex           m_sendMutex;
    std::atomic<bool>    m_closed{ false };
    bool                 m_authenticated{ false };

    // 부분 수신 패킷 누적 버퍼
    std::vector<uint8_t> m_recvBuf;
};
