#pragma once

#include "EntityBase.h"

// ─────────────────────────────────────────────────────────────────────────────
// NpcEntityData — NPC 전용 인게임 데이터
//
// 향후 추가 예정:
//   - AI 타입 (EAIType enum)
//   - 대화 트리 ID
//   - 퀘스트 연동 ID
// ─────────────────────────────────────────────────────────────────────────────
class NpcEntityData : public GameDataEntityBase
{
public:
    NpcEntityData(int32_t maxHp, int32_t attack, int32_t defense, float moveSpeed);

    EEntity::EID GetEntityType() const override { return EEntity::EID::NPC; }
};
