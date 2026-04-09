#include "PlayerPawn.h"

PlayerPawn::PlayerPawn(std::string name,
                       uint32_t    characterId,
                       std::weak_ptr<SessionActor> session)
    : Pawn(std::move(name), std::move(session))
    , m_characterId(characterId)
{}
