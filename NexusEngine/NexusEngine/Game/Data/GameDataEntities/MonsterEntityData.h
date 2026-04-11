#pragma once

#include "EntityBase.h"

// ─────────────────────────────────────────────────────────────────────────────
// MonsterEntityData — 몬스터/크리처 전용 인게임 데이터
//
// 향후 추가 예정:
//   - 드롭테이블 ID
//   - 어그로 범위 (aggroRange)
//   - 경험치 보상량 (expReward)
// ─────────────────────────────────────────────────────────────────────────────
class MonsterEntityData : public GameDataEntityBase
{
public:
    MonsterEntityData(int32_t maxHp, int32_t attack, int32_t defense, float moveSpeed);

    EEntity::EID GetEntityType() const override { return EEntity::EID::MONSTER; }
};
