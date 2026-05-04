#pragma once

#include "../NexusTypes.h"
#include "../NexusPacketBuilder.h"
#include "../PacketParser.h"
#include "../Opcodes.h"

#include <cstdint>

// ─────────────────────────────────────────────────────────────────────────────
// Packet-Chat.h — 채팅 그룹 패킷  (0x0300번대)
// ─────────────────────────────────────────────────────────────────────────────

// ── CMSG_CHAT (0x0301) ────────────────────────────────────────────────────────
// C→S (TCP) [string text]  — 존 내 브로드캐스트
struct CMsg_Chat
{
    NexusStringType text;

    [[nodiscard]] NexusByteArray  Encode() const;
    static CMsg_Chat              Decode(NexusPacketParser& r);
};

// ── SMSG_CHAT (0x0302) ────────────────────────────────────────────────────────
// S→C (TCP) [uint64 sessionId][string name][string text]
struct SMsg_Chat
{
    uint64_t        sessionId{ 0 };
    NexusStringType name;
    NexusStringType text;

    [[nodiscard]] NexusByteArray  Encode() const;
    static SMsg_Chat              Decode(NexusPacketParser& r);
};

// ── CMSG_WORLD_CHAT (0x0303) ──────────────────────────────────────────────────
// C→S (TCP) [string text]  — 전 서버 브로드캐스트
struct CMsg_WorldChat
{
    NexusStringType text;

    [[nodiscard]] NexusByteArray  Encode() const;
    static CMsg_WorldChat         Decode(NexusPacketParser& r);
};

// ── SMSG_WORLD_CHAT (0x0304) ──────────────────────────────────────────────────
// S→C (TCP) [uint64 sessionId][string name][string text]
struct SMsg_WorldChat
{
    uint64_t        sessionId{ 0 };
    NexusStringType name;
    NexusStringType text;

    [[nodiscard]] NexusByteArray  Encode() const;
    static SMsg_WorldChat         Decode(NexusPacketParser& r);
};
