#pragma once

#include "../../../Packets/PacketBase.h"
#include "../../../protocol_shared/Opcodes.h"

#include <cstdint>

// ─────────────────────────────────────────────────────────────────────────────
// Packet-Movement.h — 이동 그룹 패킷  (0x0200번대)
// ─────────────────────────────────────────────────────────────────────────────

// ── CMSG_MOVE (0x0201) ────────────────────────────────────────────────────────
// C→S (TCP) [float x][float y][float z][float orientation]
struct CMsg_Move
{
    float x, y, z;
    float orientation;

    static CMsg_Move Decode(PacketReader& r);
};

// ── SMSG_MOVE_BROADCAST (0x0202) ─────────────────────────────────────────────
// S→C (TCP) [uint64 sessionId][float x][float y][float z][float orientation]
class SMsg_MoveBroadcast : public PacketWriter
{
public:
    SMsg_MoveBroadcast(uint64_t sessionId,
                       float    x,
                       float    y,
                       float    z,
                       float    orientation);
};

// ── CMSG_MOVE_UDP (0x0203) ────────────────────────────────────────────────────
// C→S (UDP) [float x][float y][float z][float orientation]
// payload 구조는 CMSG_MOVE 와 동일. UDP opcode 구분용으로 별도 타입 유지.
struct CMsg_MoveUdp
{
    float x, y, z;
    float orientation;

    static CMsg_MoveUdp Decode(PacketReader& r);
};

// ── SMSG_MOVE_UDP (0x0204) ────────────────────────────────────────────────────
// S→C (UDP) [uint64 sessionId][float x][float y][float z][float orientation]
// 주의: UDP 송신 시 sessionToken 삽입은 NetworkManager::SendUDP 레이어에서 처리됨.
class SMsg_MoveUdp : public PacketWriter
{
public:
    SMsg_MoveUdp(uint64_t sessionId,
                 float    x,
                 float    y,
                 float    z,
                 float    orientation);
};
