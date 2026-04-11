#include "NpcEntityData.h"

NpcEntityData::NpcEntityData(int32_t maxHp, int32_t attack, int32_t defense,
                              float moveSpeed, EFactionId factionId,
                              EAIType aiType, float aggroRange, bool isImmortal)
    : GameDataEntityBase(maxHp, attack, defense, moveSpeed, factionId, aiType, aggroRange)
    , m_isImmortal(isImmortal)
{}
