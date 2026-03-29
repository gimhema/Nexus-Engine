#pragma once

#include "../../../Packets/PacketBase.h"

// ─────────────────────────────────────────────────────────────────────────────
// Opcode 정의
//
// CMSG_ : Client → Server
// SMSG_ : Server → Client
// ─────────────────────────────────────────────────────────────────────────────
enum class Opcode : uint16_t
{
    // ── 인증 ──────────────────────────────────────────────────────────────────
    CMSG_LOGIN          = 0x0001,   // 로그인 요청
    SMSG_LOGIN_RESULT   = 0x0002,   // 로그인 결과

    // ── 이동 (TCP) ────────────────────────────────────────────────────────────
    CMSG_MOVE           = 0x0101,   // 이동 요청
    SMSG_MOVE_BROADCAST = 0x0102,   // 주변 플레이어 이동 브로드캐스트

    // ── 이동 (UDP) ────────────────────────────────────────────────────────────
    CMSG_MOVE_UDP       = 0x0111,   // 이동 요청 (UDP, 빠른 위치 동기화)
    SMSG_MOVE_UDP       = 0x0112,   // 이동 브로드캐스트 (UDP)

    // ── 채팅 ──────────────────────────────────────────────────────────────────
    CMSG_CHAT           = 0x0201,   // 채팅 메시지
    SMSG_CHAT           = 0x0202,   // 채팅 브로드캐스트
};

// ─────────────────────────────────────────────────────────────────────────────
// 패킷 직렬화 / 역직렬화 예시
//
// [사용 예시 — 핸들러 등록]
//
//   net.RegisterHandler(
//       static_cast<uint16_t>(Opcode::CMSG_LOGIN),
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
//   PacketWriter w(static_cast<uint16_t>(Opcode::SMSG_LOGIN_RESULT));
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
//   PacketWriter w(static_cast<uint16_t>(Opcode::SMSG_MOVE_BROADCAST));
//   w.Write<uint64_t>(movedSessionId);
//   w.Write<float>(x);
//   w.Write<float>(y);
//   w.Write<float>(z);
//   zone.BroadcastTCP(w.Finalize(), excludeSessionId);
// ─────────────────────────────────────────────────────────────────────────────
