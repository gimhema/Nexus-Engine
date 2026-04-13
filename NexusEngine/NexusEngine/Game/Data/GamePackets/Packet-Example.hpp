#pragma once

#include "../../../Packets/PacketBase.h"
#include "../../../protocol_shared/Opcodes.h"

// ─────────────────────────────────────────────────────────────────────────────
// Packet-Example.hpp — 서버 전용 패킷 직렬화/역직렬화 예시 및 편의 매크로
//
// Opcode 정의는 protocol_shared/Opcodes.h 에 위치한다.
// 해당 파일은 UE5 클라이언트와 공용으로 사용되므로 서버 전용 내용을 추가하지 않는다.
//
// 서버 전용 그룹 범위 체크 (레거시 호환 — 신규 코드는 NEXUS_OPCODE_IN 사용 권장)
// ─────────────────────────────────────────────────────────────────────────────
#define OPCODE_IN(op, group) NEXUS_OPCODE_IN(op, group)

// ─────────────────────────────────────────────────────────────────────────────
// 패킷 직렬화 / 역직렬화 예시
//
// [CMSG_LOGIN 역직렬화]
//
//   void OnLogin(std::shared_ptr<Session> session, PacketReader& r)
//   {
//       auto accountName = r.ReadString();
//       auto token       = r.ReadString();
//   }
//
// [SMSG_LOGIN_RESULT 직렬화 & 전송]
//
//   PacketWriter w(SMSG_LOGIN_RESULT);
//   w.WriteUInt8(1);          // 1 = 성공
//   w.WriteString("OK");
//   session->Send(w.Finalize());
//
// [CMSG_CHAR_SETUP 역직렬화]
//
//   void OnCharSetup(PacketReader& r)
//   {
//       auto characterName = r.ReadString();
//   }
//
// [SMSG_CHAR_SETUP_RESULT 직렬화 & 전송]
//
//   PacketWriter w(SMSG_CHAR_SETUP_RESULT);
//   w.WriteUInt8(1);              // 1 = 성공
//   w.WriteUInt32(characterId);   // 서버 발급 ID
//   w.WriteString("OK");
//   session->Send(w.Finalize());
//
// [CMSG_MOVE 역직렬화]
//
//   float x = r.ReadFloat();
//   float y = r.ReadFloat();
//   float z = r.ReadFloat();
//   float o = r.ReadFloat();
//
// [SMSG_MOVE_BROADCAST 직렬화 & Zone 브로드캐스트]
//
//   PacketWriter w(SMSG_MOVE_BROADCAST);
//   w.WriteUInt64(sessionId);
//   w.WriteFloat(x).WriteFloat(y).WriteFloat(z).WriteFloat(orientation);
//   zone.BroadcastTcp(excludeSessionId, w.Finalize());
// ─────────────────────────────────────────────────────────────────────────────
