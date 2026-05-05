#include "ZoneActor.h"
#include "SessionActor.h"
#include "../Data/GameDataEntities/NpcEntityData.h"
#include "../Data/GameDataEntities/MonsterEntityData.h"

#include "../../Shared/Logger.h"
#include "../../protocol_shared/Packets/Packet-Auth.h"
#include "../../protocol_shared/Packets/Packet-Movement.h"
#include "../../protocol_shared/Packets/Packet-Chat.h"
#include "../../protocol_shared/Packets/Packet-Spawn.h"

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
// 플레이어 진입 공통 처리
// ─────────────────────────────────────────────────────────────────────────────
static Vec3 ResolveSpawnPos(const Vec3& requested, const Zone& zone)
{
    return (requested.x == 0.f && requested.y == 0.f && requested.z == 0.f)
         ? zone.PickPlayerSpawn()
         : requested;
}

static void SendPlayerList(SessionActor& sa,
                           const std::unordered_map<uint64_t,
                                                    std::unique_ptr<PlayerPawn>>& pawns)
{
    for (auto& [sid, existing] : pawns)
    {
        sa.Post(MsgZone_SendTcp{ SMsg_SpawnPlayer{
            .pawnId      = existing->GetPawnId(),
            .sessionId   = sid,
            .name        = existing->GetName(),
            .hp          = static_cast<uint32_t>(existing->GetHp()),
            .maxHp       = static_cast<uint32_t>(existing->GetMaxHp()),
            .x           = existing->GetPos().x,
            .y           = existing->GetPos().y,
            .z           = existing->GetPos().z,
            .orientation = existing->GetOrientation()
        }.Encode() });
    }
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

    const Vec3 spawnPos = ResolveSpawnPos(msg.spawnPos, m_zone);
    pawn->SetPos(spawnPos);
    pawn->OnSpawn();

    const uint64_t pawnId = pawn->GetPawnId();
    const uint32_t hp     = static_cast<uint32_t>(pawn->GetHp());
    const uint32_t maxHp  = static_cast<uint32_t>(pawn->GetMaxHp());

    if (auto sa = msg.sessionActor.lock())
    {
        // 1. 신규 플레이어에게 본인 폰 정보 전송
        sa->Post(MsgZone_SendTcp{ SMsg_EnterWorld{
            .success     = true,
            .pawnId      = pawnId,
            .characterId = msg.characterId,
            .name        = msg.characterName,
            .hp          = hp,
            .maxHp       = maxHp,
            .x           = spawnPos.x,
            .y           = spawnPos.y,
            .z           = spawnPos.z,
            .orientation = 0.f
        }.Encode() });

        // 2. 신규 플레이어에게 기존 플레이어 목록 전송
        SendPlayerList(*sa, m_playerPawns);
    }

    // 3. 기존 플레이어들에게 신규 플레이어 스폰 브로드캐스트
    BroadcastTcp(msg.sessionId, SMsg_SpawnPlayer{
        .pawnId      = pawnId,
        .sessionId   = msg.sessionId,
        .name        = msg.characterName,
        .hp          = hp,
        .maxHp       = maxHp,
        .x           = spawnPos.x,
        .y           = spawnPos.y,
        .z           = spawnPos.z,
        .orientation = 0.f
    }.Encode());

    m_playerPawns.emplace(msg.sessionId, std::move(pawn));
    LOG_INFO("ZoneActor {}: 플레이어 진입 sessionId={} name={} pawnId={}",
             m_zone.GetId(), msg.sessionId, msg.characterName, pawnId);
}

// WorldActor 경유 진입 (로그인/텔레포트 — 주 진입 경로)
void ZoneActor::Handle(MsgWorld_AddPlayer& msg)
{
    auto pawn = std::make_unique<PlayerPawn>(
        msg.characterName,
        msg.characterId,
        msg.sessionActor);

    const Vec3 spawnPos = ResolveSpawnPos(msg.spawnPos, m_zone);
    pawn->SetPos(spawnPos);
    pawn->OnSpawn();

    const uint64_t pawnId = pawn->GetPawnId();
    const uint32_t hp     = static_cast<uint32_t>(pawn->GetHp());
    const uint32_t maxHp  = static_cast<uint32_t>(pawn->GetMaxHp());

    if (auto sa = msg.sessionActor.lock())
    {
        // 1. 신규 플레이어에게 본인 폰 정보 전송
        sa->Post(MsgZone_SendTcp{ SMsg_EnterWorld{
            .success     = true,
            .pawnId      = pawnId,
            .characterId = msg.characterId,
            .name        = msg.characterName,
            .hp          = hp,
            .maxHp       = maxHp,
            .x           = spawnPos.x,
            .y           = spawnPos.y,
            .z           = spawnPos.z,
            .orientation = 0.f
        }.Encode() });

        // 2. 신규 플레이어에게 기존 플레이어 목록 전송
        SendPlayerList(*sa, m_playerPawns);
    }

    // 3. 기존 플레이어들에게 신규 플레이어 스폰 브로드캐스트
    BroadcastTcp(msg.sessionId, SMsg_SpawnPlayer{
        .pawnId      = pawnId,
        .sessionId   = msg.sessionId,
        .name        = msg.characterName,
        .hp          = hp,
        .maxHp       = maxHp,
        .x           = spawnPos.x,
        .y           = spawnPos.y,
        .z           = spawnPos.z,
        .orientation = 0.f
    }.Encode());

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

    BroadcastTcp(msg.sessionId, SMsg_MoveBroadcast{
        .sessionId   = msg.sessionId,
        .x           = msg.pos.x,
        .y           = msg.pos.y,
        .z           = msg.pos.z,
        .orientation = msg.orientation
    }.Encode());
}

void ZoneActor::Handle(MsgSession_MoveUdp& msg)
{
    auto* pawn = FindPlayerPawn(msg.sessionId);
    if (!pawn) return;

    pawn->SetPos(msg.pos);
    pawn->SetOrientation(msg.orientation);

    BroadcastUdp(msg.sessionId, SMsg_MoveUdp{
        .sessionId   = msg.sessionId,
        .x           = msg.pos.x,
        .y           = msg.pos.y,
        .z           = msg.pos.z,
        .orientation = msg.orientation
    }.Encode());
}

void ZoneActor::Handle(MsgSession_Chat& msg)
{
    auto* pawn = FindPlayerPawn(msg.sessionId);
    if (!pawn) return;

    LOG_DEBUG("ZoneActor {}: 채팅 [{}] {}", m_zone.GetId(), pawn->GetName(), msg.text);

    BroadcastTcp(0, SMsg_Chat{
        .sessionId = msg.sessionId,
        .name      = pawn->GetName(),
        .text      = msg.text
    }.Encode());
}

void ZoneActor::Handle(MsgSession_LeaveZone& msg)
{
    auto it = m_playerPawns.find(msg.sessionId);
    if (it == m_playerPawns.end()) return;

    const uint64_t pawnId = it->second->GetPawnId();
    it->second->OnDespawn();
    m_playerPawns.erase(it);

    BroadcastTcp(msg.sessionId, SMsg_DespawnPlayer{ .pawnId = pawnId }.Encode());

    LOG_INFO("ZoneActor {}: 플레이어 퇴장 sessionId={} pawnId={}", m_zone.GetId(), msg.sessionId, pawnId);
}

void ZoneActor::Handle(MsgWorld_RemovePlayer& msg)
{
    auto it = m_playerPawns.find(msg.sessionId);
    if (it == m_playerPawns.end()) return;

    const uint64_t pawnId = it->second->GetPawnId();
    it->second->OnDespawn();
    m_playerPawns.erase(it);

    BroadcastTcp(msg.sessionId, SMsg_DespawnPlayer{ .pawnId = pawnId }.Encode());
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
