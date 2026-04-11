#pragma once

#include "Pawn.h"
#include "../../Data/GameDataEntities/CharacterEntityData.h"

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

private:
    uint32_t m_characterId{};
};
