#pragma once

#include "../../../Actor/Actor.h"
#include "../../../Game/Messages/GameMessages.h"




class Pawn : public Actor<GameMessage>
{

public:
	Pawn() = default;
	~Pawn() override = default;

public:

};