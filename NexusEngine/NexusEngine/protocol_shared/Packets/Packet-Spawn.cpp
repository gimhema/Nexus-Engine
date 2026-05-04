#include "Packet-Spawn.h"

// ── SMsg_SpawnPlayer ──────────────────────────────────────────────────────────
NexusByteArray SMsg_SpawnPlayer::Encode() const
{
    NexusPacketBuilder b(SMSG_SPAWN_PLAYER);
    b.WriteU64(pawnId)
        .WriteU64(sessionId)
        .WriteString(name)
        .WriteU32(hp)
        .WriteU32(maxHp)
        .WriteFloat(x).WriteFloat(y).WriteFloat(z)
        .WriteFloat(orientation);
    return b.Finalize();
}

SMsg_SpawnPlayer SMsg_SpawnPlayer::Decode(NexusPacketParser& r)
{
    SMsg_SpawnPlayer msg;
    msg.pawnId      = r.ReadU64();
    msg.sessionId   = r.ReadU64();
    msg.name        = r.ReadString();
    msg.hp          = r.ReadU32();
    msg.maxHp       = r.ReadU32();
    msg.x           = r.ReadFloat();
    msg.y           = r.ReadFloat();
    msg.z           = r.ReadFloat();
    msg.orientation = r.ReadFloat();
    return msg;
}

// ── SMsg_DespawnPlayer ────────────────────────────────────────────────────────
NexusByteArray SMsg_DespawnPlayer::Encode() const
{
    NexusPacketBuilder b(SMSG_DESPAWN_PLAYER);
    b.WriteU64(pawnId);
    return b.Finalize();
}

SMsg_DespawnPlayer SMsg_DespawnPlayer::Decode(NexusPacketParser& r)
{
    SMsg_DespawnPlayer msg;
    msg.pawnId = r.ReadU64();
    return msg;
}
