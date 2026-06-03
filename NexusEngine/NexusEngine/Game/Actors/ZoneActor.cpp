#include "ZoneActor.h"
#include "SessionActor.h"
#include "../Data/GameDataEntities/NpcEntityData.h"
#include "../Data/GameDataEntities/MonsterEntityData.h"
#include "../Data/Skill/SkillRegistry.h"
#include "../Logic/Combat/CombatProcessor.h"

#include "../../Shared/Logger.h"
#include "../../protocol_shared/Packets/Packet-Auth.h"
#include "../../protocol_shared/Packets/Packet-Movement.h"
#include "../../protocol_shared/Packets/Packet-Chat.h"
#include "../../protocol_shared/Packets/Packet-Spawn.h"
#include "../../protocol_shared/Packets/Packet-Combat.h"
#include "../../protocol_shared/Packets/Packet-Item.h"
#include "../Logic/Movement/MovementProcessor.h"

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
        [this](MsgSession_UseSkill&     m) { Handle(m); },
        [this](MsgSession_UseSkin&      m) { Handle(m); },
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

static SMsg_SpawnPlayer BuildSpawnPacket(uint64_t sessionId, const PlayerPawn& pawn)
{
    return SMsg_SpawnPlayer{
        .pawnId       = pawn.GetPawnId(),
        .sessionId    = sessionId,
        .name         = pawn.GetName(),
        .hp           = static_cast<uint32_t>(pawn.GetHp()),
        .maxHp        = static_cast<uint32_t>(pawn.GetMaxHp()),
        .x            = pawn.GetPos().x,
        .y            = pawn.GetPos().y,
        .z            = pawn.GetPos().z,
        .orientation  = pawn.GetOrientation(),
        .skinHead     = pawn.GetCurrentSkinId(SKIN_PARTS_TYPE::_HEAD),
        .skinBodyUp   = pawn.GetCurrentSkinId(SKIN_PARTS_TYPE::_BODY_UP),
        .skinBodyDown = pawn.GetCurrentSkinId(SKIN_PARTS_TYPE::_BODY_DOWN),
        .skinHand     = pawn.GetCurrentSkinId(SKIN_PARTS_TYPE::_HAND),
        .skinShoes    = pawn.GetCurrentSkinId(SKIN_PARTS_TYPE::_SHOES),
    };
}

static void SendPlayerList(SessionActor& sa,
                           const std::unordered_map<uint64_t,
                                                    std::unique_ptr<PlayerPawn>>& pawns)
{
    for (auto& [sid, existing] : pawns)
        sa.Post(MsgZone_SendTcp{ BuildSpawnPacket(sid, *existing).Encode() });
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
    BroadcastTcp(msg.sessionId, BuildSpawnPacket(msg.sessionId, *pawn).Encode());

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
    BroadcastTcp(msg.sessionId, BuildSpawnPacket(msg.sessionId, *pawn).Encode());

    m_playerPawns.emplace(msg.sessionId, std::move(pawn));
    LOG_INFO("ZoneActor {}: 플레이어 추가 sessionId={} name={} pawnId={}",
             m_zone.GetId(), msg.sessionId, msg.characterName, pawnId);
}

void ZoneActor::Handle(MsgSession_Move& msg)
{
    auto* pawn = FindPlayerPawn(msg.sessionId);
    if (!pawn) return;

    const auto result = MovementProcessor::ApplyMove(
        *pawn, msg.pos, msg.orientation, msg.movementFlags, m_zone);

    if (result != MovementProcessor::EResult::Ok)
    {
        LOG_WARN("ZoneActor {}: 이동 거부 sessionId={} pos=({},{},{})",
                 m_zone.GetId(), msg.sessionId, msg.pos.x, msg.pos.y, msg.pos.z);
        return;
    }

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

    MovementProcessor::ApplyMoveUdp(*pawn, msg.pos, msg.orientation);

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

void ZoneActor::Handle(MsgSession_UseSkill& msg)
{
    // ── 공격자 확인 ───────────────────────────────────────────────────────────
    PlayerPawn* attacker = FindPlayerPawn(msg.sessionId);
    if (!attacker) return;

    // ── 스킬 정의 조회 ────────────────────────────────────────────────────────
    const SkillDef* def = SkillRegistry::Instance().Get(msg.skillId);
    if (!def)
    {
        LOG_WARN("ZoneActor {}: 알 수 없는 skillId={} sessionId={}",
                 m_zone.GetId(), msg.skillId, msg.sessionId);
        return;
    }

    // ── 대상 탐색 (단일 대상 스킬) ───────────────────────────────────────────
    Pawn* target = nullptr;
    if (msg.targetPawnId != 0)
    {
        // 플레이어 대상 우선 조회, 없으면 NPC/몬스터 조회
        for (auto& [sid, pawn] : m_playerPawns)
        {
            if (pawn->GetPawnId() == msg.targetPawnId)
            {
                target = pawn.get();
                break;
            }
        }
        if (!target) target = FindNpcPawn(msg.targetPawnId);
    }

    if (!target)
    {
        LOG_WARN("ZoneActor {}: 스킬 대상 없음 targetPawnId={}", m_zone.GetId(), msg.targetPawnId);
        return;
    }

    // ── 전투 처리 ─────────────────────────────────────────────────────────────
    const auto res = CombatProcessor::ProcessSingleTarget(*attacker, *target, *def);

    // 결과 브로드캐스트 (자신 포함 — 자신의 예측값과 비교용)
    BroadcastTcp(0, SMsg_SkillResult{
        .success        = res.success,
        .attackerPawnId = attacker->GetPawnId(),
        .targetPawnId   = target->GetPawnId(),
        .damage         = res.damage,
        .targetRemainHp = res.targetRemainHp,
        .message        = res.success ? "" : res.failReason
    }.Encode());

    if (!res.success) return;

    LOG_DEBUG("ZoneActor {}: 스킬 적중 skill={} attacker={} target={} dmg={} hp={}",
              m_zone.GetId(), def->name, attacker->GetPawnId(),
              target->GetPawnId(), res.damage, res.targetRemainHp);

    // 대상 사망 시 — HP 0 브로드캐스트 후 디스폰
    if (res.targetDied)
    {
        BroadcastTcp(0, SMsg_PawnHpUpdate{
            .pawnId = target->GetPawnId(),
            .hp     = 0,
            .maxHp  = static_cast<uint32_t>(target->GetMaxHp())
        }.Encode());

        // 플레이어 사망이면 세션 연결 유지, NPC/몬스터면 디스폰
        if (!target->IsPlayable())
        {
            DespawnNpc(target->GetPawnId());
            LOG_INFO("ZoneActor {}: NPC 사망 pawnId={}", m_zone.GetId(), target->GetPawnId());
        }
        else
        {
            LOG_INFO("ZoneActor {}: 플레이어 사망 pawnId={}", m_zone.GetId(), target->GetPawnId());
            // Phase 2: 리스폰 로직, 패널티 처리 예정
        }
    }
}

void ZoneActor::Handle(MsgSession_UseSkin& msg)
{
    auto* pawn = FindPlayerPawn(msg.sessionId);
    if (!pawn) return;

    const auto result = pawn->SwapSkin(msg.bagPos);
    if (!result.success)
    {
        LOG_WARN("ZoneActor {}: 스킨 장착 실패 sessionId={} bagPos={}",
                 m_zone.GetId(), msg.sessionId, msg.bagPos);
        return;
    }

    LOG_DEBUG("ZoneActor {}: 스킨 장착 sessionId={} partsType={} itemId={}",
              m_zone.GetId(), msg.sessionId, result.partsType, result.itemId);

    // 자신 포함 존 전체에 브로드캐스트 — 모든 클라이언트가 외형 갱신
    BroadcastTcp(0, SMsg_UseSkin{
        .sessionId = msg.sessionId,
        .partsType = result.partsType,
        .itemId    = result.itemId
    }.Encode());
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
