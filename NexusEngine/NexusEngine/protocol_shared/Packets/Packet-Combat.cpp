#include "Packet-Combat.h"

// ── CMsg_UseSkill ─────────────────────────────────────────────────────────────
NexusByteArray CMsg_UseSkill::Encode() const
{
    NexusPacketBuilder b(CMSG_USE_SKILL);
    b.WriteU32(skillId)
        .WriteU64(targetPawnId)
        .WriteFloat(targetX).WriteFloat(targetY).WriteFloat(targetZ)
        .WriteU32(clientTimestamp);
    return b.Finalize();
}

CMsg_UseSkill CMsg_UseSkill::Decode(NexusPacketParser& r)
{
    CMsg_UseSkill msg;
    msg.skillId         = r.ReadU32();
    msg.targetPawnId    = r.ReadU64();
    msg.targetX         = r.ReadFloat();
    msg.targetY         = r.ReadFloat();
    msg.targetZ         = r.ReadFloat();
    msg.clientTimestamp = r.ReadU32();
    return msg;
}

// ── SMsg_SkillResult ──────────────────────────────────────────────────────────
NexusByteArray SMsg_SkillResult::Encode() const
{
    NexusPacketBuilder b(SMSG_SKILL_RESULT);
    b.WriteU8(success ? 1u : 0u)
        .WriteU64(attackerPawnId)
        .WriteU64(targetPawnId)
        .WriteU32(static_cast<uint32_t>(damage))   // int32 → uint32 (클라에서 부호 복원)
        .WriteU32(targetRemainHp)
        .WriteString(message);
    return b.Finalize();
}

SMsg_SkillResult SMsg_SkillResult::Decode(NexusPacketParser& r)
{
    SMsg_SkillResult msg;
    msg.success         = r.ReadU8() != 0u;
    msg.attackerPawnId  = r.ReadU64();
    msg.targetPawnId    = r.ReadU64();
    msg.damage          = static_cast<int32_t>(r.ReadU32());
    msg.targetRemainHp  = r.ReadU32();
    msg.message         = r.ReadString();
    return msg;
}

// ── SMsg_PawnHpUpdate ─────────────────────────────────────────────────────────
NexusByteArray SMsg_PawnHpUpdate::Encode() const
{
    NexusPacketBuilder b(SMSG_PAWN_HP_UPDATE);
    b.WriteU64(pawnId).WriteU32(hp).WriteU32(maxHp);
    return b.Finalize();
}

SMsg_PawnHpUpdate SMsg_PawnHpUpdate::Decode(NexusPacketParser& r)
{
    SMsg_PawnHpUpdate msg;
    msg.pawnId = r.ReadU64();
    msg.hp     = r.ReadU32();
    msg.maxHp  = r.ReadU32();
    return msg;
}
