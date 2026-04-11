#pragma once

#include <algorithm>
#include <atomic>
#include <cstdint>
#include <memory>
#include <string>

#include "../../Messages/GameMessages.h"                    // Vec3
#include "../../Data/GameDataEntities/EntityBase.h"         // GameDataEntityBase

class SessionActor;

// ─────────────────────────────────────────────────────────────────────────────
// Pawn — 게임 내 모든 상호작용 존재의 슈퍼클래스
//
// 역할 분리:
//   Pawn            — 런타임 상태 (현재 hp, 위치, 방향, 세션)
//   GameDataEntityBase — 정적 데이터 (maxHp, attack, defense, moveSpeed 등)
//
// Playable     (플레이어블) : m_session이 유효한 SessionActor를 가리킴.
// Non-Playable (논플레이어블): m_session 없음. NPC, 몬스터 등.
//
// 소유권:
//   ZoneActor가 unique_ptr<Pawn>으로 소유.
//   ZoneActor 전용 스레드 내에서만 Pawn 상태를 직접 읽고 쓴다.
// ─────────────────────────────────────────────────────────────────────────────
class Pawn
{
public:
    // 논플레이어블 (NPC/Monster) 생성
    Pawn(std::string name, std::unique_ptr<GameDataEntityBase> data);

    // 플레이어블 생성 (세션 보유)
    Pawn(std::string name, std::weak_ptr<SessionActor> session,
         std::unique_ptr<GameDataEntityBase> data);

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

    // ── 체력 (런타임 상태) ────────────────────────────────────────────────
    [[nodiscard]] int32_t GetHp()    const { return m_hp; }
    [[nodiscard]] bool    IsAlive()  const { return m_hp > 0; }
    void SetHp(int32_t hp) { m_hp = std::clamp(hp, 0, GetMaxHp()); }

    // ── 스탯 조회 (EntityData 위임) ───────────────────────────────────────
    [[nodiscard]] int32_t GetMaxHp()     const { return m_data ? m_data->GetMaxHp()     : 0; }
    [[nodiscard]] int32_t GetAttack()    const { return m_data ? m_data->GetAttack()    : 0; }
    [[nodiscard]] int32_t GetDefense()   const { return m_data ? m_data->GetDefense()   : 0; }
    [[nodiscard]] float   GetMoveSpeed() const { return m_data ? m_data->GetMoveSpeed() : 0.f; }

    // ── 인게임 데이터 컴포넌트 접근 ───────────────────────────────────────
    [[nodiscard]] GameDataEntityBase*       GetEntityData()       { return m_data.get(); }
    [[nodiscard]] const GameDataEntityBase* GetEntityData() const { return m_data.get(); }

    // ── 세션 (플레이어블 판별) ────────────────────────────────────────────
    void SetSession(std::weak_ptr<SessionActor> session) { m_session = std::move(session); }
    void ClearSession()                                   { m_session.reset(); }

    [[nodiscard]] bool IsPlayable() const { return !m_session.expired(); }
    [[nodiscard]] std::shared_ptr<SessionActor> GetSession() const { return m_session.lock(); }

    // ── ZoneActor tick에서 호출 ───────────────────────────────────────────
    virtual void OnTick(uint64_t /*tickCount*/) {}

    // ── 생명주기 훅 ──────────────────────────────────────────────────────
    virtual void OnSpawn()   {}
    virtual void OnDespawn() {}

    // ── 피해 / 회복 ──────────────────────────────────────────────────────
    virtual void ApplyDamage(int32_t amount);
    virtual void ApplyHeal(int32_t amount);

protected:
    std::string m_name;
    Vec3        m_pos;
    float       m_orientation{ 0.f };

    std::unique_ptr<GameDataEntityBase>  m_data;    // m_hp 초기화 전에 선언 필수
    int32_t m_hp{ 0 };   // 현재 체력 — 생성자에서 m_data->GetMaxHp()로 초기화

    std::weak_ptr<SessionActor>          m_session;

private:
    uint64_t                         m_pawnId;
    static std::atomic<uint64_t>     s_pawnIdCounter;
};
