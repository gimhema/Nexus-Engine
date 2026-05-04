#include "Packet-Movement.h"

// ── CMsg_Move ─────────────────────────────────────────────────────────────────
CMsg_Move CMsg_Move::Decode(PacketReader& r)
{
    CMsg_Move msg;
    msg.x           = r.ReadFloat();
    msg.y           = r.ReadFloat();
    msg.z           = r.ReadFloat();
    msg.orientation = r.ReadFloat();
    return msg;
}

// ── SMsg_MoveBroadcast ────────────────────────────────────────────────────────
SMsg_MoveBroadcast::SMsg_MoveBroadcast(uint64_t sessionId,
                                        float    x,
                                        float    y,
                                        float    z,
                                        float    orientation)
    : PacketWriter(SMSG_MOVE_BROADCAST)
{
    WriteUInt64(sessionId)
        .WriteFloat(x).WriteFloat(y).WriteFloat(z)
        .WriteFloat(orientation);
}

// ── CMsg_MoveUdp ──────────────────────────────────────────────────────────────
CMsg_MoveUdp CMsg_MoveUdp::Decode(PacketReader& r)
{
    CMsg_MoveUdp msg;
    msg.x           = r.ReadFloat();
    msg.y           = r.ReadFloat();
    msg.z           = r.ReadFloat();
    msg.orientation = r.ReadFloat();
    return msg;
}

// ── SMsg_MoveUdp ──────────────────────────────────────────────────────────────
SMsg_MoveUdp::SMsg_MoveUdp(uint64_t sessionId,
                              float    x,
                              float    y,
                              float    z,
                              float    orientation)
    : PacketWriter(SMSG_MOVE_UDP)
{
    WriteUInt64(sessionId)
        .WriteFloat(x).WriteFloat(y).WriteFloat(z)
        .WriteFloat(orientation);
}
