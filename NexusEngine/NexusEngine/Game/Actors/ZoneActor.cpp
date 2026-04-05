#include "ZoneActor.h"
#include "SessionActor.h"

#include "../../Packets/PacketBase.h"
#include "../../Shared/Logger.h"
#include "../Data/GamePackets/Packet-Example.hpp"

ZoneActor::ZoneActor(uint32_t zoneId, WorldActor& world)
    : m_zoneId(zoneId)
    , m_world(world)
{}

void ZoneActor::OnStart()
{
    LOG_INFO("ZoneActor 시작: zoneId={}", m_zoneId);
}

void ZoneActor::OnStop()
{
    LOG_INFO("ZoneActor 종료: zoneId={}", m_zoneId);
}

void ZoneActor::OnMessage(ZoneMessage& msg)
{
    std::visit(overloaded{
        [this](MsgSession_EnterZone& m)  { Handle(m); },
        [this](MsgSession_Move& m)       { Handle(m); },
        [this](MsgSession_MoveUdp& m)    { Handle(m); },
        [this](MsgSession_Chat& m)       { Handle(m); },
        [this](MsgSession_LeaveZone& m)  { Handle(m); },
        [this](MsgWorld_AddPlayer& m)    { Handle(m); },
        [this](MsgWorld_RemovePlayer& m) { Handle(m); },
    }, msg);
}

// ─────────────────────────────────────────────────────────────────────────────
// Tick — 게임 루프 (50ms = 20Hz)
// Phase 2/3에서 AI, AOI 갱신, 버프 틱 등을 여기에 추가
// ─────────────────────────────────────────────────────────────────────────────
void ZoneActor::OnTick()
{
    ++m_tickCount;
    // TODO: GridCell AOI 갱신, Creature AI tick, Aura 틱 등
}

// ─────────────────────────────────────────────────────────────────────────────
// 메시지 핸들러
// ─────────────────────────────────────────────────────────────────────────────
void ZoneActor::Handle(MsgSession_EnterZone& msg)
{
    PlayerState ps;
    ps.sessionId   = msg.sessionId;
    ps.characterId = msg.characterId;
    ps.name        = msg.characterName;
    ps.pos         = msg.spawnPos;
    m_players[msg.sessionId] = ps;

    LOG_INFO("ZoneActor {}: 플레이어 진입 sessionId={} name={}", m_zoneId, msg.sessionId, msg.characterName);
}

void ZoneActor::Handle(MsgWorld_AddPlayer& msg)
{
    // WorldActor 경유 진입 (로그인/텔레포트)
    PlayerState ps;
    ps.sessionId   = msg.sessionId;
    ps.characterId = msg.characterId;
    ps.name        = msg.characterName;
    ps.pos         = msg.spawnPos;
    m_players[msg.sessionId] = ps;

    // SessionActor 등록 — ZoneActor 스레드 내에서 처리해 레이스 방지
    m_sessionActors[msg.sessionId] = msg.sessionActor;

    LOG_INFO("ZoneActor {}: 플레이어 추가 sessionId={}", m_zoneId, msg.sessionId);

    // 진입 클라이언트에 스폰 위치 전송
    if (auto sa = msg.sessionActor.lock())
    {
        PacketWriter w(SMSG_ENTER_WORLD);
        w.WriteUInt8(1);
        w.WriteFloat(msg.spawnPos.x);
        w.WriteFloat(msg.spawnPos.y);
        w.WriteFloat(msg.spawnPos.z);
        sa->Post(MsgZone_SendTcp{ w.Finalize() });
    }
}

void ZoneActor::Handle(MsgSession_Move& msg)
{
    auto it = m_players.find(msg.sessionId);
    if (it == m_players.end()) return;

    // TODO: 이동 검증 (Phase 2)
    it->second.pos         = msg.pos;
    it->second.orientation = msg.orientation;

    // 인근 플레이어에게 브로드캐스트
    PacketWriter w(SMSG_MOVE_BROADCAST);
    w.WriteUInt64(msg.sessionId);
    w.WriteFloat(msg.pos.x).WriteFloat(msg.pos.y).WriteFloat(msg.pos.z);
    w.WriteFloat(msg.orientation);
    BroadcastTcp(msg.sessionId, w.Finalize());
}

void ZoneActor::Handle(MsgSession_MoveUdp& msg)
{
    auto it = m_players.find(msg.sessionId);
    if (it == m_players.end()) return;

    it->second.pos         = msg.pos;
    it->second.orientation = msg.orientation;

    PacketWriter w(SMSG_MOVE_UDP);
    w.WriteUInt64(msg.sessionId);
    w.WriteFloat(msg.pos.x).WriteFloat(msg.pos.y).WriteFloat(msg.pos.z);
    w.WriteFloat(msg.orientation);
    BroadcastUdp(msg.sessionId, w.Finalize());
}

void ZoneActor::Handle(MsgSession_Chat& msg)
{
    auto it = m_players.find(msg.sessionId);
    if (it == m_players.end()) return;

    LOG_DEBUG("ZoneActor {}: 채팅 [{}] {}", m_zoneId, it->second.name, msg.text);

    PacketWriter w(SMSG_CHAT);
    w.WriteUInt64(msg.sessionId);
    w.WriteString(it->second.name);
    w.WriteString(msg.text);
    // 자신 포함 전체 브로드캐스트
    BroadcastTcp(0, w.Finalize());
}

void ZoneActor::Handle(MsgSession_LeaveZone& msg)
{
    m_players.erase(msg.sessionId);
    m_sessionActors.erase(msg.sessionId);
    LOG_INFO("ZoneActor {}: 플레이어 퇴장 sessionId={}", m_zoneId, msg.sessionId);
}

void ZoneActor::Handle(MsgWorld_RemovePlayer& msg)
{
    m_players.erase(msg.sessionId);
    m_sessionActors.erase(msg.sessionId);  // 브로드캐스트 라우팅 제거
}

// ─────────────────────────────────────────────────────────────────────────────
// 브로드캐스트 헬퍼
// excludeSessionId = 0 이면 전체 전송
// ─────────────────────────────────────────────────────────────────────────────
void ZoneActor::BroadcastTcp(uint64_t excludeSessionId, const std::vector<uint8_t>& packet)
{
    for (auto& [sid, weakSa] : m_sessionActors)
    {
        if (sid == excludeSessionId) continue;
        if (auto sa = weakSa.lock())
            sa->Post(MsgZone_SendTcp{ packet });
    }
}

void ZoneActor::BroadcastUdp(uint64_t excludeSessionId, const std::vector<uint8_t>& packet)
{
    for (auto& [sid, weakSa] : m_sessionActors)
    {
        if (sid == excludeSessionId) continue;
        if (auto sa = weakSa.lock())
            sa->Post(MsgZone_SendUdp{ packet });
    }
}

std::shared_ptr<SessionActor> ZoneActor::FindSessionActor(uint64_t sessionId) const
{
    auto it = m_sessionActors.find(sessionId);
    return (it != m_sessionActors.end()) ? it->second.lock() : nullptr;
}
