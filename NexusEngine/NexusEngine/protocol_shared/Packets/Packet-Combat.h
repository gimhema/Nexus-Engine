#pragma once

#include "../NexusTypes.h"
#include "../NexusPacketBuilder.h"
#include "../PacketParser.h"
#include "../Opcodes.h"

#include <cstdint>

// ─────────────────────────────────────────────────────────────────────────────
// Packet-Combat.h — 전투 그룹 패킷  (0x0500번대)
// ─────────────────────────────────────────────────────────────────────────────

// ── CMSG_USE_SKILL (0x0501) ───────────────────────────────────────────────────
// C→S (TCP)
// targetPawnId = 0 이면 지점 지정 스킬 (AoE), 아니면 단일 대상 스킬
struct CMsg_UseSkill
{
    uint32_t skillId;
    uint64_t targetPawnId;   // 단일 대상 (AoE면 0)
    float    targetX;        // 스킬 시전 좌표 (AoE 중심점 또는 시전자 위치)
    float    targetY;
    float    targetZ;
    uint32_t clientTimestamp; // 클라이언트 로컬 타임스탬프 (ms) — 래그 보상용

    [[nodiscard]] NexusByteArray  Encode() const;
    static CMsg_UseSkill          Decode(NexusPacketParser& r);
};

// ── SMSG_SKILL_RESULT (0x0502) ────────────────────────────────────────────────
// S→C (TCP)
// success=false 시 damage=0, message=실패 사유
struct SMsg_SkillResult
{
    bool            success{ false };
    uint64_t        attackerPawnId{ 0 };
    uint64_t        targetPawnId{ 0 };
    int32_t         damage{ 0 };         // 실제 적용된 데미지 (방어 차감 후)
    uint32_t        targetRemainHp{ 0 };
    NexusStringType message;             // 실패 사유 또는 "OK"

    [[nodiscard]] NexusByteArray  Encode() const;
    static SMsg_SkillResult       Decode(NexusPacketParser& r);
};

// ── SMSG_PAWN_HP_UPDATE (0x0503) ─────────────────────────────────────────────
// S→C (TCP) — 사망/회복 등 HP 단독 브로드캐스트
struct SMsg_PawnHpUpdate
{
    uint64_t pawnId{ 0 };
    uint32_t hp{ 0 };
    uint32_t maxHp{ 0 };

    [[nodiscard]] NexusByteArray  Encode() const;
    static SMsg_PawnHpUpdate      Decode(NexusPacketParser& r);
};
