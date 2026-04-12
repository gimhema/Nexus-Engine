#include "ZoneActor.h"
#include "SessionActor.h"
#include "../Data/GameDataEntities/NpcEntityData.h"
#include "../Data/GameDataEntities/MonsterEntityData.h"

#include "../../Packets/PacketBase.h"
#include "../../Shared/Logger.h"
#include "../Data/GamePackets/Packet-Example.hpp"

ZoneActor::ZoneActor(Zone zone, WorldActor& world)
    : m_zone(std::move(zone))
    , m_world(world)
{}

void ZoneActor::OnStart()
{
    // Zone 설정에 정의된 NPC/몬스터 초기 스폰
    for (const auto& def : m_zone.GetNpcSpawns())
    {
        // entityType에 따라 적합한 EntityData 생성
        std::unique_ptr<GameDataEntityBase> data;
        if (def.entityType == EEntity::EID::MONSTER)
            data = std::make_unique<MonsterEntityData>(def.maxHp, def.attack, def.defense, def.moveSpeed,
                                                       def.factionId, def.aiType, def.aggroRange);
        else
            data = std::make_unique<NpcEntityData>(def.maxHp, def.attack, def.defense, def.moveSpeed,
                                                   def.factionId, def.aiType, def.aggroRange);

        auto pawn = std::make_unique<Pawn>(def.name, std::move(data));
        pawn->SetPos(def.pos);
        pawn->SetOrientation(def.orientation);
        SpawnNpc(std::move(pawn));
    }

    LOG_INFO("ZoneActor 시작: zoneId={} name=\"{}\" npc={}",
             m_zone.GetId(), m_zone.GetName(), m_npcPawns.size());
}

void ZoneActor::OnStop()
{
    LOG_INFO("ZoneActor 종료: zoneId={} players={} npcs={}",
             m_zone.GetId(), m_playerPawns.size(), m_npcPawns.size());
}

// ─────────────────────────────────────────────────────────────────────────────
// Tick — 50ms (20Hz)
// 모든 Pawn의 OnTick()을 순회 호출.
// Phase 2/3: GridCell AOI, AI FSM, Aura 틱 등 추가 예정.
// ─────────────────────────────────────────────────────────────────────────────
void ZoneActor::OnTick()
{
    ++m_tickCount;

    for (auto& [sid, pawn] : m_playerPawns)
        pawn->OnTick(m_tickCount);

    for (auto& [pid, pawn] : m_npcPawns)
        pawn->OnTick(m_tickCount);
}

// ─────────────────────────────────────────────────────────────────────────────
// 메시지 디스패치
// ─────────────────────────────────────────────────────────────────────────────
void ZoneActor::OnMessage(ZoneMessage& msg)
{
    std::visit(overloaded{
        [this](MsgSession_EnterZone&    m) { Handle(m); },
        [this](MsgSession_Move&         m) { Handle(m); },
        [this](MsgSession_MoveUdp&      m) { Handle(m); },
        [this](MsgSession_Chat&         m) { Handle(m); },
        [this](MsgSession_LeaveZone&    m) { Handle(m); },
        [this](MsgWorld_AddPlayer&      m) { Handle(m); },
        [this](MsgWorld_RemovePlayer&   m) { Handle(m); },
        [this](MsgGameLogic_WorldEvent& m) { Handle(m); },
    }, msg);
}

// ─────────────────────────────────────────────────────────────────────────────
// 메시지 핸들러
// ─────────────────────────────────────────────────────────────────────────────

// 직접 존 진입 경로 (SessionActor → ZoneActor, 또는 텔레포트 후속 처리)
void ZoneActor::Handle(MsgSession_EnterZone& msg)
{
    auto pawn = std::make_unique<PlayerPawn>(
        msg.characterName,
        msg.characterId,
        msg.sessionActor);

    pawn->SetPos(msg.spawnPos);
    pawn->OnSpawn();

    m_playerPawns.emplace(msg.sessionId, std::move(pawn));
    LOG_INFO("ZoneActor {}: 플레이어 진입 sessionId={} name={}",
             m_zone.GetId(), msg.sessionId, msg.characterName);
}

// WorldActor 경유 진입 (로그인/텔레포트 — 주 진입 경로)
void ZoneActor::Handle(MsgWorld_AddPlayer& msg)
{
    auto pawn = std::make_unique<PlayerPawn>(
        msg.characterName,
        msg.characterId,
        msg.sessionActor);

    // spawnPos가 원점이면 Zone 설정의 스폰 포인트 순환 사용
    const Vec3 spawnPos = (msg.spawnPos.x == 0.f && msg.spawnPos.y == 0.f && msg.spawnPos.z == 0.f)
                        ? m_zone.PickPlayerSpawn()
                        : msg.spawnPos;

    pawn->SetPos(spawnPos);
    pawn->OnSpawn();

    // 브로드캐스트에 사용할 폰 정보 — move 전에 캡처
    const uint64_t pawnId  = pawn->GetPawnId();
    const uint32_t hp      = static_cast<uint32_t>(pawn->GetHp());
    const uint32_t maxHp   = static_cast<uint32_t>(pawn->GetMaxHp());

    if (auto sa = msg.sessionActor.lock())
    {
        // ── 1. 신규 플레이어에게 본인 폰 정보 전송 ──────────────────────────
        {
            PacketWriter w(SMSG_ENTER_WORLD);
            w.WriteUInt8(1);
            w.WriteUInt64(pawnId);
            w.WriteUInt32(msg.characterId);
            w.WriteString(msg.characterName);
            w.WriteUInt32(hp);
            w.WriteUInt32(maxHp);
            w.WriteFloat(spawnPos.x);
            w.WriteFloat(spawnPos.y);
            w.WriteFloat(spawnPos.z);
            w.WriteFloat(0.f);   // orientation
            sa->Post(MsgZone_SendTcp{ w.Finalize() });
        }

        // ── 2. 신규 플레이어에게 기존 플레이어 목록 전송 ────────────────────
        for (auto& [sid, existing] : m_playerPawns)
        {
            PacketWriter w(SMSG_SPAWN_PLAYER);
            w.WriteUInt64(existing->GetPawnId());
            w.WriteUInt64(sid);
            w.WriteString(existing->GetName());
            w.WriteUInt32(static_cast<uint32_t>(existing->GetHp()));
            w.WriteUInt32(static_cast<uint32_t>(existing->GetMaxHp()));
            w.WriteFloat(existing->GetPos().x);
            w.WriteFloat(existing->GetPos().y);
            w.WriteFloat(existing->GetPos().z);
            w.WriteFloat(existing->GetOrientation());
            sa->Post(MsgZone_SendTcp{ w.Finalize() });
        }
    }

    // ── 3. 기존 플레이어들에게 신규 플레이어 스폰 브로드캐스트 ────────────
    {
        PacketWriter w(SMSG_SPAWN_PLAYER);
        w.WriteUInt64(pawnId);
        w.WriteUInt64(msg.sessionId);
        w.WriteString(msg.characterName);
        w.WriteUInt32(hp);
        w.WriteUInt32(maxHp);
        w.WriteFloat(spawnPos.x);
        w.WriteFloat(spawnPos.y);
        w.WriteFloat(spawnPos.z);
        w.WriteFloat(0.f);
        BroadcastTcp(msg.sessionId, w.Finalize());
    }

    m_playerPawns.emplace(msg.sessionId, std::move(pawn));
    LOG_INFO("ZoneActor {}: 플레이어 추가 sessionId={} name={} pawnId={}",
             m_zone.GetId(), msg.sessionId, msg.characterName, pawnId);
}

void ZoneActor::Handle(MsgSession_Move& msg)
{
    auto* pawn = FindPlayerPawn(msg.sessionId);
    if (!pawn) return;

    // 존 경계 검증 — 범위 밖 이동은 무시
    if (!m_zone.IsInBounds(msg.pos))
    {
        LOG_WARN("ZoneActor {}: 경계 밖 이동 sessionId={} pos=({},{},{})",
                 m_zone.GetId(), msg.sessionId, msg.pos.x, msg.pos.y, msg.pos.z);
        return;
    }

    pawn->SetPos(msg.pos);
    pawn->SetOrientation(msg.orientation);

    PacketWriter w(SMSG_MOVE_BROADCAST);
    w.WriteUInt64(msg.sessionId);
    w.WriteFloat(msg.pos.x).WriteFloat(msg.pos.y).WriteFloat(msg.pos.z);
    w.WriteFloat(msg.orientation);
    BroadcastTcp(msg.sessionId, w.Finalize());
}

void ZoneActor::Handle(MsgSession_MoveUdp& msg)
{
    auto* pawn = FindPlayerPawn(msg.sessionId);
    if (!pawn) return;

    pawn->SetPos(msg.pos);
    pawn->SetOrientation(msg.orientation);

    PacketWriter w(SMSG_MOVE_UDP);
    w.WriteUInt64(msg.sessionId);
    w.WriteFloat(msg.pos.x).WriteFloat(msg.pos.y).WriteFloat(msg.pos.z);
    w.WriteFloat(msg.orientation);
    BroadcastUdp(msg.sessionId, w.Finalize());
}

void ZoneActor::Handle(MsgSession_Chat& msg)
{
    auto* pawn = FindPlayerPawn(msg.sessionId);
    if (!pawn) return;

    LOG_DEBUG("ZoneActor {}: 채팅 [{}] {}", m_zone.GetId(), pawn->GetName(), msg.text);

    PacketWriter w(SMSG_CHAT);
    w.WriteUInt64(msg.sessionId);
    w.WriteString(pawn->GetName());
    w.WriteString(msg.text);
    BroadcastTcp(0, w.Finalize());  // 자신 포함 전체 브로드캐스트
}

void ZoneActor::Handle(MsgSession_LeaveZone& msg)
{
    auto it = m_playerPawns.find(msg.sessionId);
    if (it == m_playerPawns.end()) return;

    const uint64_t pawnId = it->second->GetPawnId();
    it->second->OnDespawn();
    m_playerPawns.erase(it);

    PacketWriter w(SMSG_DESPAWN_PLAYER);
    w.WriteUInt64(pawnId);
    BroadcastTcp(msg.sessionId, w.Finalize());

    LOG_INFO("ZoneActor {}: 플레이어 퇴장 sessionId={} pawnId={}", m_zone.GetId(), msg.sessionId, pawnId);
}

void ZoneActor::Handle(MsgWorld_RemovePlayer& msg)
{
    auto it = m_playerPawns.find(msg.sessionId);
    if (it == m_playerPawns.end()) return;

    const uint64_t pawnId = it->second->GetPawnId();
    it->second->OnDespawn();
    m_playerPawns.erase(it);

    PacketWriter w(SMSG_DESPAWN_PLAYER);
    w.WriteUInt64(pawnId);
    BroadcastTcp(msg.sessionId, w.Finalize());
}

void ZoneActor::Handle(MsgGameLogic_WorldEvent& msg)
{
    LOG_INFO("ZoneActor {}: 월드 이벤트 [{}] 게임 시각 {:.1f}시",
             m_zone.GetId(), msg.eventName, msg.gameHour);

    // TODO Phase 2: 이벤트 종류에 따라 존 상태 변경
    //   DayBegin/NightBegin → 크리처 스폰 패턴 변경
    //   DungeonOpen/Close   → 던전 입장 포털 활성화/비활성화 + NPC 스폰
}

// ─────────────────────────────────────────────────────────────────────────────
// NPC 스폰 / 디스폰
// ─────────────────────────────────────────────────────────────────────────────
void ZoneActor::SpawnNpc(std::unique_ptr<Pawn> pawn)
{
    pawn->OnSpawn();
    uint64_t id = pawn->GetPawnId();
    LOG_DEBUG("ZoneActor {}: NPC 스폰 pawnId={} name={}", m_zone.GetId(), id, pawn->GetName());
    m_npcPawns.emplace(id, std::move(pawn));
}

void ZoneActor::DespawnNpc(uint64_t pawnId)
{
    auto it = m_npcPawns.find(pawnId);
    if (it == m_npcPawns.end()) return;

    it->second->OnDespawn();
    LOG_DEBUG("ZoneActor {}: NPC 디스폰 pawnId={}", m_zone.GetId(), pawnId);
    m_npcPawns.erase(it);
}

// ─────────────────────────────────────────────────────────────────────────────
// 브로드캐스트 헬퍼
// ─────────────────────────────────────────────────────────────────────────────
void ZoneActor::BroadcastTcp(uint64_t excludeSessionId, const std::vector<uint8_t>& packet)
{
    for (auto& [sid, pawn] : m_playerPawns)
    {
        if (sid == excludeSessionId) continue;
        if (auto sa = pawn->GetSession())
            sa->Post(MsgZone_SendTcp{ packet });
    }
}

void ZoneActor::BroadcastUdp(uint64_t excludeSessionId, const std::vector<uint8_t>& packet)
{
    for (auto& [sid, pawn] : m_playerPawns)
    {
        if (sid == excludeSessionId) continue;
        if (auto sa = pawn->GetSession())
            sa->Post(MsgZone_SendUdp{ packet });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Pawn 조회 헬퍼
// ─────────────────────────────────────────────────────────────────────────────
PlayerPawn* ZoneActor::FindPlayerPawn(uint64_t sessionId) const
{
    auto it = m_playerPawns.find(sessionId);
    return (it != m_playerPawns.end()) ? it->second.get() : nullptr;
}

Pawn* ZoneActor::FindNpcPawn(uint64_t pawnId) const
{
    auto it = m_npcPawns.find(pawnId);
    return (it != m_npcPawns.end()) ? it->second.get() : nullptr;
}

std::shared_ptr<SessionActor> ZoneActor::GetSessionActor(uint64_t sessionId) const
{
    auto* pawn = FindPlayerPawn(sessionId);
    return pawn ? pawn->GetSession() : nullptr;
}
