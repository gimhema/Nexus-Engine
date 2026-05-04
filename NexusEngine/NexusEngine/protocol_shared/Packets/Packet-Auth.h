#pragma once

#include "../NexusTypes.h"
#include "../NexusPacketBuilder.h"
#include "../PacketParser.h"
#include "../Opcodes.h"

#include <cstdint>

// ─────────────────────────────────────────────────────────────────────────────
// Packet-Auth.h — 인증 / 입장 그룹 패킷  (0x0100번대)
//
// 모든 패킷은 Encode() / Decode() 를 양쪽에서 사용할 수 있다.
//   Encode() — 송신측에서 바이트 버퍼 생성 (서버: SMSG, 클라이언트: CMSG)
//   Decode() — 수신측에서 구조체로 역직렬화 (서버: CMSG, 클라이언트: SMSG)
// ─────────────────────────────────────────────────────────────────────────────

// ── CMSG_LOGIN (0x0101) ───────────────────────────────────────────────────────
// C→S [string accountName][string token]
struct CMsg_Login
{
    NexusStringType accountName;
    NexusStringType token;

    [[nodiscard]] NexusByteArray  Encode() const;
    static CMsg_Login             Decode(NexusPacketParser& r);
};

// ── SMSG_LOGIN_RESULT (0x0102) ────────────────────────────────────────────────
// S→C [uint8 success][string message]
struct SMsg_LoginResult
{
    bool            success{ false };
    NexusStringType message;

    [[nodiscard]] NexusByteArray  Encode() const;
    static SMsg_LoginResult       Decode(NexusPacketParser& r);
};

// ── CMSG_CHAR_SETUP (0x0105) ──────────────────────────────────────────────────
// C→S [string characterName]
struct CMsg_CharSetup
{
    NexusStringType characterName;

    [[nodiscard]] NexusByteArray  Encode() const;
    static CMsg_CharSetup         Decode(NexusPacketParser& r);
};

// ── SMSG_CHAR_SETUP_RESULT (0x0106) ───────────────────────────────────────────
// S→C [uint8 success][uint32 characterId][string message]
struct SMsg_CharSetupResult
{
    bool            success{ false };
    uint32_t        characterId{ 0 };
    NexusStringType message;

    [[nodiscard]] NexusByteArray  Encode() const;
    static SMsg_CharSetupResult   Decode(NexusPacketParser& r);
};

// ── CMSG_ENTER_WORLD (0x0103) ─────────────────────────────────────────────────
// C→S [uint32 characterId]
struct CMsg_EnterWorld
{
    uint32_t characterId{ 0 };

    [[nodiscard]] NexusByteArray  Encode() const;
    static CMsg_EnterWorld        Decode(NexusPacketParser& r);
};

// ── SMSG_ENTER_WORLD (0x0104) ─────────────────────────────────────────────────
// S→C [uint8 success]
//     [uint64 pawnId][uint32 characterId][string name]
//     [uint32 hp][uint32 maxHp]
//     [float x][float y][float z][float orientation]
struct SMsg_EnterWorld
{
    bool            success{ false };
    uint64_t        pawnId{ 0 };
    uint32_t        characterId{ 0 };
    NexusStringType name;
    uint32_t        hp{ 0 };
    uint32_t        maxHp{ 0 };
    float           x{ 0.f }, y{ 0.f }, z{ 0.f };
    float           orientation{ 0.f };

    [[nodiscard]] NexusByteArray  Encode() const;
    static SMsg_EnterWorld        Decode(NexusPacketParser& r);
};
