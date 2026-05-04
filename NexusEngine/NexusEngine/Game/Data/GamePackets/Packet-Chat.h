#pragma once

#include "../../../Packets/PacketBase.h"
#include "../../../protocol_shared/Opcodes.h"

#include <cstdint>
#include <string>

// ─────────────────────────────────────────────────────────────────────────────
// Packet-Chat.h — 채팅 그룹 패킷  (0x0300번대)
// ─────────────────────────────────────────────────────────────────────────────

// ── CMSG_CHAT (0x0301) ────────────────────────────────────────────────────────
// C→S (TCP) [string text]  — 존 내 브로드캐스트
struct CMsg_Chat
{
    std::string text;

    static CMsg_Chat Decode(PacketReader& r);
};

// ── SMSG_CHAT (0x0302) ────────────────────────────────────────────────────────
// S→C (TCP) [uint64 sessionId][string name][string text]
class SMsg_Chat : public PacketWriter
{
public:
    SMsg_Chat(uint64_t sessionId, std::string_view name, std::string_view text);
};

// ── CMSG_WORLD_CHAT (0x0303) ──────────────────────────────────────────────────
// C→S (TCP) [string text]  — 전 서버 브로드캐스트
struct CMsg_WorldChat
{
    std::string text;

    static CMsg_WorldChat Decode(PacketReader& r);
};

// ── SMSG_WORLD_CHAT (0x0304) ──────────────────────────────────────────────────
// S→C (TCP) [uint64 sessionId][string name][string text]
class SMsg_WorldChat : public PacketWriter
{
public:
    SMsg_WorldChat(uint64_t sessionId, std::string_view name, std::string_view text);
};
