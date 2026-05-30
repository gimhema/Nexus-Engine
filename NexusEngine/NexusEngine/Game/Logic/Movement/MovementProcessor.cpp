#include "MovementProcessor.h"

#include "../Pawn/Pawn.h"

namespace MovementProcessor
{

EResult ApplyMove(Pawn& pawn, const Vec3& pos, float orientation,
                  uint32_t /*flags*/, const Zone& zone)
{
    if (!zone.IsInBounds(pos))
        return EResult::OutOfBounds;

    pawn.SetPos(pos);
    pawn.SetOrientation(orientation);
    return EResult::Ok;
}

void ApplyMoveUdp(Pawn& pawn, const Vec3& pos, float orientation)
{
    pawn.SetPos(pos);
    pawn.SetOrientation(orientation);
}

} // namespace MovementProcessor
