#include "Packet-Auth.h"

// ── CMsg_Login ────────────────────────────────────────────────────────────────
CMsg_Login CMsg_Login::Decode(PacketReader& r)
{
    CMsg_Login msg;
    msg.accountName = r.ReadString();
    msg.token       = r.ReadString();
    return msg;
}

// ── SMsg_LoginResult ──────────────────────────────────────────────────────────
SMsg_LoginResult::SMsg_LoginResult(bool success, std::string_view message)
    : PacketWriter(SMSG_LOGIN_RESULT)
{
    WriteUInt8(success ? 1u : 0u).WriteString(message);
}

// ── CMsg_CharSetup ────────────────────────────────────────────────────────────
CMsg_CharSetup CMsg_CharSetup::Decode(PacketReader& r)
{
    CMsg_CharSetup msg;
    msg.characterName = r.ReadString();
    return msg;
}

// ── SMsg_CharSetupResult ──────────────────────────────────────────────────────
SMsg_CharSetupResult::SMsg_CharSetupResult(bool           success,
                                            uint32_t       characterId,
                                            std::string_view message)
    : PacketWriter(SMSG_CHAR_SETUP_RESULT)
{
    WriteUInt8(success ? 1u : 0u).WriteUInt32(characterId).WriteString(message);
}

// ── CMsg_EnterWorld ───────────────────────────────────────────────────────────
CMsg_EnterWorld CMsg_EnterWorld::Decode(PacketReader& r)
{
    CMsg_EnterWorld msg;
    msg.characterId = r.ReadUInt32();
    return msg;
}

// ── SMsg_EnterWorld ───────────────────────────────────────────────────────────
SMsg_EnterWorld::SMsg_EnterWorld(bool     success,
                                  uint64_t pawnId,
                                  uint32_t characterId,
                                  std::string_view name,
                                  uint32_t hp,
                                  uint32_t maxHp,
                                  float    x,
                                  float    y,
                                  float    z,
                                  float    orientation)
    : PacketWriter(SMSG_ENTER_WORLD)
{
    WriteUInt8(success ? 1u : 0u)
        .WriteUInt64(pawnId)
        .WriteUInt32(characterId)
        .WriteString(name)
        .WriteUInt32(hp)
        .WriteUInt32(maxHp)
        .WriteFloat(x).WriteFloat(y).WriteFloat(z)
        .WriteFloat(orientation);
}
