#include "SkillRegistry.h"

SkillRegistry& SkillRegistry::Instance()
{
    static SkillRegistry instance;
    return instance;
}

void SkillRegistry::Register(SkillDef def)
{
    m_skills.emplace(def.skillId, std::move(def));
}

const SkillDef* SkillRegistry::Get(uint32_t skillId) const
{
    auto it = m_skills.find(skillId);
    return (it != m_skills.end()) ? &it->second : nullptr;
}

bool SkillRegistry::Exists(uint32_t skillId) const
{
    return m_skills.count(skillId) > 0;
}
