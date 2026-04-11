#pragma once

#include "FactionTypes.h"

#include <cstdint>
#include <unordered_map>

// ─────────────────────────────────────────────────────────────────────────────
// FactionRegistry — 진영 간 관계 테이블 (싱글턴)
//
// 역할:
//   두 진영의 관계(HOSTILE/NEUTRAL/FRIENDLY)를 등록·조회한다.
//   관계는 대칭 보장: SetRelation(A, B, ...) == SetRelation(B, A, ...)
//   미등록 쌍의 기본 반환값: NEUTRAL
//
// 사용:
//   서버 시작 시 Server::Run()에서 진영 관계를 일괄 등록.
//   전투 판정, AI 어그로, 스킬 타게팅 등에서 IsHostile()로 조회.
//
// 예시:
//   FactionRegistry::Instance().SetRelation(
//       EFactionId::ALLIANCE, EFactionId::HORDE, EFactionRelation::HOSTILE);
//
//   bool hostile = FactionRegistry::Instance().IsHostile(
//       EFactionId::ALLIANCE, EFactionId::HORDE);  // true
// ─────────────────────────────────────────────────────────────────────────────
class FactionRegistry
{
public:
    [[nodiscard]] static FactionRegistry& Instance();

    // ── 관계 등록 (대칭 자동 처리) ───────────────────────────────────────
    void SetRelation(EFactionId a, EFactionId b, EFactionRelation relation);

    // ── 관계 조회 ────────────────────────────────────────────────────────
    [[nodiscard]] EFactionRelation GetRelation(EFactionId a, EFactionId b) const;

    [[nodiscard]] bool IsHostile (EFactionId a, EFactionId b) const;
    [[nodiscard]] bool IsNeutral (EFactionId a, EFactionId b) const;
    [[nodiscard]] bool IsFriendly(EFactionId a, EFactionId b) const;

    // ── 같은 진영은 항상 우호 ────────────────────────────────────────────
    // (NONE 제외 — NONE vs NONE = NEUTRAL)
    [[nodiscard]] bool IsSameFaction(EFactionId a, EFactionId b) const;

private:
    FactionRegistry() = default;

    // (lo, hi) 순서로 정렬된 대칭 키 생성
    [[nodiscard]] static uint64_t MakeKey(EFactionId a, EFactionId b);

    std::unordered_map<uint64_t, EFactionRelation> m_relations;
    static constexpr EFactionRelation DEFAULT_RELATION = EFactionRelation::NEUTRAL;
};
