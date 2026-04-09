#pragma once

#include "Pawn.h"

// ─────────────────────────────────────────────────────────────────────────────
// PlayerPawn — 플레이어블 Pawn (클라이언트 세션 보유)
//
// Pawn 베이스에 플레이어 전용 데이터를 추가한 서브클래스.
// ZoneActor는 m_playerPawns 맵에 PlayerPawn을 sessionId 키로 보관한다.
//
// 향후 추가 예정:
//   - 인벤토리, 장비 슬롯
//   - 경험치/레벨
//   - 퀘스트 진행 상태
// ─────────────────────────────────────────────────────────────────────────────
class PlayerPawn : public Pawn
{
public:
    PlayerPawn(std::string name,
               uint32_t    characterId,
               std::weak_ptr<SessionActor> session);

    ~PlayerPawn() override = default;

    [[nodiscard]] uint32_t GetCharacterId() const { return m_characterId; }

private:
    uint32_t m_characterId{};
};
