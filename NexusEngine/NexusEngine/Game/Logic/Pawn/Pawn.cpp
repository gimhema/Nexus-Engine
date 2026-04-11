#include "Pawn.h"

std::atomic<uint64_t> Pawn::s_pawnIdCounter{ 0 };

Pawn::Pawn(std::string name, std::unique_ptr<GameDataEntityBase> data)
    : m_name(std::move(name))
    , m_data(std::move(data))
    , m_hp(m_data ? m_data->GetMaxHp() : 0)
    , m_pawnId(s_pawnIdCounter.fetch_add(1, std::memory_order_relaxed))
{}

Pawn::Pawn(std::string name, std::weak_ptr<SessionActor> session,
           std::unique_ptr<GameDataEntityBase> data)
    : m_name(std::move(name))
    , m_data(std::move(data))
    , m_hp(m_data ? m_data->GetMaxHp() : 0)
    , m_session(std::move(session))
    , m_pawnId(s_pawnIdCounter.fetch_add(1, std::memory_order_relaxed))
{}

void Pawn::ApplyDamage(int32_t amount)
{
    if (amount <= 0 || !IsAlive()) return;
    SetHp(m_hp - amount);
}

void Pawn::ApplyHeal(int32_t amount)
{
    if (amount <= 0 || !IsAlive()) return;
    SetHp(m_hp + amount);
}
