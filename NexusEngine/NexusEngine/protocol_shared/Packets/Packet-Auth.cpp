#include "Packet-Auth.h"

// ── CMsg_Login ────────────────────────────────────────────────────────────────
NexusByteArray CMsg_Login::Encode() const
{
    NexusPacketBuilder b(CMSG_LOGIN);
    b.WriteString(accountName).WriteString(token);
    return b.Finalize();
}

CMsg_Login CMsg_Login::Decode(NexusPacketParser& r)
{
    CMsg_Login msg;
    msg.accountName = r.ReadString();
    msg.token       = r.ReadString();
    return msg;
}

// ── SMsg_LoginResult ──────────────────────────────────────────────────────────
NexusByteArray SMsg_LoginResult::Encode() const
{
    NexusPacketBuilder b(SMSG_LOGIN_RESULT);
    b.WriteU8(success ? 1u : 0u).WriteString(message);
    return b.Finalize();
}

SMsg_LoginResult SMsg_LoginResult::Decode(NexusPacketParser& r)
{
    SMsg_LoginResult msg;
    msg.success = r.ReadU8() != 0u;
    msg.message = r.ReadString();
    return msg;
}

// ── CMsg_CharSetup ────────────────────────────────────────────────────────────
NexusByteArray CMsg_CharSetup::Encode() const
{
    NexusPacketBuilder b(CMSG_CHAR_SETUP);
    b.WriteString(characterName);
    return b.Finalize();
}

CMsg_CharSetup CMsg_CharSetup::Decode(NexusPacketParser& r)
{
    CMsg_CharSetup msg;
    msg.characterName = r.ReadString();
    return msg;
}

// ── SMsg_CharSetupResult ──────────────────────────────────────────────────────
NexusByteArray SMsg_CharSetupResult::Encode() const
{
    NexusPacketBuilder b(SMSG_CHAR_SETUP_RESULT);
    b.WriteU8(success ? 1u : 0u).WriteU32(characterId).WriteString(message);
    return b.Finalize();
}

SMsg_CharSetupResult SMsg_CharSetupResult::Decode(NexusPacketParser& r)
{
    SMsg_CharSetupResult msg;
    msg.success     = r.ReadU8() != 0u;
    msg.characterId = r.ReadU32();
    msg.message     = r.ReadString();
    return msg;
}

// ── CMsg_EnterWorld ───────────────────────────────────────────────────────────
NexusByteArray CMsg_EnterWorld::Encode() const
{
    NexusPacketBuilder b(CMSG_ENTER_WORLD);
    b.WriteU32(characterId);
    return b.Finalize();
}

CMsg_EnterWorld CMsg_EnterWorld::Decode(NexusPacketParser& r)
{
    CMsg_EnterWorld msg;
    msg.characterId = r.ReadU32();
    return msg;
}

// ── SMsg_EnterWorld ───────────────────────────────────────────────────────────
NexusByteArray SMsg_EnterWorld::Encode() const
{
    NexusPacketBuilder b(SMSG_ENTER_WORLD);
    b.WriteU8(success ? 1u : 0u)
        .WriteU64(pawnId)
        .WriteU32(characterId)
        .WriteString(name)
        .WriteU32(hp)
        .WriteU32(maxHp)
        .WriteFloat(x).WriteFloat(y).WriteFloat(z)
        .WriteFloat(orientation);
    return b.Finalize();
}

SMsg_EnterWorld SMsg_EnterWorld::Decode(NexusPacketParser& r)
{
    SMsg_EnterWorld msg;
    msg.success     = r.ReadU8() != 0u;
    msg.pawnId      = r.ReadU64();
    msg.characterId = r.ReadU32();
    msg.name        = r.ReadString();
    msg.hp          = r.ReadU32();
    msg.maxHp       = r.ReadU32();
    msg.x           = r.ReadFloat();
    msg.y           = r.ReadFloat();
    msg.z           = r.ReadFloat();
    msg.orientation = r.ReadFloat();
    return msg;
}
