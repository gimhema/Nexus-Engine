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

void WorldActor::OnMessage(WorldMessage& msg)
{
    std::visit(overloaded{
        [this](MsgSession_Login& m)            { Handle(m); },
        [this](MsgSession_EnterWorld& m)       { Handle(m); },
        [this](MsgSession_Logout& m)           { Handle(m); },
        [this](MsgZone_TeleportRequest& m)     { Handle(m); },
        [this](MsgServer_RegisterSession& m)   { Handle(m); },
        [this](MsgServer_UnregisterSession& m) { Handle(m); },
    }, msg);
}

// ─────────────────────────────────────────────────────────────────────────────
void WorldActor::Handle(MsgServer_RegisterSession& msg)
{
    m_sessions[msg.sessionId] = std::move(msg.actor);
}

void WorldActor::Handle(MsgServer_UnregisterSession& msg)
{
    // 연결이 갑자기 끊긴 경우 User가 남아있을 수 있으므로 함께 정리
    m_users.erase(msg.sessionId);
    m_sessions.erase(msg.sessionId);
}

// ─────────────────────────────────────────────────────────────────────────────
void WorldActor::Handle(MsgSession_Login& msg)
{
    // TODO: AuthDB 검증 (Phase 4) — 현재는 항상 성공 처리
    LOG_INFO("WorldActor: 로그인 sessionId={} account={}", msg.sessionId, msg.accountName);

    auto sa = FindSession(msg.sessionId);
    if (!sa)
    {
        LOG_WARN("WorldActor: SessionActor 없음 sessionId={}", msg.sessionId);
        return;
    }

    // User 생성 및 인증 상태 설정
    User user(msg.sessionId);
    user.SetIdentification(msg.accountName, msg.token);
    user.SetProfile({ msg.accountName });   // TODO: DB에서 캐릭터 이름 로드 (Phase 4)
    user.SetStatus(EUser::AUTHENTICATED);
    m_users.emplace(msg.sessionId, std::move(user));

    MsgWorld_LoginResult result;
    result.success = true;
    result.message = "OK";
    sa->Post(std::move(result));
}

void WorldActor::Handle(MsgSession_EnterWorld& msg)
{
    LOG_INFO("WorldActor: 월드 진입 sessionId={} charId={}", msg.sessionId, msg.characterId);

    // 인증되지 않은 세션 차단
    User* user = FindUser(msg.sessionId);
    if (!user || !user->IsAuthenticated())
    {
        LOG_WARN("WorldActor: 미인증 세션의 월드 진입 시도 sessionId={}", msg.sessionId);
        return;
    }

    // TODO: CharacterDB에서 캐릭터 로드 (Phase 4)
    // 임시: 기본 존(ID=1)에 스폰
    constexpr uint32_t DEFAULT_ZONE = 1;
    ZoneActor* zone = FindZone(DEFAULT_ZONE);
    if (!zone)
    {
        LOG_ERROR("WorldActor: 기본 Zone 없음 zoneId={}", DEFAULT_ZONE);
        return;
    }

    auto sa = FindSession(msg.sessionId);
    if (!sa) return;

    sa->SetZone(zone);

    // RegisterSessionActor는 ZoneActor 스레드 내에서 처리
    // MsgWorld_AddPlayer에 weak_ptr을 실어 보내 ZoneActor가 직접 등록
    MsgWorld_AddPlayer add;
    add.sessionId     = msg.sessionId;
    add.characterId   = msg.characterId;
    add.characterName = "Player";       // TODO: DB에서 로드
    add.spawnPos      = { 0.f, 0.f, 0.f };
    add.sessionActor  = sa;             // weak_ptr — ZoneActor가 Handle 내에서 등록
    zone->Post(std::move(add));
}

void WorldActor::Handle(MsgSession_Logout& msg)
{
    LOG_INFO("WorldActor: 로그아웃 sessionId={}", msg.sessionId);

    // User 상태를 WAIT_DISCONNECT로 전환 후 제거
    if (User* user = FindUser(msg.sessionId))
    {
        user->SetStatus(EUser::WAIT_DISCONNECT);
        LOG_INFO("WorldActor: User 제거 account={} sessionId={}",
                 user->GetAccountId(), msg.sessionId);
    }
    m_users.erase(msg.sessionId);

    if (auto sa = FindSession(msg.sessionId))
        sa->ClearZone();

    // 모든 Zone에 플레이어 제거 요청
    MsgWorld_RemovePlayer remove{ msg.sessionId };
    for (auto& [id, zone] : m_zones)
        zone->Post(remove);

    m_sessions.erase(msg.sessionId);
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

    auto sa = FindSession(msg.sessionId);
    if (!sa) return;

    // 기존 모든 존에서 제거
    MsgWorld_RemovePlayer remove{ msg.sessionId };
    for (auto& [id, zone] : m_zones)
        zone->Post(remove);

    sa->SetZone(targetZone);

    MsgWorld_AddPlayer add;
    add.sessionId    = msg.sessionId;
    add.spawnPos     = msg.targetPos;
    add.sessionActor = sa;             // weak_ptr — ZoneActor가 Handle 내에서 등록
    targetZone->Post(std::move(add));
}

// ─────────────────────────────────────────────────────────────────────────────
User* WorldActor::FindUser(uint64_t sessionId)
{
    auto it = m_users.find(sessionId);
    return (it != m_users.end()) ? &it->second : nullptr;
}

ZoneActor* WorldActor::FindZone(uint32_t zoneId) const
{
    auto it = m_zones.find(zoneId);
    return (it != m_zones.end()) ? it->second.get() : nullptr;
}

std::shared_ptr<SessionActor> WorldActor::FindSession(uint64_t sessionId) const
{
    auto it = m_sessions.find(sessionId);
    return (it != m_sessions.end()) ? it->second : nullptr;
}
