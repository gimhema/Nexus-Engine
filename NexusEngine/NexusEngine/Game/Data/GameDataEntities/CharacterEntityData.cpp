#include "CharacterEntityData.h"

// 신규 캐릭터 기본 스탯: hp=100, atk=10, def=5, speed=3
CharacterEntityData::CharacterEntityData()
    : GameDataEntityBase(100, 10, 5, 3.f)
    , m_expToNextLevel(CalcExpToNextLevel(1))
{}

CharacterEntityData::CharacterEntityData(int32_t maxHp, int32_t attack, int32_t defense,
                                          float moveSpeed, uint32_t level, uint64_t experience)
    : GameDataEntityBase(maxHp, attack, defense, moveSpeed)
    , m_level(level)
    , m_experience(experience)
    , m_expToNextLevel(CalcExpToNextLevel(level))
{}

// ─────────────────────────────────────────────────────────────────────────────
// 경험치 추가 및 레벨업 처리
// ─────────────────────────────────────────────────────────────────────────────
bool CharacterEntityData::AddExperience(uint64_t amount)
{
    m_experience += amount;
    bool leveledUp = false;

    while (m_experience >= m_expToNextLevel)
    {
        m_experience -= m_expToNextLevel;
        ++m_level;
        m_expToNextLevel = CalcExpToNextLevel(m_level);
        ApplyLevelUp();
        leveledUp = true;
    }

    return leveledUp;
}

// ─────────────────────────────────────────────────────────────────────────────
// 레벨업 시 스탯 증가: maxHp +10, attack +1, defense +1
// ─────────────────────────────────────────────────────────────────────────────
void CharacterEntityData::ApplyLevelUp()
{
    m_maxHp   += 10;
    m_attack  += 1;
    m_defense += 1;
}

// ─────────────────────────────────────────────────────────────────────────────
// 다음 레벨 필요 경험치: level^2 * 100
// lv1→2 = 100, lv2→3 = 400, lv3→4 = 900, ...
// ─────────────────────────────────────────────────────────────────────────────
uint64_t CharacterEntityData::CalcExpToNextLevel(uint32_t level)
{
    return static_cast<uint64_t>(level) * level * 100;
}
