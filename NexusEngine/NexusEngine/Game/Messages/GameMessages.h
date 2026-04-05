#pragma once

#include <cstdint>
#include <string>
#include <vector>
#include <variant>
#include <memory>

// 전방 선언
class Session;
class SessionActor;

// ─────────────────────────────────────────────────────────────────────────────
// GameMessages.h
//
// 서버 내 모든 Actor 간 메시지 타입 정의.
// 메시지는 값 타입 (복사/이동 가능). shared_ptr은 최소화.
//
// 네이밍 규칙:
//   Msg<송신자><수신자>_<동작>
//   예) MsgNetZone_PlayerMove  — 네트워크→ZoneActor 플레이어 이동
//       MsgZoneSession_Broadcast — ZoneActor→SessionActor 브로드캐스트
// ─────────────────────────────────────────────────────────────────────────────

// ─────────────────────────────────────────────────────────────────────────────
// 공용 타입
// ─────────────────────────────────────────────────────────────────────────────
struct Vec3
{
    float x{ 0.f }, y{ 0.f }, z{ 0.f };
};

// ─────────────────────────────────────────────────────────────────────────────
// Network Worker → SessionActor
// (네트워크 레이어가 수신한 raw 패킷을 SessionActor에 전달)
// ─────────────────────────────────────────────────────────────────────────────
struct MsgNet_PacketReceived
{
    uint16_t             opcode{};
    std::vector<uint8_t> payload;   // opcode 이후 페이로드 (헤더 제외)
};

// ─────────────────────────────────────────────────────────────────────────────
// SessionActor → ZoneActor
// ─────────────────────────────────────────────────────────────────────────────

// 플레이어가 존에 진입 요청 (로그인 후 WorldActor 경유로 전달됨)
struct MsgSession_EnterZone
{
    uint64_t    sessionId{};
    uint32_t    characterId{};
    std::string characterName;
    Vec3        spawnPos;
};

// 플레이어 이동 (TCP)
struct MsgSession_Move
{
    uint64_t sessionId{};
    Vec3     pos;
    float    orientation{ 0.f };   // 라디안
    uint32_t movementFlags{ 0 };
};

// 플레이어 이동 (UDP — 고빈도)
struct MsgSession_MoveUdp
{
    uint64_t sessionId{};
    Vec3     pos;
    float    orientation{ 0.f };
};

// 플레이어가 채팅 메시지 전송
struct MsgSession_Chat
{
    uint64_t    sessionId{};
    std::string text;
};

// 플레이어가 존을 떠남 (연결 끊김 또는 텔레포트)
struct MsgSession_LeaveZone
{
    uint64_t sessionId{};
};

// ─────────────────────────────────────────────────────────────────────────────
// ZoneActor → SessionActor
// ─────────────────────────────────────────────────────────────────────────────

// 직렬화된 패킷을 TCP로 전송
struct MsgZone_SendTcp
{
    std::vector<uint8_t> packet;
};

// 직렬화된 패킷을 UDP로 전송
struct MsgZone_SendUdp
{
    std::vector<uint8_t> packet;
};

// SessionActor 종료 요청 (서버 강제 킥 등)
struct MsgZone_Disconnect
{
    std::string reason;
};

// ─────────────────────────────────────────────────────────────────────────────
// SessionActor → WorldActor
// ─────────────────────────────────────────────────────────────────────────────

// 로그인 요청
struct MsgSession_Login
{
    uint64_t    sessionId{};
    std::string accountName;
    std::string token;
};

// 캐릭터 선택 후 월드 진입
struct MsgSession_EnterWorld
{
    uint64_t sessionId{};
    uint32_t characterId{};
};

// 세션 종료 알림
struct MsgSession_Logout
{
    uint64_t sessionId{};
};

// ─────────────────────────────────────────────────────────────────────────────
// WorldActor → SessionActor
// ─────────────────────────────────────────────────────────────────────────────

struct MsgWorld_LoginResult
{
    bool        success{ false };
    std::string message;
};

// ─────────────────────────────────────────────────────────────────────────────
// WorldActor → ZoneActor
// ─────────────────────────────────────────────────────────────────────────────

// 플레이어를 존에 추가하도록 지시
struct MsgWorld_AddPlayer
{
    uint64_t    sessionId{};
    uint32_t    characterId{};
    std::string characterName;
    Vec3        spawnPos;
    std::weak_ptr<SessionActor> sessionActor;  // ZoneActor 내부 등록용 (ZoneActor→SessionActor 직접 호출 대체)
};

// 플레이어를 존에서 제거하도록 지시
struct MsgWorld_RemovePlayer
{
    uint64_t sessionId{};
};

// ─────────────────────────────────────────────────────────────────────────────
// ZoneActor → WorldActor
// ─────────────────────────────────────────────────────────────────────────────

// 텔레포트 요청 (다른 존으로 이동)
struct MsgZone_TeleportRequest
{
    uint64_t sessionId{};
    uint32_t targetZoneId{};
    Vec3     targetPos;
};

// ─────────────────────────────────────────────────────────────────────────────
// Variant 타입 별칭
// (각 Actor의 Message 타입으로 사용)
// ─────────────────────────────────────────────────────────────────────────────

// SessionActor가 받는 메시지
using SessionMessage = std::variant<
    MsgNet_PacketReceived,
    MsgZone_SendTcp,
    MsgZone_SendUdp,
    MsgZone_Disconnect,
    MsgWorld_LoginResult
>;

// ZoneActor가 받는 메시지
using ZoneMessage = std::variant<
    MsgSession_EnterZone,
    MsgSession_Move,
    MsgSession_MoveUdp,
    MsgSession_Chat,
    MsgSession_LeaveZone,
    MsgWorld_AddPlayer,
    MsgWorld_RemovePlayer
>;

// ─────────────────────────────────────────────────────────────────────────────
// Server 인프라 → WorldActor (내부 관리용, 게임 로직과 무관)
// ─────────────────────────────────────────────────────────────────────────────

// 새 SessionActor 등록 — 네트워크 워커 스레드 대신 Post()로 전달해 m_sessions 레이스 방지
struct MsgServer_RegisterSession
{
    uint64_t                     sessionId{};
    std::shared_ptr<SessionActor> actor;
};

// SessionActor 등록 해제
struct MsgServer_UnregisterSession
{
    uint64_t sessionId{};
};

// WorldActor가 받는 메시지
using WorldMessage = std::variant<
    MsgSession_Login,
    MsgSession_EnterWorld,
    MsgSession_Logout,
    MsgZone_TeleportRequest,
    MsgServer_RegisterSession,
    MsgServer_UnregisterSession
>;
