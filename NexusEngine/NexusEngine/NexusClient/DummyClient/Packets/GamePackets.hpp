#pragma once

// ─────────────────────────────────────────────────────────────────────────────
// GamePackets.hpp — 클라이언트측 Opcode 정의
//
// 서버의 Packet-Example.hpp와 동일한 값을 유지해야 함.
// ─────────────────────────────────────────────────────────────────────────────

#include <cstdint>

enum Opcode : uint16_t
{
    OPCODE_INVALID = 0,

    // ── 인증 / 접속 (0x0100번대) ──────────────────────────────────────────────
    _AUTH_BASE      = 0x0100,
    CMSG_LOGIN,             // 로그인 요청            [string accountName][string token]
    SMSG_LOGIN_RESULT,      // 로그인 결과            [uint8 success][string message]
    CMSG_ENTER_WORLD,       // 월드 진입 요청         [uint32 characterId]
    SMSG_ENTER_WORLD,       // 월드 진입 결과         [uint8 success]
                            //                        [uint64 pawnId][uint32 characterId][string name]
                            //                        [uint32 hp][uint32 maxHp]
                            //                        [float x][float y][float z][float orientation]
    _AUTH_END,

    // ── 이동 (0x0200번대) ─────────────────────────────────────────────────────
    _MOVE_BASE      = 0x0200,
    CMSG_MOVE,              // 이동 요청 (TCP)        [float x][float y][float z][float orientation]
    SMSG_MOVE_BROADCAST,    // 이동 브로드캐스트 (TCP)[uint64 sessionId][float x][float y][float z][float orientation]
    CMSG_MOVE_UDP,          // 이동 요청 (UDP)        [float x][float y][float z][float orientation]
    SMSG_MOVE_UDP,          // 이동 브로드캐스트 (UDP)[uint64 sessionId][float x][float y][float z][float orientation]
    _MOVE_END,

    // ── 채팅 (0x0300번대) ─────────────────────────────────────────────────────
    _CHAT_BASE      = 0x0300,
    CMSG_CHAT,              // 채팅 메시지            [string text]
    SMSG_CHAT,              // 채팅 브로드캐스트      [uint64 sessionId][string name][string text]
    _CHAT_END,

    // ── 스폰 / 디스폰 (0x0400번대) ────────────────────────────────────────────
    _SPAWN_BASE     = 0x0400,
    SMSG_SPAWN_PLAYER,      // 다른 플레이어 스폰 알림 [uint64 pawnId][uint64 sessionId][string name]
                            //                         [uint32 hp][uint32 maxHp]
                            //                         [float x][float y][float z][float orientation]
    SMSG_DESPAWN_PLAYER,    // 다른 플레이어 디스폰 알림[uint64 pawnId]
    _SPAWN_END,
};
