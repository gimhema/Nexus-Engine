#pragma once

// ─────────────────────────────────────────────────────────────────────────────
// ArbiterOpcodes.h — 런처↔서버 관리 채널 전용 Opcode
//
// 게임 클라이언트↔서버 Opcode(Opcodes.h)와 완전히 분리된 별도 열거형.
// 런처(Tauri/Rust)와 NexusEngine Arbiter가 동일하게 사용한다.
//
// 네이밍 규칙:
//   LMSG_ : Launcher → Server (런처가 서버에 보내는 명령)
//   AMSG_ : Server  → Launcher (서버가 런처에 보내는 응답/이벤트)
//
// 패킷 포맷: 게임 프로토콜과 동일한 바이너리 구조 재사용
//   [ uint16 size ][ uint16 opcode ][ payload... ]
//
// 그룹 범위:
//   0x0F00 — 런처 관리 (인증, 상태 조회, 호스트 권한)
// ─────────────────────────────────────────────────────────────────────────────

#include <cstdint>

enum ArbiterOpcode : uint16_t
{
    ARBITER_OPCODE_INVALID    = 0,

    // ── 런처 관리 (0x0F00번대) ────────────────────────────────────────────────
    _ARBITER_BASE             = 0x0F00,  // 더미 — 직접 사용 금지

    LMSG_AUTH,               // L→S [string secret]                           — 런처 인증 요청
    AMSG_AUTH_RESULT,        // S→L [uint8 success][string message]

    LMSG_GET_STATUS,         // L→S (payload 없음)                             — 서버 상태 조회
    AMSG_STATUS,             // S→L [uint32 playerCount][uint32 uptimeSeconds]

    LMSG_KICK_PLAYER,        // L→S [uint64 sessionId][string reason]          — 플레이어 강제 퇴장
    AMSG_KICK_RESULT,        // S→L [uint8 success][string message]

    AMSG_EVENT_SERVER_READY, // S→L (push, payload 없음)                                           — 서버 준비 완료
    AMSG_EVENT_PLAYER_JOIN,  // S→L (push) [uint64 sessionId][string name]                       — 플레이어 접속
    AMSG_EVENT_PLAYER_LEAVE, // S→L (push) [uint64 sessionId]                                    — 플레이어 접속 해제

    LMSG_GET_PLAYERS,        // L→S (payload 없음)                                               — 현재 접속자 목록 조회
    AMSG_PLAYERS,            // S→L [uint32 count][{uint64 sessionId, string name} × count]      — 접속자 목록 스냅샷

    _ARBITER_END,            // 더미 — 범위 검사용
};

// 범위 검사 헬퍼
#define ARBITER_OPCODE_IN(op) ((op) > _ARBITER_BASE && (op) < _ARBITER_END)
