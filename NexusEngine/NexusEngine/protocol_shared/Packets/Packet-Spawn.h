#pragma once

#include "../NexusTypes.h"
#include "../NexusPacketBuilder.h"
#include "../PacketParser.h"
#include "../Opcodes.h"

#include <cstdint>

// ─────────────────────────────────────────────────────────────────────────────
// Packet-Spawn.h — 스폰 / 디스폰 그룹 패킷  (0x0400번대)
//
// 이 그룹은 서버만 전송(S→C)하지만, 클라이언트 유닛 테스트 등을 위해
// Encode() 도 구현한다.
// ─────────────────────────────────────────────────────────────────────────────

// ── SMSG_SPAWN_PLAYER (0x0401) ────────────────────────────────────────────────
// S→C (TCP) [uint64 pawnId][uint64 sessionId][string name]
//           [uint32 hp][uint32 maxHp]
//           [float x][float y][float z][float orientation]
//           [uint64 skinHead][uint64 skinBodyUp][uint64 skinBodyDown]
//           [uint64 skinHand][uint64 skinShoes]
//           — skin 필드: 0 = 해당 부위 기본 외형 (미장착)
struct SMsg_SpawnPlayer
{
    uint64_t        pawnId{ 0 };
    uint64_t        sessionId{ 0 };
    NexusStringType name;
    uint32_t        hp{ 0 };
    uint32_t        maxHp{ 0 };
    float           x{ 0.f }, y{ 0.f }, z{ 0.f };
    float           orientation{ 0.f };
    uint64_t        skinHead{ 0 };
    uint64_t        skinBodyUp{ 0 };
    uint64_t        skinBodyDown{ 0 };
    uint64_t        skinHand{ 0 };
    uint64_t        skinShoes{ 0 };

    [[nodiscard]] NexusByteArray  Encode() const;
    static SMsg_SpawnPlayer       Decode(NexusPacketParser& r);
};

// ── SMSG_DESPAWN_PLAYER (0x0402) ─────────────────────────────────────────────
// S→C (TCP) [uint64 pawnId]
struct SMsg_DespawnPlayer
{
    uint64_t pawnId{ 0 };

    [[nodiscard]] NexusByteArray  Encode() const;
    static SMsg_DespawnPlayer     Decode(NexusPacketParser& r);
};
