#include "Packet-Chat.h"

// ── CMsg_Chat ─────────────────────────────────────────────────────────────────
CMsg_Chat CMsg_Chat::Decode(PacketReader& r)
{
    CMsg_Chat msg;
    msg.text = r.ReadString();
    return msg;
}

// ── SMsg_Chat ─────────────────────────────────────────────────────────────────
SMsg_Chat::SMsg_Chat(uint64_t sessionId, std::string_view name, std::string_view text)
    : PacketWriter(SMSG_CHAT)
{
    WriteUInt64(sessionId).WriteString(name).WriteString(text);
}

// ── CMsg_WorldChat ────────────────────────────────────────────────────────────
CMsg_WorldChat CMsg_WorldChat::Decode(PacketReader& r)
{
    CMsg_WorldChat msg;
    msg.text = r.ReadString();
    return msg;
}

// ── SMsg_WorldChat ────────────────────────────────────────────────────────────
SMsg_WorldChat::SMsg_WorldChat(uint64_t sessionId, std::string_view name, std::string_view text)
    : PacketWriter(SMSG_WORLD_CHAT)
{
    WriteUInt64(sessionId).WriteString(name).WriteString(text);
}
