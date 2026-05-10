#pragma once

#include <cstdint>
#include <string>

// ─────────────────────────────────────────────────────────────────────────────
// ESkillType — 스킬 판정 방식 분류
// ─────────────────────────────────────────────────────────────────────────────
enum class ESkillType : uint8_t
{
    Melee  = 0,  // 근거리 단일 대상 — 시전자와 대상의 거리 검증
    Ranged = 1,  // 원거리 단일 대상 — 시전자와 대상의 거리 검증
    AoE    = 2,  // 범위 공격 — 지정 좌표 기준 aoeRadius 내 모든 대상
};

// ─────────────────────────────────────────────────────────────────────────────
// SkillDef — 스킬 정적 데이터 (불변)
//
// 런타임에는 SkillRegistry::Get(skillId)로 조회.
// ZoneActor 전투 처리 시 판정·비용·배율의 기준이 됨.
// ─────────────────────────────────────────────────────────────────────────────
struct SkillDef
{
    uint32_t    skillId;
    std::string name;

    ESkillType  type;

    float       range;       // 최대 사거리 (AoE면 시전 중심점까지 거리)
    float       aoeRadius;   // AoE 판정 반경 (type != AoE 이면 무시)

    uint32_t    cooldownMs;  // 쿨타임 (밀리초)
    uint32_t    mpCost;      // MP 소모량

    float       damageMult;  // 데미지 배율 — 최종 데미지 = (공격력 × damageMult) - 방어력
};
