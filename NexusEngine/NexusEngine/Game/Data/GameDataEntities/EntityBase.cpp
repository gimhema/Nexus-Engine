#include "EntityBase.h"

GameDataEntityBase::GameDataEntityBase(int32_t maxHp, int32_t attack, int32_t defense, float moveSpeed)
    : m_maxHp(maxHp)
    , m_attack(attack)
    , m_defense(defense)
    , m_moveSpeed(moveSpeed)
{}
