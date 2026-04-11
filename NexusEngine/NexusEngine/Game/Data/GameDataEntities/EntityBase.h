#pragma once

#include <cstdint>

namespace EEntity
{
    enum EID : uint8_t
    {
        UNKNOWN = 0,
        PLAYER  = 1,   // 플레이어 캐릭터
        NPC     = 2,   // NPC
        MONSTER = 3,   // 몬스터/크리처
        ITEM    = 4,   // 아이템
    };
}

// ─────────────────────────────────────────────────────────────────────────────
// GameDataEntityBase — Pawn이 소유하는 인게임 데이터 컴포넌트 (캐릭터 시트)
//
// 역할:
//   Pawn의 정적 스탯(maxHp, attack, defense, moveSpeed)을 보관.
//   서브클래스로 타입별 추가 데이터 확장.
//
// 소유권:
//   Pawn이 unique_ptr<GameDataEntityBase>로 독점 소유.
//   Pawn 생성 시 m_hp를 maxHp로 초기화.
//
// 서브클래스:
//   CharacterEntityData  — 플레이어 (level, exp, 향후 인벤토리/장비)
//   NpcEntityData        — NPC (향후 AI 타입, 대화 트리)
//   MonsterEntityData    — 몬스터 (향후 드롭테이블, 어그로 범위)
// ─────────────────────────────────────────────────────────────────────────────
class GameDataEntityBase
{
public:
    GameDataEntityBase(int32_t maxHp, int32_t attack, int32_t defense, float moveSpeed);
    virtual ~GameDataEntityBase() = default;

    GameDataEntityBase(const GameDataEntityBase&)            = delete;
    GameDataEntityBase& operator=(const GameDataEntityBase&) = delete;

    [[nodiscard]] virtual EEntity::EID GetEntityType() const = 0;

    // ── 스탯 조회 ─────────────────────────────────────────────────────────
    [[nodiscard]] int32_t GetMaxHp()     const { return m_maxHp; }
    [[nodiscard]] int32_t GetAttack()    const { return m_attack; }
    [[nodiscard]] int32_t GetDefense()   const { return m_defense; }
    [[nodiscard]] float   GetMoveSpeed() const { return m_moveSpeed; }

    // ── 스탯 설정 (장비/버프 등에 의한 변화) ────────────────────────────
    void SetMaxHp(int32_t v)    { m_maxHp    = v; }
    void SetAttack(int32_t v)   { m_attack   = v; }
    void SetDefense(int32_t v)  { m_defense  = v; }
    void SetMoveSpeed(float v)  { m_moveSpeed = v; }

protected:
    int32_t m_maxHp{ 100 };
    int32_t m_attack{ 10 };
    int32_t m_defense{ 5 };
    float   m_moveSpeed{ 3.f };
};
