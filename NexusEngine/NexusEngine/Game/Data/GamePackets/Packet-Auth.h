#pragma once

#include "../../../Packets/PacketBase.h"
#include "../../../protocol_shared/Opcodes.h"

#include <cstdint>
#include <string>

// ─────────────────────────────────────────────────────────────────────────────
// Packet-Auth.h — 인증 / 입장 그룹 패킷  (0x0100번대)
//
// 명명 규칙
//   CMsg_*  Client → Server 수신 패킷
//           PacketReader& 를 받는 static Decode() 로 역직렬화
//   SMsg_*  Server → Client 송신 패킷
//           PacketWriter 를 상속. 생성자가 필드를 기록. Finalize() 로 버퍼 획득.
// ─────────────────────────────────────────────────────────────────────────────

// ── CMSG_LOGIN (0x0101) ───────────────────────────────────────────────────────
// C→S [string accountName][string token]
struct CMsg_Login
{
    std::string accountName;
    std::string token;

    static CMsg_Login Decode(PacketReader& r);
};

// ── SMSG_LOGIN_RESULT (0x0102) ────────────────────────────────────────────────
// S→C [uint8 success][string message]
class SMsg_LoginResult : public PacketWriter
{
public:
    SMsg_LoginResult(bool success, std::string_view message);
};

// ── CMSG_CHAR_SETUP (0x0105) ──────────────────────────────────────────────────
// C→S [string characterName]
struct CMsg_CharSetup
{
    std::string characterName;

    static CMsg_CharSetup Decode(PacketReader& r);
};

// ── SMSG_CHAR_SETUP_RESULT (0x0106) ───────────────────────────────────────────
// S→C [uint8 success][uint32 characterId][string message]
class SMsg_CharSetupResult : public PacketWriter
{
public:
    SMsg_CharSetupResult(bool success, uint32_t characterId, std::string_view message);
};

// ── CMSG_ENTER_WORLD (0x0103) ─────────────────────────────────────────────────
// C→S [uint32 characterId]
struct CMsg_EnterWorld
{
    uint32_t characterId;

    static CMsg_EnterWorld Decode(PacketReader& r);
};

// ── SMSG_ENTER_WORLD (0x0104) ─────────────────────────────────────────────────
// S→C [uint8 success]
//     [uint64 pawnId][uint32 characterId][string name]
//     [uint32 hp][uint32 maxHp]
//     [float x][float y][float z][float orientation]
//
// success=false 시 pawnId=0, 나머지 필드는 기본값으로 전달.
class SMsg_EnterWorld : public PacketWriter
{
public:
    SMsg_EnterWorld(bool     success,
                    uint64_t pawnId,
                    uint32_t characterId,
                    std::string_view name,
                    uint32_t hp,
                    uint32_t maxHp,
                    float    x,
                    float    y,
                    float    z,
                    float    orientation);
};
