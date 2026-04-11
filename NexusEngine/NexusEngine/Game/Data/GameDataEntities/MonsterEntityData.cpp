#include "MonsterEntityData.h"

MonsterEntityData::MonsterEntityData(int32_t maxHp, int32_t attack, int32_t defense,
                                      float moveSpeed, EFactionId factionId,
                                      EAIType aiType, float aggroRange)
    : GameDataEntityBase(maxHp, attack, defense, moveSpeed, factionId, aiType, aggroRange)
{}
