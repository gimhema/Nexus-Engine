#include "EntityBase.h"

GameDataEntityBase::GameDataEntityBase(int32_t maxHp, int32_t attack, int32_t defense,
                                       float moveSpeed, EFactionId factionId,
                                       EAIType aiType, float aggroRange)
    : m_maxHp(maxHp)
    , m_attack(attack)
    , m_defense(defense)
    , m_moveSpeed(moveSpeed)
    , m_factionId(factionId)
    , m_aiType(aiType)
    , m_aggroRange(aggroRange)
{}
