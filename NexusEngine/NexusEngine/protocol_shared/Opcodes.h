#pragma once

// ─────────────────────────────────────────────────────────────────────────────
// Opcodes.h — 서버 ↔ 클라이언트 공용 Opcode 정의
//
// 이 파일은 서버(C++)와 UE5 클라이언트가 동일하게 사용한다.
// 의존성: <cstdint> 만 사용. UE5에서 별도 설정 없이 include 가능.
//
// 네이밍 규칙:
//   CMSG_ : Client → Server
//   SMSG_ : Server → Client
//
// 값 할당 규칙:
//   - 그룹 기준값(_BASE)만 직접 지정. 이후 항목은 auto-increment.
//   - _BASE / _END 항목은 실제 opcode로 사용하지 않는 더미값.
//
// 그룹 범위:
//   0x0100 — 인증 / 캐릭터 설정
//   0x0200 — 이동
//   0x0300 — 채팅
//   0x0400 — 스폰 / 디스폰
// ─────────────────────────────────────────────────────────────────────────────

#include <cstdint>

enum Opcode : uint16_t
{
    OPCODE_INVALID = 0,

    // ── 인증 / 캐릭터 설정 (0x0100번대) ──────────────────────────────────────
    _AUTH_BASE            = 0x0100,   // 더미 — 직접 사용 금지
    CMSG_LOGIN,                       // C→S (TCP) [string accountName][string token]
    SMSG_LOGIN_RESULT,                // S→C (TCP) [uint8 success][string message]
    CMSG_ENTER_WORLD,                 // C→S (TCP) [uint32 characterId]
    SMSG_ENTER_WORLD,                 // S→C (TCP) [uint8 success]
                                      //           [uint64 pawnId][uint32 characterId][string name]
                                      //           [uint32 hp][uint32 maxHp]
                                      //           [float x][float y][float z][float orientation]
    CMSG_CHAR_SETUP,                  // C→S (TCP) [string characterName]
    SMSG_CHAR_SETUP_RESULT,           // S→C (TCP) [uint8 success][uint32 characterId][string message]
    _AUTH_END,                        // 더미 — 범위 검사용

    // ── 이동 (0x0200번대) ─────────────────────────────────────────────────────
    _MOVE_BASE            = 0x0200,   // 더미 — 직접 사용 금지
    CMSG_MOVE,                        // C→S (TCP) [float x][float y][float z][float orientation]
    SMSG_MOVE_BROADCAST,              // S→C (TCP) [uint64 sessionId][float x][float y][float z][float orientation]
    CMSG_MOVE_UDP,                    // C→S (UDP) [float x][float y][float z][float orientation]
    SMSG_MOVE_UDP,                    // S→C (UDP) [uint64 sessionId][float x][float y][float z][float orientation]
    _MOVE_END,                        // 더미 — 범위 검사용

    // ── 채팅 (0x0300번대) ─────────────────────────────────────────────────────
    _CHAT_BASE            = 0x0300,   // 더미 — 직접 사용 금지
    CMSG_CHAT,                        // C→S (TCP) [string text]                              — 존 내 브로드캐스트
    SMSG_CHAT,                        // S→C (TCP) [uint64 sessionId][string name][string text]
    CMSG_WORLD_CHAT,                  // C→S (TCP) [string text]                              — 전 서버 브로드캐스트
    SMSG_WORLD_CHAT,                  // S→C (TCP) [uint64 sessionId][string name][string text]
    _CHAT_END,                        // 더미 — 범위 검사용

    // ── 스폰 / 디스폰 (0x0400번대) ────────────────────────────────────────────
    _SPAWN_BASE           = 0x0400,   // 더미 — 직접 사용 금지
    SMSG_SPAWN_PLAYER,                // S→C (TCP) [uint64 pawnId][uint64 sessionId][string name]
                                      //           [uint32 hp][uint32 maxHp]
                                      //           [float x][float y][float z][float orientation]
    SMSG_DESPAWN_PLAYER,              // S→C (TCP) [uint64 pawnId]
    _SPAWN_END,                       // 더미 — 범위 검사용
};

// 그룹 범위 체크 헬퍼 (inclusive, _BASE/_END 제외)
#define NEXUS_OPCODE_IN(op, group) \
    ((op) > group##_BASE && (op) < group##_END)
