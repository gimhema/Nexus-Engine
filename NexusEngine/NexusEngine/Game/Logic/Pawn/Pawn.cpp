#include "Pawn.h"

std::atomic<uint64_t> Pawn::s_pawnIdCounter{ 1 };

Pawn::Pawn(std::string name)
    : m_name(std::move(name))
    , m_pawnId(s_pawnIdCounter.fetch_add(1, std::memory_order_relaxed))
{}

Pawn::Pawn(std::string name, std::weak_ptr<SessionActor> session)
    : m_name(std::move(name))
    , m_session(std::move(session))
    , m_pawnId(s_pawnIdCounter.fetch_add(1, std::memory_order_relaxed))
{}

void Pawn::SetMaxHp(int32_t maxHp)
{
    m_maxHp = (maxHp > 0) ? maxHp : 1;
    m_hp    = std::clamp(m_hp, 0, m_maxHp);
}

void Pawn::ApplyDamage(int32_t amount)
{
    if (amount <= 0 || !IsAlive()) return;
    m_hp = std::max(0, m_hp - amount);
}

void Pawn::ApplyHeal(int32_t amount)
{
    if (amount <= 0 || !IsAlive()) return;
    m_hp = std::min(m_maxHp, m_hp + amount);
}
