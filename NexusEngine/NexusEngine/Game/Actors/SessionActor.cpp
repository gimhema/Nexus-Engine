#include "SessionActor.h"
#include "WorldActor.h"
#include "ZoneActor.h"

#include "../../Connection.h"
#include "../../Shared/Logger.h"
#include "../../protocol_shared/Packets/Packet-Auth.h"
#include "../../protocol_shared/Packets/Packet-Movement.h"
#include "../../protocol_shared/Packets/Packet-Chat.h"
#include "../../protocol_shared/Packets/Packet-Combat.h"

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
    NexusPacketParser r{ msg.payload.data(), static_cast<uint32_t>(msg.payload.size()) };

    switch (msg.opcode)
    {
    case CMSG_LOGIN:
    {
        auto pkt = CMsg_Login::Decode(r);
        MsgSession_Login login;
        login.sessionId   = m_sessionId;
        login.accountName = std::move(pkt.accountName);
        login.token       = std::move(pkt.token);
        m_world.Post(std::move(login));
        break;
    }
    case CMSG_CHAR_SETUP:
    {
        auto pkt = CMsg_CharSetup::Decode(r);
        MsgSession_CharSetup charSetup;
        charSetup.sessionId           = m_sessionId;
        charSetup.setup.characterName = std::move(pkt.characterName);
        m_world.Post(std::move(charSetup));
        break;
    }
    case CMSG_ENTER_WORLD:
    {
        auto pkt = CMsg_EnterWorld::Decode(r);
        MsgSession_EnterWorld enter;
        enter.sessionId   = m_sessionId;
        enter.characterId = pkt.characterId;
        m_world.Post(std::move(enter));
        break;
    }
    case CMSG_MOVE:
    {
        auto* zone = m_zone.load(std::memory_order_acquire);
        if (!zone) break;
        auto pkt = CMsg_Move::Decode(r);
        MsgSession_Move move;
        move.sessionId   = m_sessionId;
        move.pos         = { pkt.x, pkt.y, pkt.z };
        move.orientation = pkt.orientation;
        zone->Post(std::move(move));
        break;
    }
    case CMSG_MOVE_UDP:
    {
        auto* zone = m_zone.load(std::memory_order_acquire);
        if (!zone) break;
        auto pkt = CMsg_MoveUdp::Decode(r);
        MsgSession_MoveUdp move;
        move.sessionId   = m_sessionId;
        move.pos         = { pkt.x, pkt.y, pkt.z };
        move.orientation = pkt.orientation;
        zone->Post(std::move(move));
        break;
    }
    case CMSG_CHAT:
    {
        auto* zone = m_zone.load(std::memory_order_acquire);
        if (!zone) break;
        auto pkt = CMsg_Chat::Decode(r);
        MsgSession_Chat chat;
        chat.sessionId = m_sessionId;
        chat.text      = std::move(pkt.text);
        zone->Post(std::move(chat));
        break;
    }
    case CMSG_WORLD_CHAT:
    {
        auto pkt = CMsg_WorldChat::Decode(r);
        MsgSession_WorldChat worldChat;
        worldChat.sessionId = m_sessionId;
        worldChat.text      = std::move(pkt.text);
        m_world.Post(std::move(worldChat));
        break;
    }
    case CMSG_USE_SKILL:
    {
        auto* zone = m_zone.load(std::memory_order_acquire);
        if (!zone) break;
        auto pkt = CMsg_UseSkill::Decode(r);
        MsgSession_UseSkill skill;
        skill.sessionId       = m_sessionId;
        skill.skillId         = pkt.skillId;
        skill.targetPawnId    = pkt.targetPawnId;
        skill.targetPos       = { pkt.targetX, pkt.targetY, pkt.targetZ };
        skill.clientTimestamp = pkt.clientTimestamp;
        zone->Post(std::move(skill));
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
    if (m_session && m_session->IsConnected())
        m_session->Send(SMsg_LoginResult{ .success = msg.success, .message = msg.message }.Encode());
}

void SessionActor::Handle(MsgWorld_CharSetupResult& msg)
{
    if (m_session && m_session->IsConnected())
        m_session->Send(SMsg_CharSetupResult{
            .success     = msg.success,
            .characterId = msg.characterId,
            .message     = msg.message
        }.Encode());
}
