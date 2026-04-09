#pragma once

#include <algorithm>
#include <atomic>
#include <cstdint>
#include <memory>
#include <string>

#include "../../Messages/GameMessages.h"  // Vec3

class SessionActor;

// ─────────────────────────────────────────────────────────────────────────────
// Pawn — 게임 내 모든 상호작용 존재의 슈퍼클래스
//
// Playable     (플레이어블)  : m_session이 유효한 SessionActor를 가리킴.
//                              클라이언트가 조종하는 캐릭터.
// Non-Playable (논플레이어블): m_session 없음.
//                              NPC, 몬스터, 소환수, 상호작용 오브젝트 등.
//
// 소유권:
//   ZoneActor가 unique_ptr<Pawn>으로 소유한다.
//   ZoneActor 전용 스레드 내에서만 Pawn 상태를 직접 읽고 쓴다.
//   외부 Actor는 ZoneActor 메시지를 통해 간접 요청해야 한다.
//
// 서브클래싱 예:
//   class PlayerPawn : public Pawn { ... };   // 플레이어 전용 스탯/인벤토리
//   class Creature   : public Pawn { ... };   // AI 상태 머신, 룻테이블
//   class NPC        : public Pawn { ... };   // 대화 트리, 퀘스트 제공
// ─────────────────────────────────────────────────────────────────────────────
class Pawn
{
public:
    // 논플레이어블 (NPC/Monster) 생성
    explicit Pawn(std::string name);

    // 플레이어블 생성 (세션 보유)
    Pawn(std::string name, std::weak_ptr<SessionActor> session);

    virtual ~Pawn() = default;

    Pawn(const Pawn&)            = delete;
    Pawn& operator=(const Pawn&) = delete;

    // ── 식별 ─────────────────────────────────────────────────────────────
    [[nodiscard]] uint64_t           GetPawnId() const { return m_pawnId; }
    [[nodiscard]] const std::string& GetName()   const { return m_name; }
    void SetName(std::string name) { m_name = std::move(name); }

    // ── 위치 ─────────────────────────────────────────────────────────────
    [[nodiscard]] const Vec3& GetPos()         const { return m_pos; }
    [[nodiscard]] float       GetOrientation() const { return m_orientation; }
    void SetPos(const Vec3& pos)           { m_pos = pos; }
    void SetOrientation(float orientation) { m_orientation = orientation; }

    // ── 체력 ─────────────────────────────────────────────────────────────
    [[nodiscard]] int32_t GetHp()    const { return m_hp; }
    [[nodiscard]] int32_t GetMaxHp() const { return m_maxHp; }
    [[nodiscard]] bool    IsAlive()  const { return m_hp > 0; }

    void SetHp(int32_t hp)     { m_hp = std::clamp(hp, 0, m_maxHp); }
    void SetMaxHp(int32_t maxHp);

    // ── 세션 (플레이어블 판별) ────────────────────────────────────────────
    // SetSession → 논플레이어블을 플레이어블로 전환 (이미 있으면 교체)
    void SetSession(std::weak_ptr<SessionActor> session) { m_session = std::move(session); }
    void ClearSession()                                   { m_session.reset(); }

    [[nodiscard]] bool IsPlayable() const { return !m_session.expired(); }
    [[nodiscard]] std::shared_ptr<SessionActor> GetSession() const { return m_session.lock(); }

    // ── ZoneActor tick에서 호출 ───────────────────────────────────────────
    // tickCount: ZoneActor의 누적 tick 번호. 시간 기반 로직에 활용.
    // 기본 구현은 아무 일도 하지 않음 — 서브클래스에서 오버라이드.
    virtual void OnTick(uint64_t tickCount) {}

    // ── 생명주기 훅 ──────────────────────────────────────────────────────
    // OnSpawn  : ZoneActor가 Pawn을 존에 배치할 때 호출
    // OnDespawn: ZoneActor가 Pawn을 존에서 제거할 때 호출
    virtual void OnSpawn()   {}
    virtual void OnDespawn() {}

    // ── 피해 / 회복 ──────────────────────────────────────────────────────
    // 기본 구현은 HP만 변경.
    // 서브클래스에서 오버라이드해 스탯 계산, 방어력, 이벤트 트리거 등 연동.
    virtual void ApplyDamage(int32_t amount);
    virtual void ApplyHeal(int32_t amount);

protected:
    std::string m_name;
    Vec3        m_pos;
    float       m_orientation{ 0.f };

    int32_t m_hp{ 100 };
    int32_t m_maxHp{ 100 };

    // 유효하면 플레이어블, expired이면 논플레이어블 (NPC/몬스터)
    std::weak_ptr<SessionActor> m_session;

private:
    uint64_t                         m_pawnId;
    static std::atomic<uint64_t>     s_pawnIdCounter;
};
