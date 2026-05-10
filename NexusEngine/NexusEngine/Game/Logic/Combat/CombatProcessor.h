#pragma once

#include "../../Data/Skill/SkillDef.h"
#include "../../Messages/GameMessages.h"  // Vec3

#include <cstdint>

class Pawn;
class PlayerPawn;

// ─────────────────────────────────────────────────────────────────────────────
// CombatProcessor — 전투 판정 + 데미지 계산
//
// ZoneActor 전용 스레드에서만 호출. 상태 없는 순수 함수 모음.
//
// 판정 순서:
//   1. 스킬 유효성 검증 (쿨타임, MP, 생존 여부)
//   2. 사거리 검증 (공격자↔대상 거리 ≤ range)
//   3. 데미지 계산: max(1, attack × damageMult - defense)
//   4. 적용 및 결과 반환
// ─────────────────────────────────────────────────────────────────────────────

struct CombatResult
{
    bool     success{ false };
    int32_t  damage{ 0 };
    uint32_t targetRemainHp{ 0 };
    bool     targetDied{ false };
    char     failReason[64]{};  // 실패 시 사유 (동적 할당 회피)
};

namespace CombatProcessor
{
    // 단일 대상 스킬 처리 (Melee / Ranged)
    // attacker: 공격자 PlayerPawn
    // target:   피격 대상 Pawn (플레이어 또는 NPC/몬스터)
    // def:      사용 스킬 정의
    CombatResult ProcessSingleTarget(PlayerPawn& attacker,
                                     Pawn&       target,
                                     const SkillDef& def);

    // 두 Vec3 사이의 거리² 계산 (sqrt 회피)
    float DistanceSq(const Vec3& a, const Vec3& b);
}
