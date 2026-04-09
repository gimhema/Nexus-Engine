#pragma once

#include "../../Actor/Actor.h"
#include "../../Game/Messages/GameMessages.h"

#include <unordered_map>
#include <memory>
#include <string>
#include <cstdint>

class SessionActor;
class WorldActor;

// ─────────────────────────────────────────────────────────────────────────────
// PlayerState
//
// ZoneActor가 소유하는 플레이어 인게임 상태.
// 외부에서 직접 접근 불가 — ZoneActor 메시지로만 변경.
// ─────────────────────────────────────────────────────────────────────────────
struct PlayerState
{
    uint64_t    sessionId{};
    uint32_t    characterId{};
    std::string name;
    Vec3        pos;
    float       orientation{ 0.f };
};

// ─────────────────────────────────────────────────────────────────────────────
// ZoneActor
//
// 게임 핵심 로직을 담당하는 Dedicated Actor (전용 스레드 + tick 루프).
// - 존 내 모든 플레이어 상태를 독점 소유
// - tick마다 게임 로직 갱신 (이동 검증, AI, AOI 등 — Phase 2/3에서 구현)
// - 인트라존 이벤트(이동·전투·채팅)는 내부에서 완결 → 뮤텍스 불필요
// ─────────────────────────────────────────────────────────────────────────────
class ZoneActor : public Actor<ZoneMessage>
{
public:
    explicit ZoneActor(uint32_t zoneId, WorldActor& world);
    ~ZoneActor() override = default;

    [[nodiscard]] uint32_t GetZoneId() const { return m_zoneId; }

protected:
    void OnMessage(ZoneMessage& msg) override;
    void OnTick()                    override;
    void OnStart()                   override;
    void OnStop()                    override;

private:
    void Handle(MsgSession_EnterZone& msg);
    void Handle(MsgSession_Move& msg);
    void Handle(MsgSession_MoveUdp& msg);
    void Handle(MsgSession_Chat& msg);
    void Handle(MsgSession_LeaveZone& msg);
    void Handle(MsgWorld_AddPlayer& msg);
    void Handle(MsgWorld_RemovePlayer& msg);
    void Handle(MsgGameLogic_WorldEvent& msg);

    // AOI: 인근 플레이어에게 패킷 브로드캐스트 (자신 제외)
    void BroadcastTcp(uint64_t excludeSessionId, const std::vector<uint8_t>& packet);
    void BroadcastUdp(uint64_t excludeSessionId, const std::vector<uint8_t>& packet);

    // weak_ptr 잠금 실패(SessionActor 소멸) 시 nullptr 반환
    [[nodiscard]] std::shared_ptr<SessionActor> FindSessionActor(uint64_t sessionId) const;

    uint32_t    m_zoneId{};
    WorldActor& m_world;

    std::unordered_map<uint64_t, PlayerState>                m_players;        // 게임 상태 (ZoneActor 소유)
    std::unordered_map<uint64_t, std::weak_ptr<SessionActor>> m_sessionActors; // 송신용 weak 레퍼런스

    uint64_t m_tickCount{ 0 };
};
