#include <NetClient.h>
#include <Packets/PacketBase.h>
#include "Packets/GamePackets.hpp"

#include <atomic>
#include <chrono>
#include <cstdio>
#include <thread>

// ─────────────────────────────────────────────────────────────────────────────
// 접속 테스트 시나리오
//
//   1. 서버 접속
//   2. CMSG_LOGIN       → SMSG_LOGIN_RESULT 대기
//   3. CMSG_ENTER_WORLD → SMSG_ENTER_WORLD  대기
//   4. CMSG_CHAT        (입장 메시지)
//   5. CMSG_MOVE        (임의 위치로 이동)
//   6. 3초 대기 후 연결 해제
// ─────────────────────────────────────────────────────────────────────────────

static constexpr const char*   SERVER_HOST    = "127.0.0.1";
static constexpr uint16_t      SERVER_PORT    = 7070;
static constexpr uint32_t      CHARACTER_ID   = 1;
static constexpr const char*   ACCOUNT_NAME   = "test_user";
static constexpr const char*   LOGIN_TOKEN    = "dummy_token";

std::atomic<bool> g_running{ true };

void SendLogin(NetClient& client)
{
    ClientPacketWriter w(CMSG_LOGIN);
    w.WriteString(ACCOUNT_NAME);
    w.WriteString(LOGIN_TOKEN);
    client.Send(w.Finalize());
    std::printf("[→] CMSG_LOGIN  account=%s\n", ACCOUNT_NAME);
}

void SendEnterWorld(NetClient& client)
{
    ClientPacketWriter w(CMSG_ENTER_WORLD);
    w.WriteUInt32(CHARACTER_ID);
    client.Send(w.Finalize());
    std::printf("[→] CMSG_ENTER_WORLD  characterId=%u\n", CHARACTER_ID);
}

void SendChat(NetClient& client, const std::string& text)
{
    ClientPacketWriter w(CMSG_CHAT);
    w.WriteString(text);
    client.Send(w.Finalize());
    std::printf("[→] CMSG_CHAT  text=\"%s\"\n", text.c_str());
}

void SendMove(NetClient& client, float x, float y, float z)
{
    ClientPacketWriter w(CMSG_MOVE);
    w.WriteFloat(x).WriteFloat(y).WriteFloat(z).WriteFloat(0.f);
    client.Send(w.Finalize());
    std::printf("[→] CMSG_MOVE  pos=(%.1f, %.1f, %.1f)\n", x, y, z);
}

int main()
{
    std::printf("=== NexusClient DummyClient ===\n");
    std::printf("서버 접속 시도: %s:%u\n\n", SERVER_HOST, SERVER_PORT);

    NetClient client;

    // ── 콜백 등록 ──────────────────────────────────────────────────────────
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

    client.SetOnPacket([&client](uint16_t opcode, std::vector<uint8_t> payload)
    {
        ClientPacketReader reader(payload.data(), static_cast<uint32_t>(payload.size()) + CLIENT_PACKET_HEADER_SIZE);

        // SetOnPacket의 payload는 헤더 제외 바이트이므로
        // 직접 역직렬화를 위해 raw pointer + size로 읽음
        const uint8_t* data = payload.data();
        uint32_t       size = static_cast<uint32_t>(payload.size());
        uint32_t       pos  = 0;

        auto readU8 = [&]() -> uint8_t {
            return (pos < size) ? data[pos++] : 0;
        };
        auto readFloat = [&]() -> float {
            float v{}; if (pos + 4 <= size) { std::memcpy(&v, data + pos, 4); pos += 4; } return v;
        };
        auto readString = [&]() -> std::string {
            if (pos + 2 > size) return {};
            uint16_t len{}; std::memcpy(&len, data + pos, 2); pos += 2;
            if (pos + len > size) return {};
            std::string s(reinterpret_cast<const char*>(data + pos), len); pos += len; return s;
        };
        auto readU64 = [&]() -> uint64_t {
            uint64_t v{}; if (pos + 8 <= size) { std::memcpy(&v, data + pos, 8); pos += 8; } return v;
        };

        switch (static_cast<Opcode>(opcode))
        {
        case SMSG_LOGIN_RESULT:
        {
            uint8_t success = readU8();
            std::string msg = readString();
            std::printf("[←] SMSG_LOGIN_RESULT  success=%u message=\"%s\"\n",
                        success, msg.c_str());
            if (success)
                SendEnterWorld(client);
            else
                g_running.store(false);
            break;
        }
        case SMSG_ENTER_WORLD:
        {
            uint8_t success = readU8();
            float x = readFloat(), y = readFloat(), z = readFloat();
            std::printf("[←] SMSG_ENTER_WORLD  success=%u pos=(%.1f, %.1f, %.1f)\n",
                        success, x, y, z);
            if (success)
            {
                SendChat(client, "안녕하세요!");
                SendMove(client, 10.f, 0.f, 5.f);
            }
            break;
        }
        case SMSG_CHAT:
        {
            uint64_t    sid  = readU64();
            std::string name = readString();
            std::string text = readString();
            std::printf("[←] SMSG_CHAT  [%s] %s  (sessionId=%llu)\n",
                        name.c_str(), text.c_str(),
                        static_cast<unsigned long long>(sid));
            break;
        }
        case SMSG_MOVE_BROADCAST:
        {
            uint64_t sid = readU64();
            float x = readFloat(), y = readFloat(), z = readFloat(), o = readFloat();
            std::printf("[←] SMSG_MOVE_BROADCAST  sessionId=%llu pos=(%.1f,%.1f,%.1f) o=%.2f\n",
                        static_cast<unsigned long long>(sid), x, y, z, o);
            break;
        }
        default:
            std::printf("[←] 알 수 없는 opcode=0x%04x  payloadSize=%u\n",
                        opcode, size);
            break;
        }
    });

    // ── 서버 접속 ──────────────────────────────────────────────────────────
    if (!client.Connect(SERVER_HOST, SERVER_PORT))
    {
        std::printf("[오류] 서버 접속 실패: %s:%u\n", SERVER_HOST, SERVER_PORT);
        return 1;
    }

    // ── 3초 대기 후 종료 ───────────────────────────────────────────────────
    auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(3);
    while (g_running.load() &&
           std::chrono::steady_clock::now() < deadline)
    {
        std::this_thread::sleep_for(std::chrono::milliseconds(100));
    }

    std::printf("\n테스트 종료 — 연결 해제 중...\n");
    client.Disconnect();
    return 0;
}
