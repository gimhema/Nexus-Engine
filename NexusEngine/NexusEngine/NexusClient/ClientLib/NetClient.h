#pragma once

// ─────────────────────────────────────────────────────────────────────────────
// NetClient.h
//
// 플랫폼 독립 TCP 클라이언트 인터페이스.
// Windows / Linux / Unreal Engine 이식을 고려해 설계.
//
// 사용법:
//   NetClient client;
//   client.SetOnConnected   ([](){ /* 연결 완료 */ });
//   client.SetOnDisconnected([](){ /* 연결 해제 */ });
//   client.SetOnPacket([](uint16_t opcode, std::vector<uint8_t> payload){ ... });
//
//   client.Connect("127.0.0.1", 7070);  // 블로킹, 수신 스레드 시작
//   client.Send(packet);                // 스레드 안전
//   client.Disconnect();
//
// 주의:
//   콜백은 수신 스레드에서 호출됨.
//   콜백 내부에서 Send() 호출은 가능하나, Disconnect()는 데드락 위험.
// ─────────────────────────────────────────────────────────────────────────────

#include "Platform/Platform.h"
#include "Packets/PacketBase.h"

#include <atomic>
#include <functional>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

class NetClient
{
public:
    using OnConnectedFn    = std::function<void()>;
    using OnDisconnectedFn = std::function<void()>;
    using OnPacketFn       = std::function<void(uint16_t opcode,
                                                std::vector<uint8_t> payload)>;

    NetClient();
    ~NetClient();

    NetClient(const NetClient&)            = delete;
    NetClient& operator=(const NetClient&) = delete;

    // ── 콜백 등록 (Connect 전에 설정) ────────────────────────────────────────
    void SetOnConnected   (OnConnectedFn    fn) { m_onConnected    = std::move(fn); }
    void SetOnDisconnected(OnDisconnectedFn fn) { m_onDisconnected = std::move(fn); }
    void SetOnPacket      (OnPacketFn       fn) { m_onPacket       = std::move(fn); }

    // ── 연결 (블로킹) ─────────────────────────────────────────────────────────
    // 성공 시 수신 스레드를 시작하고 OnConnected 콜백 호출.
    bool Connect(const std::string& host, uint16_t port);

    // ── 연결 해제 ─────────────────────────────────────────────────────────────
    // 수신 스레드 종료 대기. 수신 스레드 내에서 호출 금지.
    void Disconnect();

    // ── 패킷 전송 (스레드 안전) ───────────────────────────────────────────────
    void Send(const std::vector<uint8_t>& packet);

    [[nodiscard]] bool IsConnected() const { return m_connected.load(); }

private:
    void RecvLoop();
    void OnDisconnectInternal();

    NxSocket              m_socket{ NX_INVALID_SOCKET };
    std::atomic<bool>     m_connected{ false };
    std::thread           m_recvThread;
    std::mutex            m_sendMutex;

    OnConnectedFn         m_onConnected;
    OnDisconnectedFn      m_onDisconnected;
    OnPacketFn            m_onPacket;

    // TCP 스트림 재조립 버퍼
    std::vector<uint8_t>  m_recvBuf;
};
