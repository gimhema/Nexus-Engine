#include "Packet-Chat.h"

// ── CMsg_Chat ─────────────────────────────────────────────────────────────────
NexusByteArray CMsg_Chat::Encode() const
{
    NexusPacketBuilder b(CMSG_CHAT);
    b.WriteString(text);
    return b.Finalize();
}

CMsg_Chat CMsg_Chat::Decode(NexusPacketParser& r)
{
    CMsg_Chat msg;
    msg.text = r.ReadString();
    return msg;
}

// ── SMsg_Chat ─────────────────────────────────────────────────────────────────
NexusByteArray SMsg_Chat::Encode() const
{
    NexusPacketBuilder b(SMSG_CHAT);
    b.WriteU64(sessionId).WriteString(name).WriteString(text);
    return b.Finalize();
}

SMsg_Chat SMsg_Chat::Decode(NexusPacketParser& r)
{
    SMsg_Chat msg;
    msg.sessionId = r.ReadU64();
    msg.name      = r.ReadString();
    msg.text      = r.ReadString();
    return msg;
}

// ── CMsg_WorldChat ────────────────────────────────────────────────────────────
NexusByteArray CMsg_WorldChat::Encode() const
{
    NexusPacketBuilder b(CMSG_WORLD_CHAT);
    b.WriteString(text);
    return b.Finalize();
}

CMsg_WorldChat CMsg_WorldChat::Decode(NexusPacketParser& r)
{
    CMsg_WorldChat msg;
    msg.text = r.ReadString();
    return msg;
}

// ── SMsg_WorldChat ────────────────────────────────────────────────────────────
NexusByteArray SMsg_WorldChat::Encode() const
{
    NexusPacketBuilder b(SMSG_WORLD_CHAT);
    b.WriteU64(sessionId).WriteString(name).WriteString(text);
    return b.Finalize();
}

SMsg_WorldChat SMsg_WorldChat::Decode(NexusPacketParser& r)
{
    SMsg_WorldChat msg;
    msg.sessionId = r.ReadU64();
    msg.name      = r.ReadString();
    msg.text      = r.ReadString();
    return msg;
}
