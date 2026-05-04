#pragma once

#include "../../../Packets/PacketBase.h"
#include "../../../protocol_shared/Opcodes.h"

#include <cstdint>
#include <string>

// ─────────────────────────────────────────────────────────────────────────────
// Packet-Spawn.h — 스폰 / 디스폰 그룹 패킷  (0x0400번대)
//
// 이 그룹의 패킷은 서버만 전송(S→C)하므로 CMsg 클래스가 없다.
// ─────────────────────────────────────────────────────────────────────────────

// ── SMSG_SPAWN_PLAYER (0x0401) ────────────────────────────────────────────────
// S→C (TCP) [uint64 pawnId][uint64 sessionId][string name]
//           [uint32 hp][uint32 maxHp]
//           [float x][float y][float z][float orientation]
//
// 두 상황에서 모두 전송됨:
//   A) 내가 존 입장 시 → 기존 플레이어 목록을 각 1회씩 전송
//   B) 다른 플레이어가 존 입장 시 → 나를 포함한 기존 플레이어에게 1회 전송
class SMsg_SpawnPlayer : public PacketWriter
{
public:
    SMsg_SpawnPlayer(uint64_t pawnId,
                     uint64_t sessionId,
                     std::string_view name,
                     uint32_t hp,
                     uint32_t maxHp,
                     float    x,
                     float    y,
                     float    z,
                     float    orientation);
};

// ── SMSG_DESPAWN_PLAYER (0x0402) ─────────────────────────────────────────────
// S→C (TCP) [uint64 pawnId]
class SMsg_DespawnPlayer : public PacketWriter
{
public:
    explicit SMsg_DespawnPlayer(uint64_t pawnId);
};
