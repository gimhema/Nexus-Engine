#pragma once

#include "SkillDef.h"

#include <unordered_map>

// ─────────────────────────────────────────────────────────────────────────────
// SkillRegistry — 스킬 정의 조회 싱글턴
//
// 사용법:
//   // 서버 시작 시 (Server::Run)
//   SkillRegistry::Instance().Register({ 1, "기본 공격", ESkillType::Melee, ... });
//
//   // ZoneActor 전투 처리 시
//   const SkillDef* def = SkillRegistry::Instance().Get(skillId);
//   if (!def) { /* 알 수 없는 스킬 */ }
// ─────────────────────────────────────────────────────────────────────────────
class SkillRegistry
{
public:
    static SkillRegistry& Instance();

    // 스킬 등록 (서버 시작 시 한 번)
    void Register(SkillDef def);

    // skillId → SkillDef 조회. 없으면 nullptr.
    [[nodiscard]] const SkillDef* Get(uint32_t skillId) const;

    [[nodiscard]] bool Exists(uint32_t skillId) const;

private:
    SkillRegistry() = default;

    std::unordered_map<uint32_t, SkillDef> m_skills;
};
