#include "PlayerPawn.h"

PlayerPawn::PlayerPawn(std::string name,
                       uint32_t    characterId,
                       std::weak_ptr<SessionActor> session)
    : Pawn(std::move(name), std::move(session), std::make_unique<CharacterEntityData>())
    , m_characterId(characterId)
{}

bool PlayerPawn::IsOnCooldown(uint32_t skillId) const
{
    auto it = m_cooldowns.find(skillId);
    if (it == m_cooldowns.end()) return false;
    return Clock::now() < it->second;
}

void PlayerPawn::SetCooldown(uint32_t skillId, uint32_t cooldownMs)
{
    m_cooldowns[skillId] = Clock::now() + std::chrono::milliseconds(cooldownMs);
}

void PlayerPawn::PickUpItem(ItemBase* pickuped)
{
    
}

void PlayerPawn::DropItem(ITEM_TYPE bagType, int pos)
{
    
}

void PlayerPawn::ItemInteraction(ITEM_TYPE bagType, int pos)
{

}


