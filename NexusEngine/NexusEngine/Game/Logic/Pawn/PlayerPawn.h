#pragma once

#include "Pawn.h"
#include "../../Data/GameDataEntities/CharacterEntityData.h"
#include "PlayerItemComponent.h"
#include "../Item.h"

#include <chrono>
#include <unordered_map>

// ─────────────────────────────────────────────────────────────────────────────
// PlayerPawn — 플레이어블 Pawn (클라이언트 세션 보유)
//
// Pawn 베이스에 플레이어 전용 식별자(characterId)를 추가.
// 인게임 데이터는 CharacterEntityData (level, exp, 향후 인벤토리/장비)를 통해 관리.
//
// 사용 예:
//   auto* data = static_cast<CharacterEntityData*>(pawn->GetEntityData());
//   data->AddExperience(150);
// ─────────────────────────────────────────────────────────────────────────────



class PlayerPawn : public Pawn
{
public:
    PlayerPawn(std::string name,
               uint32_t    characterId,
               std::weak_ptr<SessionActor> session);

    ~PlayerPawn() override = default;

    [[nodiscard]] uint32_t GetCharacterId() const { return m_characterId; }

    // CharacterEntityData 직접 접근 편의 메서드 (nullptr 반환 가능성 없음)
    [[nodiscard]] CharacterEntityData& GetCharacterData()
    {
        return *static_cast<CharacterEntityData*>(m_data.get());
    }
    [[nodiscard]] const CharacterEntityData& GetCharacterData() const
    {
        return *static_cast<const CharacterEntityData*>(m_data.get());
    }

    // ── 스킬 쿨타임 ──────────────────────────────────────────────────────────
    // 쿨타임 진행 중이면 true 반환
    [[nodiscard]] bool IsOnCooldown(uint32_t skillId) const;

    // 스킬 사용 후 쿨타임 등록 (cooldownMs 밀리초 동안 재사용 불가)
    void SetCooldown(uint32_t skillId, uint32_t cooldownMs);

private:
    uint32_t m_characterId{};

    using Clock     = std::chrono::steady_clock;
    using TimePoint = Clock::time_point;

    // skillId → 쿨타임 만료 시각. ZoneActor 전용 스레드에서만 접근.
    std::unordered_map<uint32_t, TimePoint> m_cooldowns;

    // Item Member
private:
    std::unique_ptr<ItemComponent> m_itemComponent;

    // Item Action
public:
    // Pick & Drop
    void PickUpItem(ItemBase* pickuped);
    void DropItem(ITEM_TYPE bagType, int pos);

    // ItemBag Interaction
    void ItemBagInteraction(ITEM_TYPE bagType, int pos);

    // Slot Interaction
    void QuickSlotInteraction(int pos);
    void SkinSlotInteraction(SKIN_PARTS_TYPE parts);
    void EquipmentSlotInteraction(EQUIPMENT_POS_TYPE pos);

    // ZoneActor 전용 — 스킨 장착 후 브로드캐스트에 필요한 결과 반환
    SkinSwapResult SwapSkin(uint32_t bagPos);
};
