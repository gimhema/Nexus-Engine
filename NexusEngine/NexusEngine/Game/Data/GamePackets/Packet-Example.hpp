#pragma once

#include "../../../Packets/PacketBase.h"

// ─────────────────────────────────────────────────────────────────────────────
// Opcode 정의
//
// CMSG_ : Client → Server
// SMSG_ : Server → Client
//
// 값 할당 규칙:
//   - 그룹 기준값(_BASE)만 직접 지정. 이후 항목은 auto-increment.
//   - 새 그룹 추가 시 _BASE 하나만 지정하면 이후 값 충돌 없음.
//   - _BASE 항목은 실제 opcode로 사용하지 않는 더미값.
//   - 그룹 범위 확인: OPCODE_IN(opcode, GROUP) 매크로 사용.
//
// 그룹 범위:
//   0x0100번대 — 인증 / 접속
//   0x0200번대 — 이동
//   0x0300번대 — 채팅
// ─────────────────────────────────────────────────────────────────────────────

// 그룹 범위 체크 헬퍼 (inclusive, _BASE 제외)
#define OPCODE_IN(op, group) \
    ((op) > group##_BASE && (op) < group##_END)

enum Opcode : uint16_t
{
    OPCODE_INVALID = 0,

    // ── 인증 / 접속 (0x0100번대) ──────────────────────────────────────────────
    _AUTH_BASE      = 0x0100,   // 더미 — 직접 사용 금지
    CMSG_LOGIN,                 // 로그인 요청            [string accountName][string token]
    SMSG_LOGIN_RESULT,          // 로그인 결과            [uint8 success][string message]
    CMSG_ENTER_WORLD,           // 월드 진입 요청         [uint32 characterId]
    SMSG_ENTER_WORLD,           // 월드 진입 결과         [uint8 success]
                                //                        [uint64 pawnId][uint32 characterId][string name]
                                //                        [uint32 hp][uint32 maxHp]
                                //                        [float x][float y][float z][float orientation]
    CMSG_CHAR_SETUP,            // 캐릭터 설정            [string characterName]
    SMSG_CHAR_SETUP_RESULT,     // 캐릭터 설정 결과       [uint8 success][uint32 characterId][string message]
    _AUTH_END,                  // 더미 — 범위 검사용

    // ── 이동 (0x0200번대) ─────────────────────────────────────────────────────
    _MOVE_BASE      = 0x0200,   // 더미 — 직접 사용 금지
    CMSG_MOVE,                  // 이동 요청 (TCP)        [float x][float y][float z][float orientation]
    SMSG_MOVE_BROADCAST,        // 이동 브로드캐스트 (TCP)[uint64 sessionId][float x][float y][float z][float orientation]
    CMSG_MOVE_UDP,              // 이동 요청 (UDP)        [float x][float y][float z][float orientation]
    SMSG_MOVE_UDP,              // 이동 브로드캐스트 (UDP)[uint64 sessionId][float x][float y][float z][float orientation]
    _MOVE_END,                  // 더미 — 범위 검사용

    // ── 채팅 (0x0300번대) ─────────────────────────────────────────────────────
    _CHAT_BASE      = 0x0300,   // 더미 — 직접 사용 금지
    CMSG_CHAT,                  // 존 채팅 메시지          [string text]
    SMSG_CHAT,                  // 존 채팅 브로드캐스트    [uint64 sessionId][string name][string text]
    CMSG_WORLD_CHAT,            // 월드 채팅 메시지        [string text]
    SMSG_WORLD_CHAT,            // 월드 채팅 브로드캐스트  [uint64 sessionId][string name][string text]
    _CHAT_END,                  // 더미 — 범위 검사용

    // ── 스폰 / 디스폰 (0x0400번대) ────────────────────────────────────────────
    _SPAWN_BASE     = 0x0400,   // 더미 — 직접 사용 금지
    SMSG_SPAWN_PLAYER,          // 다른 플레이어 스폰 알림 [uint64 pawnId][uint64 sessionId][string name]
                                //                         [uint32 hp][uint32 maxHp]
                                //                         [float x][float y][float z][float orientation]
    SMSG_DESPAWN_PLAYER,        // 다른 플레이어 디스폰 알림[uint64 pawnId]
    _SPAWN_END,                 // 더미 — 범위 검사용
};

// ─────────────────────────────────────────────────────────────────────────────
// 패킷 직렬화 / 역직렬화 예시
//
// [사용 예시 — 핸들러 등록]
//
//   net.RegisterHandler(
//       static_cast<uint16_t>(CMSG_LOGIN),
//       [this](std::shared_ptr<Session> s, PacketReader& r) {
//           OnLogin(s, r);
//       });
//
// [CMSG_LOGIN 역직렬화]
//
//   void OnLogin(std::shared_ptr<Session> session, PacketReader& r)
//   {
//       auto username = r.ReadString();   // std::string
//       auto token    = r.Read<uint64_t>();
//       // ... 처리 ...
//   }
//
// [SMSG_LOGIN_RESULT 직렬화 & 전송]
//
//   PacketWriter w(static_cast<uint16_t>(SMSG_LOGIN_RESULT));
//   w.Write<uint8_t>(1);      // 1 = 성공
//   w.Write<uint64_t>(sessionId);
//   session->Send(w.Finalize());
//
// [CMSG_MOVE 역직렬화]
//
//   void OnMove(std::shared_ptr<Session> session, PacketReader& r)
//   {
//       float x = r.Read<float>();
//       float y = r.Read<float>();
//       float z = r.Read<float>();
//       // ... Zone 에 전달 ...
//   }
//
// [SMSG_MOVE_BROADCAST 직렬화 & Zone 브로드캐스트]
//
//   PacketWriter w(static_cast<uint16_t>(SMSG_MOVE_BROADCAST));
//   w.Write<uint64_t>(movedSessionId);
//   w.Write<float>(x);
//   w.Write<float>(y);
//   w.Write<float>(z);
//   zone.BroadcastTCP(w.Finalize(), excludeSessionId);
// ─────────────────────────────────────────────────────────────────────────────
