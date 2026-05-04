#include "Packet-Spawn.h"

// ── SMsg_SpawnPlayer ──────────────────────────────────────────────────────────
SMsg_SpawnPlayer::SMsg_SpawnPlayer(uint64_t pawnId,
                                    uint64_t sessionId,
                                    std::string_view name,
                                    uint32_t hp,
                                    uint32_t maxHp,
                                    float    x,
                                    float    y,
                                    float    z,
                                    float    orientation)
    : PacketWriter(SMSG_SPAWN_PLAYER)
{
    WriteUInt64(pawnId)
        .WriteUInt64(sessionId)
        .WriteString(name)
        .WriteUInt32(hp)
        .WriteUInt32(maxHp)
        .WriteFloat(x).WriteFloat(y).WriteFloat(z)
        .WriteFloat(orientation);
}

// ── SMsg_DespawnPlayer ────────────────────────────────────────────────────────
SMsg_DespawnPlayer::SMsg_DespawnPlayer(uint64_t pawnId)
    : PacketWriter(SMSG_DESPAWN_PLAYER)
{
    WriteUInt64(pawnId);
}
