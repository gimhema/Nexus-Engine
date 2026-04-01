#include "WorldActor.h"
#include "SessionActor.h"
#include "ZoneActor.h"
#include "../../Shared/Logger.h"

WorldActor::WorldActor() = default;

void WorldActor::OnStart()
{
    LOG_INFO("WorldActor 시작");
}

void WorldActor::OnStop()
{
    LOG_INFO("WorldActor 종료");
}

void WorldActor::RegisterZone(uint32_t zoneId, std::shared_ptr<ZoneActor> zone)
{
    m_zones[zoneId] = std::move(zone);
    LOG_INFO("Zone 등록: zoneId={}", zoneId);
}

void WorldActor::RegisterSession(uint64_t sessionId, std::shared_ptr<SessionActor> actor)
{
    m_sessions[sessionId] = std::move(actor);
}

void WorldActor::UnregisterSession(uint64_t sessionId)
{
    m_sessions.erase(sessionId);
}

void WorldActor::OnMessage(WorldMessage& msg)
{
    std::visit(overloaded{
        [this](MsgSession_Login& m)         { Handle(m); },
        [this](MsgSession_EnterWorld& m)    { Handle(m); },
        [this](MsgSession_Logout& m)        { Handle(m); },
        [this](MsgZone_TeleportRequest& m)  { Handle(m); },
    }, msg);
}

// ─────────────────────────────────────────────────────────────────────────────
void WorldActor::Handle(MsgSession_Login& msg)
{
    // TODO: AuthDB 검증 (Phase 4)
    // 현재는 항상 성공 처리
    LOG_INFO("WorldActor: 로그인 sessionId={} account={}", msg.sessionId, msg.accountName);

    SessionActor* sa = FindSession(msg.sessionId);
    if (!sa)
    {
        LOG_WARN("WorldActor: SessionActor 없음 sessionId={}", msg.sessionId);
        return;
    }

    MsgWorld_LoginResult result;
    result.success = true;
    result.message = "OK";
    sa->Post(std::move(result));
}

void WorldActor::Handle(MsgSession_EnterWorld& msg)
{
    LOG_INFO("WorldActor: 월드 진입 sessionId={} charId={}", msg.sessionId, msg.characterId);

    // TODO: CharacterDB에서 캐릭터 로드 (Phase 4)
    // 임시: 기본 존(ID=1)에 스폰
    constexpr uint32_t DEFAULT_ZONE = 1;
    ZoneActor* zone = FindZone(DEFAULT_ZONE);
    if (!zone)
    {
        LOG_ERROR("WorldActor: 기본 Zone 없음 zoneId={}", DEFAULT_ZONE);
        return;
    }

    SessionActor* sa = FindSession(msg.sessionId);
    if (!sa) return;

    sa->SetZone(zone);

    MsgWorld_AddPlayer add;
    add.sessionId     = msg.sessionId;
    add.characterId   = msg.characterId;
    add.characterName = "Player";       // TODO: DB에서 로드
    add.spawnPos      = { 0.f, 0.f, 0.f };
    zone->Post(std::move(add));
}

void WorldActor::Handle(MsgSession_Logout& msg)
{
    LOG_INFO("WorldActor: 로그아웃 sessionId={}", msg.sessionId);

    SessionActor* sa = FindSession(msg.sessionId);
    if (sa)
        sa->ClearZone();

    // 모든 Zone에 플레이어 제거 요청
    MsgWorld_RemovePlayer remove{ msg.sessionId };
    for (auto& [id, zone] : m_zones)
        zone->Post(remove);

    UnregisterSession(msg.sessionId);
}

void WorldActor::Handle(MsgZone_TeleportRequest& msg)
{
    LOG_INFO("WorldActor: 텔레포트 sessionId={} targetZone={}", msg.sessionId, msg.targetZoneId);

    ZoneActor* targetZone = FindZone(msg.targetZoneId);
    if (!targetZone)
    {
        LOG_WARN("WorldActor: 대상 Zone 없음 zoneId={}", msg.targetZoneId);
        return;
    }

    SessionActor* sa = FindSession(msg.sessionId);
    if (!sa) return;

    sa->SetZone(targetZone);

    MsgWorld_AddPlayer add;
    add.sessionId   = msg.sessionId;
    add.spawnPos    = msg.targetPos;
    targetZone->Post(std::move(add));
}

// ─────────────────────────────────────────────────────────────────────────────
ZoneActor* WorldActor::FindZone(uint32_t zoneId) const
{
    auto it = m_zones.find(zoneId);
    return (it != m_zones.end()) ? it->second.get() : nullptr;
}

SessionActor* WorldActor::FindSession(uint64_t sessionId) const
{
    auto it = m_sessions.find(sessionId);
    return (it != m_sessions.end()) ? it->second.get() : nullptr;
}
