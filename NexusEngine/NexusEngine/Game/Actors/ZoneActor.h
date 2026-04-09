#pragma once

#include "../../Actor/Actor.h"
#include "../../Game/Messages/GameMessages.h"
#include "../Logic/Pawn/PlayerPawn.h"

#include <cstdint>
#include <memory>
#include <string>
#include <unordered_map>
#include <vector>

class SessionActor;
class WorldActor;

// ─────────────────────────────────────────────────────────────────────────────
// ZoneActor — 게임 로직의 핵심 단위 (Dedicated Actor + Pawn 관리자)
//
// 역할:
//   1. 존 내 모든 Pawn 소유 및 생명주기 관리
//      - m_playerPawns : sessionId → PlayerPawn (플레이어블)
//      - m_npcPawns    : pawnId   → Pawn        (NPC/몬스터)
//   2. 50ms tick 루프에서 모든 Pawn의 OnTick() 호출
//   3. 인트라존 이벤트 처리 (이동, 채팅, 브로드캐스트)
//   4. WorldEvent 수신 → 스폰/디스폰 등 존 상태 변경 (Phase 2)
//
// 스레드 안전성:
//   ZoneActor 전용 스레드만 Pawn에 직접 접근한다.
//   외부 Actor는 반드시 ZoneActor 메시지를 통해 간접 요청해야 한다.
//   SpawnNpc / DespawnNpc 는 ZoneActor 스레드 내부 또는 Start() 전 초기화
//   구간(단일 스레드)에서만 호출해야 한다.
// ─────────────────────────────────────────────────────────────────────────────
class ZoneActor : public Actor<ZoneMessage>
{
public:
    explicit ZoneActor(uint32_t zoneId, WorldActor& world);
    ~ZoneActor() override = default;

    [[nodiscard]] uint32_t GetZoneId() const { return m_zoneId; }

    // ── NPC/몬스터 스폰 ───────────────────────────────────────────────────
    // pawn의 소유권을 ZoneActor로 이전.
    // 호출 시점: Start() 전 초기화 구간 또는 Handle(MsgGameLogic_WorldEvent) 내부.
    void SpawnNpc(std::unique_ptr<Pawn> pawn);

    // NPC pawnId로 디스폰 (존재하지 않으면 무시)
    void DespawnNpc(uint64_t pawnId);

protected:
    void OnMessage(ZoneMessage& msg) override;
    void OnTick()                    override;
    void OnStart()                   override;
    void OnStop()                    override;

private:
    // ── 메시지 핸들러 ─────────────────────────────────────────────────────
    void Handle(MsgSession_EnterZone&    msg);
    void Handle(MsgSession_Move&         msg);
    void Handle(MsgSession_MoveUdp&      msg);
    void Handle(MsgSession_Chat&         msg);
    void Handle(MsgSession_LeaveZone&    msg);
    void Handle(MsgWorld_AddPlayer&      msg);
    void Handle(MsgWorld_RemovePlayer&   msg);
    void Handle(MsgGameLogic_WorldEvent& msg);

    // ── 브로드캐스트 헬퍼 ────────────────────────────────────────────────
    // excludeSessionId = 0 → 전체 플레이어에게 전송
    void BroadcastTcp(uint64_t excludeSessionId, const std::vector<uint8_t>& packet);
    void BroadcastUdp(uint64_t excludeSessionId, const std::vector<uint8_t>& packet);

    // ── Pawn 조회 헬퍼 ───────────────────────────────────────────────────
    // 반환 nullptr → 존재하지 않음 (포인터 소유권 없음)
    [[nodiscard]] PlayerPawn*                   FindPlayerPawn(uint64_t sessionId) const;
    [[nodiscard]] Pawn*                         FindNpcPawn(uint64_t pawnId) const;
    [[nodiscard]] std::shared_ptr<SessionActor> GetSessionActor(uint64_t sessionId) const;

    // ── 데이터 ───────────────────────────────────────────────────────────
    uint32_t    m_zoneId{};
    WorldActor& m_world;

    // 플레이어블 Pawn — sessionId 키 (이동/채팅 메시지의 sessionId로 즉시 조회)
    std::unordered_map<uint64_t, std::unique_ptr<PlayerPawn>> m_playerPawns;

    // 논플레이어블 Pawn — pawnId 키 (NPC, 몬스터, 소환수 등)
    std::unordered_map<uint64_t, std::unique_ptr<Pawn>> m_npcPawns;

    uint64_t m_tickCount{ 0 };
};
