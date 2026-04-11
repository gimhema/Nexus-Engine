#include "FactionRegistry.h"

#include <algorithm>

FactionRegistry& FactionRegistry::Instance()
{
    static FactionRegistry instance;
    return instance;
}

// ─────────────────────────────────────────────────────────────────────────────
// 관계 등록 — 대칭 처리: (A,B) 키 하나만 저장
// ─────────────────────────────────────────────────────────────────────────────
void FactionRegistry::SetRelation(EFactionId a, EFactionId b, EFactionRelation relation)
{
    if (a == b) return;   // 같은 진영은 IsSameFaction()으로 처리
    m_relations[MakeKey(a, b)] = relation;
}

// ─────────────────────────────────────────────────────────────────────────────
// 관계 조회
// ─────────────────────────────────────────────────────────────────────────────
EFactionRelation FactionRegistry::GetRelation(EFactionId a, EFactionId b) const
{
    if (IsSameFaction(a, b)) return EFactionRelation::FRIENDLY;

    const auto it = m_relations.find(MakeKey(a, b));
    return (it != m_relations.end()) ? it->second : DEFAULT_RELATION;
}

bool FactionRegistry::IsHostile(EFactionId a, EFactionId b) const
{
    return GetRelation(a, b) == EFactionRelation::HOSTILE;
}

bool FactionRegistry::IsNeutral(EFactionId a, EFactionId b) const
{
    return GetRelation(a, b) == EFactionRelation::NEUTRAL;
}

bool FactionRegistry::IsFriendly(EFactionId a, EFactionId b) const
{
    return GetRelation(a, b) == EFactionRelation::FRIENDLY;
}

bool FactionRegistry::IsSameFaction(EFactionId a, EFactionId b) const
{
    return (a == b) && (a != EFactionId::NONE);
}

// ─────────────────────────────────────────────────────────────────────────────
// 대칭 키: 두 진영 ID를 (lo, hi) 순으로 정렬해 uint64_t로 합성
// ─────────────────────────────────────────────────────────────────────────────
uint64_t FactionRegistry::MakeKey(EFactionId a, EFactionId b)
{
    const uint32_t lo = std::min(static_cast<uint32_t>(a), static_cast<uint32_t>(b));
    const uint32_t hi = std::max(static_cast<uint32_t>(a), static_cast<uint32_t>(b));
    return (static_cast<uint64_t>(hi) << 32) | lo;
}
