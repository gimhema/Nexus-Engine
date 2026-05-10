#include "CombatProcessor.h"

#include "../../Logic/Pawn/Pawn.h"
#include "../../Logic/Pawn/PlayerPawn.h"
#include "../../Data/GameDataEntities/CharacterEntityData.h"

#include <algorithm>
#include <cstring>

namespace CombatProcessor
{

float DistanceSq(const Vec3& a, const Vec3& b)
{
    const float dx = a.x - b.x;
    const float dy = a.y - b.y;
    const float dz = a.z - b.z;
    return dx * dx + dy * dy + dz * dz;
}

CombatResult ProcessSingleTarget(PlayerPawn& attacker,
                                  Pawn&       target,
                                  const SkillDef& def)
{
    CombatResult result;

    // ── 1. 공격자 생존 확인 ───────────────────────────────────────────────────
    if (!attacker.IsAlive())
    {
        std::strncpy(result.failReason, "사망 상태", sizeof(result.failReason) - 1);
        return result;
    }

    // ── 2. 대상 생존 확인 ────────────────────────────────────────────────────
    if (!target.IsAlive())
    {
        std::strncpy(result.failReason, "대상이 이미 사망", sizeof(result.failReason) - 1);
        return result;
    }

    // ── 3. 쿨타임 확인 ────────────────────────────────────────────────────────
    if (attacker.IsOnCooldown(def.skillId))
    {
        std::strncpy(result.failReason, "쿨타임 중", sizeof(result.failReason) - 1);
        return result;
    }

    // ── 4. MP 확인 ────────────────────────────────────────────────────────────
    auto& charData = attacker.GetCharacterData();
    if (def.mpCost > 0 && !charData.ConsumeMp(static_cast<int32_t>(def.mpCost)))
    {
        std::strncpy(result.failReason, "MP 부족", sizeof(result.failReason) - 1);
        return result;
    }

    // ── 5. 사거리 검증 ────────────────────────────────────────────────────────
    const float rangeSq = def.range * def.range;
    if (DistanceSq(attacker.GetPos(), target.GetPos()) > rangeSq)
    {
        // MP 소모를 이미 했으면 환불
        if (def.mpCost > 0) charData.RestoreMp(static_cast<int32_t>(def.mpCost));
        std::strncpy(result.failReason, "사거리 초과", sizeof(result.failReason) - 1);
        return result;
    }

    // ── 6. 쿨타임 등록 ───────────────────────────────────────────────────────
    attacker.SetCooldown(def.skillId, def.cooldownMs);

    // ── 7. 데미지 계산: max(1, attack × damageMult - defense) ────────────────
    const int32_t rawDmg  = static_cast<int32_t>(
        static_cast<float>(attacker.GetAttack()) * def.damageMult);
    const int32_t damage  = std::max(1, rawDmg - target.GetDefense());

    // ── 8. 데미지 적용 ────────────────────────────────────────────────────────
    target.ApplyDamage(damage);

    result.success        = true;
    result.damage         = damage;
    result.targetRemainHp = static_cast<uint32_t>(std::max(0, target.GetHp()));
    result.targetDied     = !target.IsAlive();
    return result;
}

} // namespace CombatProcessor
