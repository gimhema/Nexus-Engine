#pragma once

#include "EntityBase.h"

// ─────────────────────────────────────────────────────────────────────────────
// NpcEntityData — NPC 전용 인게임 데이터
//
// 기본값:
//   factionId  = NONE      (서버에서 명시적으로 지정)
//   aiType     = DEFENSIVE (공격받으면 반격, 먼저 공격하지 않음)
//   isImmortal = true      (NPC는 기본적으로 불사)
//
// 향후 추가 예정:
//   dialogueId : uint32_t  — 대화 트리 ID
//   shopId     : uint32_t  — 상점 ID (0 = 상점 없음)
//   questIds   : vector    — 제공 퀘스트 목록
// ─────────────────────────────────────────────────────────────────────────────
class NpcEntityData : public GameDataEntityBase
{
public:
    NpcEntityData(int32_t    maxHp,
                  int32_t    attack,
                  int32_t    defense,
                  float      moveSpeed,
                  EFactionId factionId  = EFactionId::NONE,
                  EAIType    aiType     = EAIType::DEFENSIVE,
                  float      aggroRange = 0.f,
                  bool       isImmortal = true);

    EEntity::EID GetEntityType() const override { return EEntity::EID::NPC; }

    [[nodiscard]] bool IsImmortal() const { return m_isImmortal; }
    void SetImmortal(bool v) { m_isImmortal = v; }

private:
    bool m_isImmortal{ true };
};
