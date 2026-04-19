#pragma once

#include "../../Actor/Actor.h"
#include "../../Game/Messages/GameMessages.h"

#include "../User/User.h"

#include <functional>
#include <unordered_map>
#include <memory>
#include <cstdint>

class SessionActor;
class ZoneActor;

// ─────────────────────────────────────────────────────────────────────────────
// WorldActor
//
// 글로벌 싱글턴 Dedicated Actor.
// - SessionActor로부터 로그인/월드진입/로그아웃 메시지 수신
// - Zone 레지스트리 관리 (ZoneActor 생성/조회)
// - 플레이어를 적절한 ZoneActor로 라우팅
// - 존 간 텔레포트 중개
// ─────────────────────────────────────────────────────────────────────────────
class WorldActor : public Actor<WorldMessage>
{
public:
    WorldActor();
    ~WorldActor() override = default;

    // Zone 등록 — 서버 시작 시 호출 (단일 스레드 초기화 구간에서만 사용)
    void RegisterZone(uint32_t zoneId, std::shared_ptr<ZoneActor> zone);

    // 플레이어 입장/퇴장 이벤트 콜백 — Arbiter 연동용
    // WorldActor 전용 스레드에서 호출되므로 콜백은 스레드 안전하게 구현해야 함
    void SetOnPlayerEntered(std::function<void(uint64_t, const std::string&)> fn)
    { m_onPlayerEntered = std::move(fn); }
    void SetOnPlayerLeft(std::function<void(uint64_t)> fn)
    { m_onPlayerLeft = std::move(fn); }

protected:
    void OnMessage(WorldMessage& msg) override;
    void OnStart()                    override;
    void OnStop()                     override;

private:
    void Handle(MsgSession_Login& msg);
    void Handle(MsgSession_EnterWorld& msg);
    void Handle(MsgSession_Logout& msg);
    void Handle(MsgSession_CharSetup& msg);
    void Handle(MsgSession_WorldChat& msg);
    void Handle(MsgZone_TeleportRequest& msg);
    void Handle(MsgServer_RegisterSession& msg);
    void Handle(MsgServer_UnregisterSession& msg);

    [[nodiscard]] ZoneActor*                    FindZone(uint32_t zoneId) const;
    [[nodiscard]] std::shared_ptr<SessionActor> FindSession(uint64_t sessionId) const;

    [[nodiscard]] User* FindUser(uint64_t sessionId);

    std::unordered_map<uint32_t, std::shared_ptr<ZoneActor>>    m_zones;
    std::unordered_map<uint64_t, std::shared_ptr<SessionActor>> m_sessions;
    std::unordered_map<uint64_t, User>                          m_users;

    // 임시 characterId 발급 카운터
    // Phase 4: DB INSERT 후 반환되는 auto-increment ID로 교체
    uint32_t m_nextCharacterId{ 1 };

    std::function<void(uint64_t, const std::string&)> m_onPlayerEntered;
    std::function<void(uint64_t)>                     m_onPlayerLeft;
};
