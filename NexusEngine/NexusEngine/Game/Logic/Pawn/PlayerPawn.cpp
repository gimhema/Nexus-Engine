#include "PlayerPawn.h"

PlayerPawn::PlayerPawn(std::string name,
                       uint32_t    characterId,
                       std::weak_ptr<SessionActor> session)
    : Pawn(std::move(name), std::move(session), std::make_unique<CharacterEntityData>())
    , m_characterId(characterId)
    , m_itemComponent(std::make_unique<ItemComponent>())
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
    m_itemComponent->Store(pickuped);
}

void PlayerPawn::DropItem(ITEM_TYPE bagType, int pos)
{
    m_itemComponent->DropItem(bagType, pos);
}

void PlayerPawn::ItemBagInteraction(ITEM_TYPE bagType, int pos)
{
    switch(bagType)
    {
        case ITEM_TYPE::DEFAULT:
            return;
        case ITEM_TYPE::EQUIPMENT:
            m_itemComponent->SwapEquip(pos);
            break;
        case ITEM_TYPE::SKIN:
            m_itemComponent->SwapSkin(pos);
            break;
        case ITEM_TYPE::CONSUMABLE:
            m_itemComponent->UseConsumbale(pos);
            break;
    }
}


SkinSwapResult PlayerPawn::SwapSkin(uint32_t bagPos)
{
    return m_itemComponent->SwapSkin(static_cast<int>(bagPos));
}

uint64_t PlayerPawn::GetCurrentSkinId(SKIN_PARTS_TYPE parts) const
{
    return m_itemComponent->GetCurrentSkinId(parts);
}

void PlayerPawn::QuickSlotInteraction(int pos)
{
    // Use
}

void PlayerPawn::SkinSlotInteraction(SKIN_PARTS_TYPE parts)
{
    // UnEquip
}

void PlayerPawn::EquipmentSlotInteraction(EQUIPMENT_POS_TYPE pos)
{
    // UnEquip
}



