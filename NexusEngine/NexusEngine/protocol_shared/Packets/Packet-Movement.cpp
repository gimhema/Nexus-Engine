#include "Packet-Movement.h"

// ── CMsg_Move ─────────────────────────────────────────────────────────────────
NexusByteArray CMsg_Move::Encode() const
{
    NexusPacketBuilder b(CMSG_MOVE);
    b.WriteFloat(x).WriteFloat(y).WriteFloat(z).WriteFloat(orientation);
    return b.Finalize();
}

CMsg_Move CMsg_Move::Decode(NexusPacketParser& r)
{
    CMsg_Move msg;
    msg.x           = r.ReadFloat();
    msg.y           = r.ReadFloat();
    msg.z           = r.ReadFloat();
    msg.orientation = r.ReadFloat();
    return msg;
}

// ── SMsg_MoveBroadcast ────────────────────────────────────────────────────────
NexusByteArray SMsg_MoveBroadcast::Encode() const
{
    NexusPacketBuilder b(SMSG_MOVE_BROADCAST);
    b.WriteU64(sessionId).WriteFloat(x).WriteFloat(y).WriteFloat(z).WriteFloat(orientation);
    return b.Finalize();
}

SMsg_MoveBroadcast SMsg_MoveBroadcast::Decode(NexusPacketParser& r)
{
    SMsg_MoveBroadcast msg;
    msg.sessionId   = r.ReadU64();
    msg.x           = r.ReadFloat();
    msg.y           = r.ReadFloat();
    msg.z           = r.ReadFloat();
    msg.orientation = r.ReadFloat();
    return msg;
}

// ── CMsg_MoveUdp ──────────────────────────────────────────────────────────────
NexusByteArray CMsg_MoveUdp::Encode() const
{
    NexusPacketBuilder b(CMSG_MOVE_UDP);
    b.WriteFloat(x).WriteFloat(y).WriteFloat(z).WriteFloat(orientation);
    return b.Finalize();
}

CMsg_MoveUdp CMsg_MoveUdp::Decode(NexusPacketParser& r)
{
    CMsg_MoveUdp msg;
    msg.x           = r.ReadFloat();
    msg.y           = r.ReadFloat();
    msg.z           = r.ReadFloat();
    msg.orientation = r.ReadFloat();
    return msg;
}

// ── SMsg_MoveUdp ──────────────────────────────────────────────────────────────
NexusByteArray SMsg_MoveUdp::Encode() const
{
    NexusPacketBuilder b(SMSG_MOVE_UDP);
    b.WriteU64(sessionId).WriteFloat(x).WriteFloat(y).WriteFloat(z).WriteFloat(orientation);
    return b.Finalize();
}

SMsg_MoveUdp SMsg_MoveUdp::Decode(NexusPacketParser& r)
{
    SMsg_MoveUdp msg;
    msg.sessionId   = r.ReadU64();
    msg.x           = r.ReadFloat();
    msg.y           = r.ReadFloat();
    msg.z           = r.ReadFloat();
    msg.orientation = r.ReadFloat();
    return msg;
}
