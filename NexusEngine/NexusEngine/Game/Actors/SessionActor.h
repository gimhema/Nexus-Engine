#pragma once

#include "../../Actor/Actor.h"
#include "../../Actor/ActorSystem.h"
#include "../../Game/Messages/GameMessages.h"

#include <atomic>
#include <memory>
#include <cstdint>

class Session;
class WorldActor;
class ZoneActor;

// ─────────────────────────────────────────────────────────────────────────────
// SessionActor
//
// 클라이언트 연결 1개에 대응하는 Pooled Actor.
// - 네트워크 워커가 패킷 수신 시 Post(MsgNet_PacketReceived) 호출
// - 패킷을 파싱해 ZoneActor 또는 WorldActor에 전달
// - ZoneActor로부터 MsgZone_SendTcp / MsgZone_SendUdp 수신 시 Session을 통해 전송
//
// Pooled 모드: 메시지가 Post될 때 ActorSystem에 ProcessMailbox를 스케줄링.
// ─────────────────────────────────────────────────────────────────────────────
class SessionActor : public Actor<SessionMessage>
{
public:
    SessionActor(std::shared_ptr<Session> session,
                 WorldActor&              world);
    ~SessionActor() override = default;

    [[nodiscard]] uint64_t GetSessionId() const { return m_sessionId; }

    // ZoneActor 연결 — WorldActor 스레드에서 호출, SessionActor 스레드에서 읽음
    // atomic으로 동기화
    void SetZone(ZoneActor* zone) { m_zone.store(zone, std::memory_order_release); }
    void ClearZone()              { m_zone.store(nullptr, std::memory_order_release); }

    // Post 오버라이드 — 메시지 투입 후 ActorSystem에 스케줄링
    // m_scheduled 플래그로 ProcessMailbox()가 동시에 두 번 실행되는 것을 방지.
    // MPSC 큐는 단일 소비자 계약이므로 동시 TryPop() 호출은 UB를 일으킨다.
    void Post(SessionMessage msg)
    {
        Actor<SessionMessage>::Post(std::move(msg));
        if (!m_scheduled.exchange(true, std::memory_order_acq_rel))
            ActorSystem::Instance().Schedule([this] { RunMailbox(); });
    }

protected:
    void OnMessage(SessionMessage& msg) override;

private:
    // 처리 루프 — m_scheduled를 false로 되돌리며 새 메시지가 있으면 재획득
    void RunMailbox()
    {
        while (true)
        {
            ProcessMailbox();
            // 처리 완료 선언 (seq_cst: 이후 HasPendingMessages 읽기보다 앞에 오도록 보장)
            m_scheduled.store(false, std::memory_order_seq_cst);
            if (!HasPendingMessages()) return;
            // 처리 중 새 메시지 도착 — 재획득 시도
            if (m_scheduled.exchange(true, std::memory_order_acq_rel)) return;
        }
    }

    void Handle(MsgNet_PacketReceived& msg);
    void Handle(MsgZone_SendTcp& msg);
    void Handle(MsgZone_SendUdp& msg);
    void Handle(MsgZone_Disconnect& msg);
    void Handle(MsgWorld_LoginResult& msg);
    void Handle(MsgWorld_CharSetupResult& msg);

    std::shared_ptr<Session>  m_session;
    WorldActor&               m_world;
    std::atomic<ZoneActor*>   m_zone{ nullptr };
    std::atomic<bool>         m_scheduled{ false };
    uint64_t                  m_sessionId{ 0 };
};
