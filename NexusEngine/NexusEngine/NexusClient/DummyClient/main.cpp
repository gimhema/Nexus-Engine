#include <NetClient.h>
#include "../../protocol_shared/Packets/Packet-Auth.h"
#include "../../protocol_shared/Packets/Packet-Movement.h"
#include "../../protocol_shared/Packets/Packet-Chat.h"
#include "../../protocol_shared/Packets/Packet-Spawn.h"

#include <atomic>
#include <chrono>
#include <cstdio>
#include <csignal>
#include <mutex>
#include <random>
#include <string>
#include <thread>
#include <unordered_map>

// ─────────────────────────────────────────────────────────────────────────────
// 접속 테스트 시나리오
//
//   1. 서버 접속
//   2. CMSG_LOGIN              → SMSG_LOGIN_RESULT 대기
//   3. CMSG_CHAR_SETUP         → SMSG_CHAR_SETUP_RESULT 대기 (서버 발급 characterId 수신)
//   4. CMSG_ENTER_WORLD        → SMSG_ENTER_WORLD 대기
//   5. SMSG_SPAWN_PLAYER       → 타 플레이어 스폰 (g_remotePlayers 등록)
//                                [UE] GetWorld()->SpawnActor<ANexusOtherPlayer>() 호출 지점
//   6. CMSG_MOVE (500ms 주기)  → 내 위치를 서버로 전송 → 타 클라이언트에 SMSG_MOVE_BROADCAST
//   7. SMSG_MOVE_BROADCAST     → 타 플레이어 위치 갱신 (g_remotePlayers 업데이트)
//                                [UE] SetActorLocation / 보간 처리 지점
//   8. SMSG_DESPAWN_PLAYER     → 타 플레이어 제거 (g_remotePlayers 삭제)
//                                [UE] Actor->Destroy() 호출 지점
//   9. Ctrl+C 로 종료
// ─────────────────────────────────────────────────────────────────────────────

static constexpr const char* SERVER_HOST = "127.0.0.1";
static constexpr uint16_t    SERVER_PORT = 7070;
static constexpr const char* LOGIN_TOKEN = "dummy_token";

// ─────────────────────────────────────────────────────────────────────────────
// 원격 플레이어 상태 레코드
//
// UE 가이드:
//   이 구조체 대신 ANexusOtherPlayer* (APawn 서브클래스)를 직접 사용.
//   위치·회전은 Actor 컴포넌트가 보유하므로 별도 저장 불필요.
//   TMap<uint64, ANexusOtherPlayer*> OtherPlayers 로 sessionId 매핑.
// ─────────────────────────────────────────────────────────────────────────────
struct RemotePlayerInfo
{
    uint64_t    pawnId;
    uint64_t    sessionId;
    std::string name;
    uint32_t    hp;
    uint32_t    maxHp;
    float       x, y, z;
    float       orientation;
};

std::unordered_map<uint64_t, RemotePlayerInfo> g_remotePlayers;  // key: sessionId
std::unordered_map<uint64_t, uint64_t>         g_pawnToSession;  // pawnId → sessionId (디스폰 역조회용)
std::mutex                                      g_playersMtx;

// ─────────────────────────────────────────────────────────────────────────────
// 내 캐릭터 상태
// ─────────────────────────────────────────────────────────────────────────────
std::atomic<bool>     g_running{ true };
std::atomic<bool>     g_inWorld{ false };   // SMSG_ENTER_WORLD 성공 후 true
std::atomic<uint32_t> g_characterId{ 0 };

// SMSG_ENTER_WORLD 수신 스레드에서 설정 후 g_inWorld(release) → 메인 스레드(acquire)로 가시성 보장
uint64_t g_myPawnId   = 0;
float    g_myX        = 0.f;
float    g_myY        = 0.f;
float    g_myZ        = 0.f;
float    g_myOrient   = 0.f;

std::string g_charName;

// ─────────────────────────────────────────────────────────────────────────────
// 무작위 캐릭터 이름 생성
// 형식: <형용사><명사><숫자 2자리>  예) "BraveSword42"
// ─────────────────────────────────────────────────────────────────────────────
static std::string GenerateCharName()
{
    static const char* adjectives[] = {
        "Brave", "Swift", "Dark", "Iron", "Storm",
        "Frost", "Blaze", "Silent", "Wild", "Shadow"
    };
    static const char* nouns[] = {
        "Sword", "Arrow", "Shield", "Wolf", "Eagle",
        "Knight", "Rogue", "Mage", "Hunter", "Titan"
    };

    static std::mt19937 rng{ std::random_device{}() };
    std::uniform_int_distribution<std::size_t> adjDist(0, std::size(adjectives) - 1);
    std::uniform_int_distribution<std::size_t> nounDist(0, std::size(nouns)      - 1);
    std::uniform_int_distribution<int>         numDist(10, 99);

    return std::string(adjectives[adjDist(rng)])
         + nouns[nounDist(rng)]
         + std::to_string(numDist(rng));
}

// ─────────────────────────────────────────────────────────────────────────────
// 송신 헬퍼 — 공유 패킷 클래스의 Encode() 사용
// ─────────────────────────────────────────────────────────────────────────────

static void SendLogin(NetClient& client)
{
    client.Send(CMsg_Login{ .accountName = g_charName, .token = LOGIN_TOKEN }.Encode());
    std::printf("[→] CMSG_LOGIN  account=%s\n", g_charName.c_str());
}

static void SendCharSetup(NetClient& client, const std::string& name)
{
    client.Send(CMsg_CharSetup{ .characterName = name }.Encode());
    std::printf("[→] CMSG_CHAR_SETUP  name=%s\n", name.c_str());
}

static void SendEnterWorld(NetClient& client, uint32_t characterId)
{
    client.Send(CMsg_EnterWorld{ .characterId = characterId }.Encode());
    std::printf("[→] CMSG_ENTER_WORLD  characterId=%u\n", characterId);
}

static void SendZoneChat(NetClient& client, const std::string& text)
{
    client.Send(CMsg_Chat{ .text = text }.Encode());
    std::printf("[→] CMSG_CHAT (존)  text=\"%s\"\n", text.c_str());
}

static void SendWorldChat(NetClient& client, const std::string& text)
{
    client.Send(CMsg_WorldChat{ .text = text }.Encode());
    std::printf("[→] CMSG_WORLD_CHAT  text=\"%s\"\n", text.c_str());
}

static void SendMove(NetClient& client, float x, float y, float z, float orientation = 0.f)
{
    client.Send(CMsg_Move{ .x = x, .y = y, .z = z, .orientation = orientation }.Encode());
    std::printf("[→] CMSG_MOVE  pos=(%.1f, %.1f, %.1f)  o=%.2f\n", x, y, z, orientation);
}

// ─────────────────────────────────────────────────────────────────────────────
// 원격 플레이어 스폰 처리
//
// SMSG_SPAWN_PLAYER 수신 시 호출.
// 두 가지 상황 모두 이 함수로 처리됨:
//   A) 내가 존 입장 → 기존 플레이어 목록을 반복 수신 (존재하는 N명만큼 호출)
//   B) 다른 플레이어가 존 입장 → 단발 수신
//
// UE 가이드:
//   FVector SpawnLoc(x, y, z);
//   FRotator SpawnRot(0.f, FMath::RadiansToDegrees(orientation), 0.f);
//   ANexusOtherPlayer* Actor = GetWorld()->SpawnActor<ANexusOtherPlayer>(
//       OtherPlayerClass, SpawnLoc, SpawnRot);
//   if (Actor)
//   {
//       Actor->InitFromServer(pawnId, sessionId, FString(name), hp, maxHp);
//       OtherPlayers.Add(sessionId, Actor);   // TMap<uint64, ANexusOtherPlayer*>
//   }
// ─────────────────────────────────────────────────────────────────────────────
static void OnRemotePlayerSpawn(const SMsg_SpawnPlayer& pkt)
{
    {
        std::lock_guard lock(g_playersMtx);
        g_remotePlayers[pkt.sessionId] = {
            pkt.pawnId, pkt.sessionId, pkt.name,
            pkt.hp, pkt.maxHp, pkt.x, pkt.y, pkt.z, pkt.orientation
        };
        g_pawnToSession[pkt.pawnId] = pkt.sessionId;
    }
    std::printf("[←] SMSG_SPAWN_PLAYER  pawnId=%-6llu  name=%-20s  hp=%u/%u  pos=(%.1f, %.1f, %.1f)\n",
                static_cast<unsigned long long>(pkt.pawnId),
                pkt.name.c_str(), pkt.hp, pkt.maxHp, pkt.x, pkt.y, pkt.z);
}

// ─────────────────────────────────────────────────────────────────────────────
// 원격 플레이어 디스폰 처리
//
// SMSG_DESPAWN_PLAYER 수신 시 호출.
// 서버는 pawnId만 전송하므로 g_pawnToSession 역조회로 sessionId를 획득.
//
// UE 가이드:
//   if (ANexusOtherPlayer** Actor = OtherPlayers.Find(sessionId))
//   {
//       (*Actor)->Destroy();
//       OtherPlayers.Remove(sessionId);
//   }
// ─────────────────────────────────────────────────────────────────────────────
static void OnRemotePlayerDespawn(const SMsg_DespawnPlayer& pkt)
{
    uint64_t    sessionId = 0;
    std::string name;
    {
        std::lock_guard lock(g_playersMtx);
        auto it = g_pawnToSession.find(pkt.pawnId);
        if (it == g_pawnToSession.end()) return;

        sessionId = it->second;
        auto pit  = g_remotePlayers.find(sessionId);
        if (pit != g_remotePlayers.end())
        {
            name = pit->second.name;
            g_remotePlayers.erase(pit);
        }
        g_pawnToSession.erase(it);
    }
    std::printf("[←] SMSG_DESPAWN_PLAYER  pawnId=%-6llu  name=%s  sessionId=%llu\n",
                static_cast<unsigned long long>(pkt.pawnId),
                name.c_str(),
                static_cast<unsigned long long>(sessionId));
}

// ─────────────────────────────────────────────────────────────────────────────
// 원격 플레이어 TCP 이동 처리
//
// SMSG_MOVE_BROADCAST 수신 시 호출. 빈도가 낮으므로 즉시 위치 적용.
//
// UE 가이드:
//   if (ANexusOtherPlayer** Actor = OtherPlayers.Find(sessionId))
//   {
//       FVector  TargetLoc(x, y, z);
//       FRotator TargetRot(0.f, FMath::RadiansToDegrees(orientation), 0.f);
//       (*Actor)->SetActorLocationAndRotation(TargetLoc, TargetRot);
//   }
// ─────────────────────────────────────────────────────────────────────────────
static void OnRemotePlayerMoveTcp(const SMsg_MoveBroadcast& pkt)
{
    {
        std::lock_guard lock(g_playersMtx);
        auto it = g_remotePlayers.find(pkt.sessionId);
        if (it != g_remotePlayers.end())
        {
            it->second.x           = pkt.x;
            it->second.y           = pkt.y;
            it->second.z           = pkt.z;
            it->second.orientation = pkt.orientation;
        }
    }
    std::printf("[←] SMSG_MOVE_BROADCAST  sessionId=%-6llu  pos=(%.1f, %.1f, %.1f)  o=%.2f\n",
                static_cast<unsigned long long>(pkt.sessionId), pkt.x, pkt.y, pkt.z, pkt.orientation);
}

// ─────────────────────────────────────────────────────────────────────────────
// 원격 플레이어 UDP 이동 처리
//
// SMSG_MOVE_UDP 수신 시 호출. 고빈도이므로 목표 위치를 저장 후 Tick에서 보간 처리 권장.
//
// UE 가이드:
//   if (ANexusOtherPlayer** Actor = OtherPlayers.Find(sessionId))
//   {
//       (*Actor)->SetNetworkTargetLocation(FVector(x, y, z));
//       // Tick():
//       //   FVector Current = GetActorLocation();
//       //   FVector Interped = FMath::VInterpTo(Current, NetworkTargetLocation,
//       //                                       DeltaTime, InterpSpeed);
//       //   SetActorLocation(Interped);
//   }
// ─────────────────────────────────────────────────────────────────────────────
static void OnRemotePlayerMoveUdp(const SMsg_MoveUdp& pkt)
{
    {
        std::lock_guard lock(g_playersMtx);
        auto it = g_remotePlayers.find(pkt.sessionId);
        if (it != g_remotePlayers.end())
        {
            it->second.x           = pkt.x;
            it->second.y           = pkt.y;
            it->second.z           = pkt.z;
            it->second.orientation = pkt.orientation;
        }
    }
    std::printf("[←] SMSG_MOVE_UDP        sessionId=%-6llu  pos=(%.1f, %.1f, %.1f)  o=%.2f\n",
                static_cast<unsigned long long>(pkt.sessionId), pkt.x, pkt.y, pkt.z, pkt.orientation);
}

// ─────────────────────────────────────────────────────────────────────────────
int main()
{
    g_charName = GenerateCharName();

    std::printf("=== NexusClient DummyClient ===\n");
    std::printf("캐릭터 이름: %s\n", g_charName.c_str());
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

    // ── 수신 패킷 핸들러 — 공유 패킷 클래스의 Decode() 사용 ───────────────────
    client.SetOnPacket([&client](uint16_t opcode, std::vector<uint8_t> payload)
    {
        NexusPacketParser r{ payload.data(), static_cast<uint32_t>(payload.size()) };

        switch (static_cast<Opcode>(opcode))
        {
        // ── 인증 / 접속 ───────────────────────────────────────────────────────
        case SMSG_LOGIN_RESULT:
        {
            auto pkt = SMsg_LoginResult::Decode(r);
            std::printf("[←] SMSG_LOGIN_RESULT  success=%u  message=\"%s\"\n",
                        pkt.success, pkt.message.c_str());
            if (pkt.success)
                SendCharSetup(client, g_charName);
            else
                g_running.store(false);
            break;
        }
        case SMSG_CHAR_SETUP_RESULT:
        {
            auto pkt = SMsg_CharSetupResult::Decode(r);
            std::printf("[←] SMSG_CHAR_SETUP_RESULT  success=%u  characterId=%u  message=\"%s\"\n",
                        pkt.success, pkt.characterId, pkt.message.c_str());
            if (pkt.success)
            {
                g_characterId.store(pkt.characterId);
                SendEnterWorld(client, pkt.characterId);
            }
            else
                g_running.store(false);
            break;
        }
        case SMSG_ENTER_WORLD:
        {
            // ── UE 가이드 ────────────────────────────────────────────────────
            // success=true 시 내 캐릭터(APlayerController가 빙의한 APawn)를
            // SpawnPos 위치에 배치하고 HUD를 초기화한다.
            // pawnId 는 이후 다른 패킷에서 자신을 식별하는 키이므로
            // GameInstance 또는 PlayerState에 저장해 둔다.
            auto pkt = SMsg_EnterWorld::Decode(r);
            std::printf("[←] SMSG_ENTER_WORLD  success=%u  pawnId=%llu  charId=%u  name=%s\n"
                        "                      hp=%u/%u  spawnPos=(%.1f, %.1f, %.1f)  o=%.2f\n",
                        pkt.success,
                        static_cast<unsigned long long>(pkt.pawnId),
                        pkt.characterId, pkt.name.c_str(),
                        pkt.hp, pkt.maxHp, pkt.x, pkt.y, pkt.z, pkt.orientation);

            if (pkt.success)
            {
                g_myPawnId = pkt.pawnId;
                g_myX      = pkt.x;
                g_myY      = pkt.y;
                g_myZ      = pkt.z;
                g_myOrient = pkt.orientation;
                g_inWorld.store(true, std::memory_order_release);

                SendZoneChat(client, "안녕하세요! (존 채팅)");
                SendWorldChat(client, "안녕하세요! (월드 채팅)");
            }
            else
            {
                g_running.store(false);
            }
            break;
        }

        // ── 스폰 / 디스폰 ─────────────────────────────────────────────────────
        case SMSG_SPAWN_PLAYER:
            OnRemotePlayerSpawn(SMsg_SpawnPlayer::Decode(r));
            break;

        case SMSG_DESPAWN_PLAYER:
            OnRemotePlayerDespawn(SMsg_DespawnPlayer::Decode(r));
            break;

        // ── 이동 ──────────────────────────────────────────────────────────────
        case SMSG_MOVE_BROADCAST:
            OnRemotePlayerMoveTcp(SMsg_MoveBroadcast::Decode(r));
            break;

        case SMSG_MOVE_UDP:
            OnRemotePlayerMoveUdp(SMsg_MoveUdp::Decode(r));
            break;

        // ── 채팅 ──────────────────────────────────────────────────────────────
        case SMSG_CHAT:
        {
            auto pkt = SMsg_Chat::Decode(r);
            std::printf("[←] SMSG_CHAT (존)  [%s] %s  (sessionId=%llu)\n",
                        pkt.name.c_str(), pkt.text.c_str(),
                        static_cast<unsigned long long>(pkt.sessionId));
            break;
        }
        case SMSG_WORLD_CHAT:
        {
            auto pkt = SMsg_WorldChat::Decode(r);
            std::printf("[←] SMSG_WORLD_CHAT  [%s] %s  (sessionId=%llu)\n",
                        pkt.name.c_str(), pkt.text.c_str(),
                        static_cast<unsigned long long>(pkt.sessionId));
            break;
        }

        default:
            std::printf("[←] 알 수 없는 opcode=0x%04x  payloadSize=%u\n",
                        opcode, static_cast<uint32_t>(payload.size()));
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

    // ── 메인 루프: 이동 시뮬레이션 + 존 현황 출력 ────────────────────────────
    //
    // UE 가이드:
    //   이 루프에 해당하는 로직은 PlayerController::Tick() 또는
    //   UNexusNetworkComponent::TickComponent() 에 구현한다.
    //   - 이동 입력: EnhancedInput 의 IA_Move 값을 읽어 CMSG_MOVE 전송
    //   - 빠른 위치 동기화가 필요하다면 CMSG_MOVE_UDP 로 전환
    auto lastMoveSend    = std::chrono::steady_clock::now();
    auto lastStatusPrint = std::chrono::steady_clock::now();
    float moveDirX = 1.f;

    while (g_running.load())
    {
        auto now = std::chrono::steady_clock::now();

        if (g_inWorld.load(std::memory_order_acquire))
        {
            // 500ms마다 이동 패킷 전송 (왕복 직선 이동 시뮬레이션)
            if (now - lastMoveSend >= std::chrono::milliseconds(500))
            {
                g_myX += moveDirX * 2.f;
                if (g_myX > 30.f || g_myX < -30.f)
                    moveDirX = -moveDirX;
                g_myOrient = (moveDirX > 0.f) ? 0.f : 3.14159f;
                SendMove(client, g_myX, g_myY, g_myZ, g_myOrient);
                lastMoveSend = now;
            }

            // 5초마다 존 플레이어 현황 출력
            if (now - lastStatusPrint >= std::chrono::seconds(5))
            {
                std::lock_guard lock(g_playersMtx);
                std::printf("\n[현황] 내 pawnId=%llu  존 내 다른 플레이어 수: %zu\n",
                            static_cast<unsigned long long>(g_myPawnId),
                            g_remotePlayers.size());
                for (auto& [sid, p] : g_remotePlayers)
                    std::printf("  %-20s  pawnId=%-6llu  pos=(%.1f, %.1f, %.1f)  hp=%u/%u\n",
                                p.name.c_str(),
                                static_cast<unsigned long long>(p.pawnId),
                                p.x, p.y, p.z, p.hp, p.maxHp);
                std::printf("\n");
                lastStatusPrint = now;
            }
        }

        std::this_thread::sleep_for(std::chrono::milliseconds(50));
    }

    std::printf("\n종료 — 연결 해제 중...\n");
    client.Disconnect();
    return 0;
}
