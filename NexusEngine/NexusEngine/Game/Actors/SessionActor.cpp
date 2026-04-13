#include "SessionActor.h"
#include "WorldActor.h"
#include "ZoneActor.h"

#include "../../Connection.h"
#include "../../Packets/PacketBase.h"
#include "../../Shared/Logger.h"
#include "../Data/GamePackets/Packet-Example.hpp"

SessionActor::SessionActor(std::shared_ptr<Session> session,
                           WorldActor&              world)
    : m_session(std::move(session))
    , m_world(world)
{
    m_sessionId = m_session->GetSessionId();
}

void SessionActor::OnMessage(SessionMessage& msg)
{
    std::visit(overloaded{
        [this](MsgNet_PacketReceived& m)    { Handle(m); },
        [this](MsgZone_SendTcp& m)          { Handle(m); },
        [this](MsgZone_SendUdp& m)          { Handle(m); },
        [this](MsgZone_Disconnect& m)       { Handle(m); },
        [this](MsgWorld_LoginResult& m)     { Handle(m); },
        [this](MsgWorld_CharSetupResult& m) { Handle(m); },
    }, msg);
}

// ─────────────────────────────────────────────────────────────────────────────
// 수신 패킷 파싱 → 상위 Actor에 메시지 전달
// ─────────────────────────────────────────────────────────────────────────────
void SessionActor::Handle(MsgNet_PacketReceived& msg)
{
    PacketReader reader = PacketReader::FromPayload(
        msg.opcode,
        msg.payload.data(),
        static_cast<uint32_t>(msg.payload.size()));

    switch (msg.opcode)
    {
    case CMSG_LOGIN:
    {
        MsgSession_Login login;
        login.sessionId   = m_sessionId;
        login.accountName = reader.ReadString();
        login.token       = reader.ReadString();
        m_world.Post(std::move(login));
        break;
    }
    case CMSG_CHAR_SETUP:
    {
        MsgSession_CharSetup charSetup;
        charSetup.sessionId            = m_sessionId;
        charSetup.setup.characterName  = reader.ReadString();
        m_world.Post(std::move(charSetup));
        break;
    }
    case CMSG_ENTER_WORLD:
    {
        MsgSession_EnterWorld enter;
        enter.sessionId    = m_sessionId;
        enter.characterId  = reader.ReadUInt32();
        m_world.Post(std::move(enter));
        break;
    }
    case CMSG_MOVE:
    {
        auto* zone = m_zone.load(std::memory_order_acquire);
        if (!zone) break;
        MsgSession_Move move;
        move.sessionId   = m_sessionId;
        move.pos.x       = reader.ReadFloat();
        move.pos.y       = reader.ReadFloat();
        move.pos.z       = reader.ReadFloat();
        move.orientation = reader.ReadFloat();
        zone->Post(std::move(move));
        break;
    }
    case CMSG_MOVE_UDP:
    {
        auto* zone = m_zone.load(std::memory_order_acquire);
        if (!zone) break;
        MsgSession_MoveUdp move;
        move.sessionId   = m_sessionId;
        move.pos.x       = reader.ReadFloat();
        move.pos.y       = reader.ReadFloat();
        move.pos.z       = reader.ReadFloat();
        move.orientation = reader.ReadFloat();
        zone->Post(std::move(move));
        break;
    }
    case CMSG_CHAT:
    {
        auto* zone = m_zone.load(std::memory_order_acquire);
        if (!zone) break;
        MsgSession_Chat chat;
        chat.sessionId = m_sessionId;
        chat.text      = reader.ReadString();
        zone->Post(std::move(chat));
        break;
    }
    case CMSG_WORLD_CHAT:
    {
        MsgSession_WorldChat worldChat;
        worldChat.sessionId = m_sessionId;
        worldChat.text      = reader.ReadString();
        m_world.Post(std::move(worldChat));
        break;
    }
    default:
        LOG_WARN("SessionActor: 알 수 없는 opcode={:#06x} sessionId={}", msg.opcode, m_sessionId);
        break;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 송신
// ─────────────────────────────────────────────────────────────────────────────
void SessionActor::Handle(MsgZone_SendTcp& msg)
{
    if (m_session && m_session->IsConnected())
        m_session->Send(msg.packet);
}

void SessionActor::Handle(MsgZone_SendUdp& msg)
{
    // UDP 송신은 NetworkManager::SendUDP 경유 — 추후 구현
    // 현재는 TCP fallback
    if (m_session && m_session->IsConnected())
        m_session->Send(msg.packet);
}

void SessionActor::Handle(MsgZone_Disconnect& msg)
{
    LOG_INFO("SessionActor: 킥 sessionId={} reason={}", m_sessionId, msg.reason);
    if (m_session)
        m_session->SetState(SessionState::Closing);
}

void SessionActor::Handle(MsgWorld_LoginResult& msg)
{
    PacketWriter w(SMSG_LOGIN_RESULT);
    w.WriteUInt8(msg.success ? 1u : 0u);
    w.WriteString(msg.message);
    const auto& buf = w.Finalize();
    if (m_session && m_session->IsConnected())
        m_session->Send(buf);
}

void SessionActor::Handle(MsgWorld_CharSetupResult& msg)
{
    PacketWriter w(SMSG_CHAR_SETUP_RESULT);
    w.WriteUInt8(msg.success ? 1u : 0u);
    w.WriteString(msg.message);
    const auto& buf = w.Finalize();
    if (m_session && m_session->IsConnected())
        m_session->Send(buf);
}
