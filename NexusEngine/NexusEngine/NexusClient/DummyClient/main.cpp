#include <NetClient.h>
#include <Packets/PacketBase.h>
#include "Packets/GamePackets.hpp"
#include "../../protocol_shared/PacketParser.h"

#include <atomic>
#include <chrono>
#include <cstdio>
#include <csignal>
#include <thread>

// ─────────────────────────────────────────────────────────────────────────────
// 접속 테스트 시나리오
//
//   1. 서버 접속
//   2. CMSG_LOGIN              → SMSG_LOGIN_RESULT 대기
//   3. CMSG_CHAR_SETUP         → SMSG_CHAR_SETUP_RESULT 대기 (서버 발급 characterId 수신)
//   4. CMSG_ENTER_WORLD        → SMSG_ENTER_WORLD 대기
//   5. CMSG_CHAT               (존 채팅 — 같은 존 플레이어에게만)
//   6. CMSG_WORLD_CHAT         (월드 채팅 — 전 서버 브로드캐스트)
//   7. CMSG_MOVE               (임의 위치로 이동)
//   8. 3초 대기 후 연결 해제
// ─────────────────────────────────────────────────────────────────────────────

static constexpr const char* SERVER_HOST   = "127.0.0.1";
static constexpr uint16_t    SERVER_PORT   = 7070;
static constexpr const char* ACCOUNT_NAME  = "test_user";
static constexpr const char* LOGIN_TOKEN   = "dummy_token";
static constexpr const char* CHAR_NAME     = "TestHero";

std::atomic<bool>     g_running{ true };
std::atomic<uint32_t> g_characterId{ 0 };   // SMSG_CHAR_SETUP_RESULT에서 수신한 서버 발급 ID

// ─────────────────────────────────────────────────────────────────────────────
// 송신 헬퍼
// ─────────────────────────────────────────────────────────────────────────────

void SendLogin(NetClient& client)
{
    ClientPacketWriter w(CMSG_LOGIN);
    w.WriteString(ACCOUNT_NAME);
    w.WriteString(LOGIN_TOKEN);
    client.Send(w.Finalize());
    std::printf("[→] CMSG_LOGIN  account=%s\n", ACCOUNT_NAME);
}

void SendCharSetup(NetClient& client)
{
    ClientPacketWriter w(CMSG_CHAR_SETUP);
    w.WriteString(CHAR_NAME);
    client.Send(w.Finalize());
    std::printf("[→] CMSG_CHAR_SETUP  name=%s\n", CHAR_NAME);
}

void SendEnterWorld(NetClient& client, uint32_t characterId)
{
    ClientPacketWriter w(CMSG_ENTER_WORLD);
    w.WriteUInt32(characterId);
    client.Send(w.Finalize());
    std::printf("[→] CMSG_ENTER_WORLD  characterId=%u\n", characterId);
}

void SendZoneChat(NetClient& client, const std::string& text)
{
    ClientPacketWriter w(CMSG_CHAT);
    w.WriteString(text);
    client.Send(w.Finalize());
    std::printf("[→] CMSG_CHAT (존)  text=\"%s\"\n", text.c_str());
}

void SendWorldChat(NetClient& client, const std::string& text)
{
    ClientPacketWriter w(CMSG_WORLD_CHAT);
    w.WriteString(text);
    client.Send(w.Finalize());
    std::printf("[→] CMSG_WORLD_CHAT  text=\"%s\"\n", text.c_str());
}

void SendMove(NetClient& client, float x, float y, float z, float orientation = 0.f)
{
    ClientPacketWriter w(CMSG_MOVE);
    w.WriteFloat(x).WriteFloat(y).WriteFloat(z).WriteFloat(orientation);
    client.Send(w.Finalize());
    std::printf("[→] CMSG_MOVE  pos=(%.1f, %.1f, %.1f) o=%.2f\n", x, y, z, orientation);
}


// ─────────────────────────────────────────────────────────────────────────────
int main()
{
    std::printf("=== NexusClient DummyClient ===\n");
    std::printf("서버 접속 시도: %s:%u\n\n", SERVER_HOST, SERVER_PORT);

    NetClient client;

    // ── 연결 콜백 ─────────────────────────────────────────────────────────────
    client.SetOnConnected([&client]()
    {
        std::printf("[연결] 서버 접속 성공\n");
        SendLogin(client);
    });

    client.SetOnDisconnected([]()
    {
        std::printf("[연결 해제] 서버와의 연결이 끊어졌습니다.\n");
        g_running.store(false);
    });

    // ── 수신 패킷 핸들러 ──────────────────────────────────────────────────────
    client.SetOnPacket([&client](uint16_t opcode, std::vector<uint8_t> payload)
    {
        NexusPacketParser r{ payload.data(), static_cast<uint32_t>(payload.size()) };

        switch (static_cast<Opcode>(opcode))
        {
        // ── 인증 / 접속 ───────────────────────────────────────────────────────
        case SMSG_LOGIN_RESULT:
        {
            const uint8_t     success = r.ReadU8();
            const std::string message = r.ReadString();
            std::printf("[←] SMSG_LOGIN_RESULT  success=%u  message=\"%s\"\n",
                        success, message.c_str());
            if (success)
                SendCharSetup(client);          // 로그인 성공 → 캐릭터 설정 단계로
            else
                g_running.store(false);
            break;
        }
        case SMSG_CHAR_SETUP_RESULT:
        {
            const uint8_t     success     = r.ReadU8();
            const uint32_t    characterId = r.ReadU32();
            const std::string message     = r.ReadString();
            std::printf("[←] SMSG_CHAR_SETUP_RESULT  success=%u  characterId=%u  message=\"%s\"\n",
                        success, characterId, message.c_str());
            if (success)
            {
                g_characterId.store(characterId);
                SendEnterWorld(client, characterId);   // 서버 발급 ID로 월드 진입
            }
            else
                g_running.store(false);
            break;
        }
        case SMSG_ENTER_WORLD:
        {
            const uint8_t     success     = r.ReadU8();
            const uint64_t    pawnId      = r.ReadU64();
            const uint32_t    characterId = r.ReadU32();
            const std::string name        = r.ReadString();
            const uint32_t    hp          = r.ReadU32();
            const uint32_t    maxHp       = r.ReadU32();
            const float       x           = r.ReadFloat();
            const float       y           = r.ReadFloat();
            const float       z           = r.ReadFloat();
            const float       orientation = r.ReadFloat();

            std::printf("[←] SMSG_ENTER_WORLD  success=%u  pawnId=%llu  charId=%u  name=%s\n"
                        "                      hp=%u/%u  pos=(%.1f, %.1f, %.1f)  o=%.2f\n",
                        success,
                        static_cast<unsigned long long>(pawnId),
                        characterId, name.c_str(),
                        hp, maxHp, x, y, z, orientation);

            if (success)
            {
                SendZoneChat(client, "안녕하세요! (존 채팅)");
                SendWorldChat(client, "안녕하세요! (월드 채팅)");
                SendMove(client, 10.f, 0.f, 5.f);
            }
            break;
        }

        // ── 채팅 ──────────────────────────────────────────────────────────────
        case SMSG_CHAT:
        {
            const uint64_t    sessionId = r.ReadU64();
            const std::string name      = r.ReadString();
            const std::string text      = r.ReadString();
            std::printf("[←] SMSG_CHAT (존)  [%s] %s  (sessionId=%llu)\n",
                        name.c_str(), text.c_str(),
                        static_cast<unsigned long long>(sessionId));
            break;
        }
        case SMSG_WORLD_CHAT:
        {
            const uint64_t    sessionId = r.ReadU64();
            const std::string name      = r.ReadString();
            const std::string text      = r.ReadString();
            std::printf("[←] SMSG_WORLD_CHAT  [%s] %s  (sessionId=%llu)\n",
                        name.c_str(), text.c_str(),
                        static_cast<unsigned long long>(sessionId));
            break;
        }

        // ── 이동 ──────────────────────────────────────────────────────────────
        case SMSG_MOVE_BROADCAST:
        {
            const uint64_t sid         = r.ReadU64();
            const float    x           = r.ReadFloat();
            const float    y           = r.ReadFloat();
            const float    z           = r.ReadFloat();
            const float    orientation = r.ReadFloat();
            std::printf("[←] SMSG_MOVE_BROADCAST  sessionId=%llu  pos=(%.1f, %.1f, %.1f)  o=%.2f\n",
                        static_cast<unsigned long long>(sid), x, y, z, orientation);
            break;
        }
        case SMSG_MOVE_UDP:
        {
            const uint64_t sid         = r.ReadU64();
            const float    x           = r.ReadFloat();
            const float    y           = r.ReadFloat();
            const float    z           = r.ReadFloat();
            const float    orientation = r.ReadFloat();
            std::printf("[←] SMSG_MOVE_UDP  sessionId=%llu  pos=(%.1f, %.1f, %.1f)  o=%.2f\n",
                        static_cast<unsigned long long>(sid), x, y, z, orientation);
            break;
        }

        // ── 스폰 / 디스폰 ─────────────────────────────────────────────────────
        case SMSG_SPAWN_PLAYER:
        {
            const uint64_t    pawnId      = r.ReadU64();
            const uint64_t    sessionId   = r.ReadU64();
            const std::string name        = r.ReadString();
            const uint32_t    hp          = r.ReadU32();
            const uint32_t    maxHp       = r.ReadU32();
            const float       x           = r.ReadFloat();
            const float       y           = r.ReadFloat();
            const float       z           = r.ReadFloat();
            const float       orientation = r.ReadFloat();
            std::printf("[←] SMSG_SPAWN_PLAYER  pawnId=%llu  sessionId=%llu  name=%s\n"
                        "                       hp=%u/%u  pos=(%.1f, %.1f, %.1f)  o=%.2f\n",
                        static_cast<unsigned long long>(pawnId),
                        static_cast<unsigned long long>(sessionId),
                        name.c_str(), hp, maxHp, x, y, z, orientation);
            break;
        }
        case SMSG_DESPAWN_PLAYER:
        {
            const uint64_t pawnId = r.ReadU64();
            std::printf("[←] SMSG_DESPAWN_PLAYER  pawnId=%llu\n",
                        static_cast<unsigned long long>(pawnId));
            break;
        }

        default:
            std::printf("[←] 알 수 없는 opcode=0x%04x  payloadSize=%u\n",
                        opcode, r.Remaining() + r.pos);
            break;
        }
    });

    // ── 서버 접속 ─────────────────────────────────────────────────────────────
    if (!client.Connect(SERVER_HOST, SERVER_PORT))
    {
        std::printf("[오류] 서버 접속 실패: %s:%u\n", SERVER_HOST, SERVER_PORT);
        return 1;
    }

    // ── Ctrl+C / SIGTERM 시그널 핸들러 ───────────────────────────────────────
    std::signal(SIGINT,  [](int) { g_running.store(false); });
    std::signal(SIGTERM, [](int) { g_running.store(false); });

    std::printf("접속 유지 중 — 종료하려면 Ctrl+C\n\n");

    // ── 서버 연결이 끊기거나 Ctrl+C 까지 대기 ────────────────────────────────
    while (g_running.load())
        std::this_thread::sleep_for(std::chrono::milliseconds(100));

    std::printf("\n종료 — 연결 해제 중...\n");
    client.Disconnect();
    return 0;
}
