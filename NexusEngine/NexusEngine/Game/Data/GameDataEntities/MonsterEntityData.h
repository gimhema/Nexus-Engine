#pragma once

#include "EntityBase.h"

// ─────────────────────────────────────────────────────────────────────────────
// MonsterEntityData — 몬스터/크리처 전용 인게임 데이터
//
// 기본값:
//   factionId  = CREATURE_WILD (야생 몬스터 기본 진영)
//   aiType     = AGGRESSIVE    (범위 내 적대 진영 자동 공격)
//   aggroRange = 5.f
//
// 향후 추가 예정:
//   expReward   : uint32_t — 처치 시 경험치 보상
//   dropTableId : uint32_t — 드롭테이블 ID
//   respawnTime : float    — 리스폰 대기 시간 (초)
// ─────────────────────────────────────────────────────────────────────────────
class MonsterEntityData : public GameDataEntityBase
{
public:
    MonsterEntityData(int32_t    maxHp,
                      int32_t    attack,
                      int32_t    defense,
                      float      moveSpeed,
                      EFactionId factionId  = EFactionId::CREATURE_WILD,
                      EAIType    aiType     = EAIType::AGGRESSIVE,
                      float      aggroRange = 5.f);

    EEntity::EID GetEntityType() const override { return EEntity::EID::MONSTER; }
};
