# Nexus Engine 개발 가이드라인

---

## 1. 클라이언트 ↔ 서버 멀티플레이 기능 구현 가이드라인

### 1.1 패킷 포맷

```
TCP : [ uint16 size ][ uint16 opcode ][ payload... ]
UDP : [ uint16 size ][ uint16 opcode ][ uint64 sessionToken ][ payload... ]
```

- `size` : 헤더 포함 전체 바이트 수
- `opcode` : 패킷 종류 (`CMSG_` = 클→서, `SMSG_` = 서→클)
- TCP 최대 32 KB / UDP 최대 1400 바이트

### 1.2 Opcode 추가 방법

`Game/Data/GamePackets/Packet-Example.hpp`의 `Opcode` enum에 추가한다.

```cpp
// 새 그룹: _BASE만 직접 지정, 이후 auto-increment
_COMBAT_BASE  = 0x0400,
CMSG_ATTACK,       // [uint64 targetPawnId]
SMSG_ATTACK_RESULT,// [uint64 attackerPawnId][uint64 targetPawnId][int32 damage]
_COMBAT_END,
```

> **주의**: DummyClient의 `NexusClient/DummyClient/Packets/GamePackets.hpp`에도 동일한 값을 반드시 추가해야 한다.

### 1.3 수신 패킷 처리 흐름

```
NetworkWorker (I/O 스레드)
  └─ NetworkManager::onPacket 콜백
       └─ SessionActor::Post(MsgNet_PacketReceived)
            └─ SessionActor::Handle(MsgNet_PacketReceived)
                 └─ switch(opcode)
                      ├─ 인증 관련  → WorldActor::Post(...)
                      └─ 존 내 로직 → ZoneActor::Post(...)
```

`SessionActor.cpp`의 `Handle(MsgNet_PacketReceived)`에 `switch` 케이스를 추가한다.

```cpp
case CMSG_ATTACK:
{
    auto* zone = m_zone.load(std::memory_order_acquire);
    if (!zone) break;
    MsgSession_Attack msg;
    msg.sessionId    = m_sessionId;
    msg.targetPawnId = reader.ReadUInt64();
    zone->Post(std::move(msg));
    break;
}
```

### 1.4 송신 패킷 조립 및 전송

`PacketWriter`로 조립 후 `SessionActor`를 통해 전송한다.

```cpp
// ZoneActor 내부에서 특정 클라이언트에게 전송
PacketWriter w(SMSG_ATTACK_RESULT);
w.WriteUInt64(attackerPawnId);
w.WriteUInt64(targetPawnId);
w.WriteUInt32(static_cast<uint32_t>(damage));
if (auto sa = pawn->GetSession())
    sa->Post(MsgZone_SendTcp{ w.Finalize() });

// 존 전체 브로드캐스트 (발신자 제외)
BroadcastTcp(attackerSessionId, w.Finalize());

// 존 전체 브로드캐스트 (자신 포함)
BroadcastTcp(0, w.Finalize());
```

### 1.5 TCP vs UDP 선택 기준

| 전송 방식 | 사용 상황 |
|---|---|
| TCP (`MsgZone_SendTcp`) | 중요 상태 변경 — 로그인, 채팅, 피해/사망, 스킬 발동 |
| UDP (`MsgZone_SendUdp`) | 고빈도 위치 동기화 — 이동 브로드캐스트 |

> **현재 상태**: UDP 수신은 구현됨. UDP 송신은 `MsgZone_SendUdp` 처리 시 TCP로 fallback 중이며, `NetworkManager::SendUDP` 경로는 미구현.

---

## 2. 서버 로직 구현 가이드라인

### 2.1 서버 루프 로직 구현

서버에는 두 개의 독립적인 틱 루프가 존재한다.

#### ZoneActor 틱 (50ms, 20Hz)

`ZoneActor::OnTick()`이 50ms마다 호출된다. 존 내 모든 Pawn의 `OnTick(tickCount)`을 순회 호출하는 것이 주 역할이다.

```
ZoneActor 전용 스레드
  └─ OnTick() — 50ms마다
       ├─ PlayerPawn::OnTick(tickCount) × N
       └─ Pawn(NPC/Monster)::OnTick(tickCount) × M
```

존 로직(이동 검증, AI, AOI 등)을 추가할 때는 `ZoneActor::OnTick()`에 작성한다.

```cpp
void ZoneActor::OnTick()
{
    ++m_tickCount;

    for (auto& [sid, pawn] : m_playerPawns)
        pawn->OnTick(m_tickCount);

    for (auto& [pid, pawn] : m_npcPawns)
        pawn->OnTick(m_tickCount);

    // Phase 2 예정: AI FSM, GridCell AOI, Aura 틱
}
```

`tickCount`를 활용해 주기 제어가 가능하다.

```cpp
// 예: 5틱(250ms)마다 실행
if (tickCount % 5 == 0) { /* ... */ }
```

#### GameLogic 틱 (1000ms, 1Hz)

`GameLogic::OnTick()`이 1초마다 호출된다. 세계 시간 진행 및 낮/밤·이벤트 발동을 담당한다. 게임플레이 로직은 이 루프에 작성하지 않는다.

```
GameLogic 전용 스레드
  └─ OnTick() — 1초마다
       ├─ 게임 시간 누적
       ├─ 낮/밤 전환 판정
       └─ 등록된 WorldEvent 발동 → 모든 ZoneActor에 MsgGameLogic_WorldEvent
```

#### 월드 이벤트 수신 (ZoneActor)

`ZoneActor::Handle(MsgGameLogic_WorldEvent&)`에서 GameLogic이 발동한 이벤트를 받는다.

```cpp
void ZoneActor::Handle(MsgGameLogic_WorldEvent& msg)
{
    if (msg.eventId == WorldEventId::NightBegin)
    {
        // 야간 몬스터 추가 스폰 등
    }
}
```

---

### 2.2 Pawn(플레이어블 / 논플레이어블) 로직 구현

#### 클래스 구조

```
Pawn                          — 런타임 상태 (hp, pos, orientation, session)
│  unique_ptr<GameDataEntityBase> m_data   — 스탯 데이터 (캐릭터 시트)
│
├─ PlayerPawn                 — 플레이어블. m_session 보유. CharacterEntityData 사용.
└─ (확장 가능) NpcPawn 등     — 논플레이어블. m_session 없음. NpcEntityData 사용.
```

현재 NPC/몬스터는 `Pawn` 베이스 클래스를 직접 사용한다.

#### Pawn 생성

```cpp
// 논플레이어블 (NPC/몬스터) — ZoneActor::OnStart() 내부
auto data = std::make_unique<NpcEntityData>(200, 15, 10, 2.f,
                                            EFactionId::STORMWIND,
                                            EAIType::DEFENSIVE, 0.f);
auto pawn = std::make_unique<Pawn>("마을 경비병", std::move(data));
pawn->SetPos({ 10.f, 0.f, 10.f });
SpawnNpc(std::move(pawn));

// 플레이어블 — ZoneActor::Handle(MsgWorld_AddPlayer) 내부
auto pawn = std::make_unique<PlayerPawn>(characterName, characterId, sessionActor);
pawn->SetPos(spawnPos);
pawn->OnSpawn();
m_playerPawns.emplace(sessionId, std::move(pawn));
```

#### Pawn 서브클래스 작성 방법

`OnTick`, `OnSpawn`, `OnDespawn`, `ApplyDamage`를 override해 로직을 추가한다.

```cpp
class MonsterPawn : public Pawn
{
public:
    using Pawn::Pawn;

    void OnSpawn() override
    {
        // 어그로 초기화 등
    }

    void OnTick(uint64_t tickCount) override
    {
        if (!IsAlive()) return;
        // AI 행동 — EAIType을 읽어 분기
        auto* data = GetEntityData();
        if (data->GetAIType() == EAIType::AGGRESSIVE)
        {
            // aggroRange 내 적대 진영 탐색 → 이동/공격
        }
    }

    void ApplyDamage(int32_t amount) override
    {
        Pawn::ApplyDamage(amount);
        if (!IsAlive())
        {
            // 드롭, 경험치 보상 처리 예정
        }
    }
};
```

> **현재 상태**: `EAIType` 정의 및 `m_aggroRange` 필드는 존재하나, AI 행동 로직(`AGGRESSIVE` 자동 어그로, `DEFENSIVE` 반격)은 미구현. `OnTick` 안에서 직접 구현해야 한다.

#### PlayerPawn 전용 데이터 접근

```cpp
PlayerPawn* pawn = FindPlayerPawn(sessionId);
CharacterEntityData& charData = pawn->GetCharacterData();

// 경험치 추가 및 레벨업 여부 확인
if (charData.AddExperience(100))
{
    // 레벨업 패킷 전송
}
```

#### 피해 / 회복 처리

```cpp
// 공격자가 대상 Pawn에 데미지 적용
int32_t damage = attacker->GetAttack() - target->GetDefense();
damage = std::max(damage, 1);
target->ApplyDamage(damage);

if (!target->IsAlive())
{
    // 사망 처리 — 디스폰, 경험치 지급 등
    DespawnNpc(target->GetPawnId());
}
```

#### 진영 관계 판정

```cpp
auto& reg = FactionRegistry::Instance();
EFactionId attackerFaction = attacker->GetEntityData()->GetFactionId();
EFactionId targetFaction   = target->GetEntityData()->GetFactionId();

if (reg.IsHostile(attackerFaction, targetFaction))
{
    // 공격 허용
}
```

---

---

## 3. 플레이어 스폰 / 디스폰 동기화 구현 가이드라인

플레이어가 존에 진입할 때 **본인과 기존 플레이어 양쪽 모두에게** 폰 정보를 전달해야 한다.
퇴장 시에는 기존 플레이어들에게 디스폰을 알린다.

### 3.1 패킷 정의

```
SMSG_ENTER_WORLD (0x0104) — 진입한 본인에게만 전송
  [uint8  success]
  [uint64 pawnId]        서버가 발급한 고유 폰 ID
  [uint32 characterId]
  [string name]
  [uint32 hp]
  [uint32 maxHp]
  [float  x][float y][float z][float orientation]

SMSG_SPAWN_PLAYER (0x0401) — 다른 플레이어에게 전송 (상호 스폰)
  [uint64 pawnId]
  [uint64 sessionId]
  [string name]
  [uint32 hp]
  [uint32 maxHp]
  [float  x][float y][float z][float orientation]

SMSG_DESPAWN_PLAYER (0x0402) — 퇴장 시 잔류 플레이어에게 전송
  [uint64 pawnId]
```

### 3.2 서버 처리 흐름

```
CMSG_ENTER_WORLD [characterId]
  → SessionActor → WorldActor (MsgSession_EnterWorld)
  → WorldActor → ZoneActor (MsgWorld_AddPlayer)
  → ZoneActor::Handle(MsgWorld_AddPlayer):
      1. PlayerPawn 생성 + spawnPos 결정
      2. 신규 플레이어 → SMSG_ENTER_WORLD (본인 폰 정보)
      3. 신규 플레이어 → SMSG_SPAWN_PLAYER × 기존 플레이어 수 (기존 폰 목록)
      4. 기존 플레이어들 → SMSG_SPAWN_PLAYER (신규 폰 정보) BroadcastTcp
      5. m_playerPawns에 추가
```

### 3.3 구현 위치

모든 로직은 `ZoneActor.cpp`의 두 핸들러에 위치한다.

```cpp
// 진입 — ZoneActor.cpp: Handle(MsgWorld_AddPlayer)
void ZoneActor::Handle(MsgWorld_AddPlayer& msg)
{
    auto pawn = std::make_unique<PlayerPawn>(msg.characterName, msg.characterId, msg.sessionActor);
    const Vec3 spawnPos = /* Zone 스폰 포인트 순환 또는 msg.spawnPos */;
    pawn->SetPos(spawnPos);
    pawn->OnSpawn();

    const uint64_t pawnId = pawn->GetPawnId();
    const uint32_t hp     = static_cast<uint32_t>(pawn->GetHp());
    const uint32_t maxHp  = static_cast<uint32_t>(pawn->GetMaxHp());

    if (auto sa = msg.sessionActor.lock())
    {
        // 1) 본인에게 SMSG_ENTER_WORLD
        PacketWriter w(SMSG_ENTER_WORLD);
        w.WriteUInt8(1).WriteUInt64(pawnId).WriteUInt32(msg.characterId)
         .WriteString(msg.characterName).WriteUInt32(hp).WriteUInt32(maxHp)
         .WriteFloat(spawnPos.x).WriteFloat(spawnPos.y).WriteFloat(spawnPos.z).WriteFloat(0.f);
        sa->Post(MsgZone_SendTcp{ w.Finalize() });

        // 2) 기존 플레이어 목록 → 신규 플레이어에게
        for (auto& [sid, existing] : m_playerPawns)
        {
            PacketWriter ws(SMSG_SPAWN_PLAYER);
            ws.WriteUInt64(existing->GetPawnId()).WriteUInt64(sid)
              .WriteString(existing->GetName())
              .WriteUInt32(static_cast<uint32_t>(existing->GetHp()))
              .WriteUInt32(static_cast<uint32_t>(existing->GetMaxHp()))
              .WriteFloat(existing->GetPos().x).WriteFloat(existing->GetPos().y)
              .WriteFloat(existing->GetPos().z).WriteFloat(existing->GetOrientation());
            sa->Post(MsgZone_SendTcp{ ws.Finalize() });
        }
    }

    // 3) 신규 플레이어 → 기존 플레이어들에게 브로드캐스트
    PacketWriter wb(SMSG_SPAWN_PLAYER);
    wb.WriteUInt64(pawnId).WriteUInt64(msg.sessionId)
      .WriteString(msg.characterName).WriteUInt32(hp).WriteUInt32(maxHp)
      .WriteFloat(spawnPos.x).WriteFloat(spawnPos.y).WriteFloat(spawnPos.z).WriteFloat(0.f);
    BroadcastTcp(msg.sessionId, wb.Finalize());

    m_playerPawns.emplace(msg.sessionId, std::move(pawn));
}
```

```cpp
// 퇴장 — ZoneActor.cpp: Handle(MsgWorld_RemovePlayer) / Handle(MsgSession_LeaveZone)
// 두 핸들러 모두 동일한 패턴
void ZoneActor::Handle(MsgWorld_RemovePlayer& msg)
{
    auto it = m_playerPawns.find(msg.sessionId);
    if (it == m_playerPawns.end()) return;

    const uint64_t pawnId = it->second->GetPawnId();
    it->second->OnDespawn();
    m_playerPawns.erase(it);

    PacketWriter w(SMSG_DESPAWN_PLAYER);
    w.WriteUInt64(pawnId);
    BroadcastTcp(msg.sessionId, w.Finalize());
}
```

### 3.4 클라이언트 수신 처리 (UE5 참고)

```
SMSG_ENTER_WORLD 수신
  → pawnId, characterId, name, hp/maxHp, 위치로 로컬 플레이어 폰 스폰

SMSG_SPAWN_PLAYER 수신
  → pawnId, sessionId, name, hp/maxHp, 위치로 원격 플레이어 폰 스폰
  → pawnId를 키로 맵에 보관 (이후 이동/디스폰 메시지에서 사용)

SMSG_DESPAWN_PLAYER 수신
  → pawnId로 원격 플레이어 폰 제거
```

### 3.5 주의 사항

- `SMSG_ENTER_WORLD` 전송 후 `m_playerPawns.emplace` 전에 기존 플레이어 목록을 순회해야  
  신규 플레이어 자신이 목록에 포함되지 않는다.
- `BroadcastTcp(msg.sessionId, ...)` 호출 시점은 `emplace` 전후 모두 안전하다.  
  `BroadcastTcp`는 `excludeSessionId`로 신규 플레이어를 자동 제외한다.
- `hp`/`maxHp`는 `int32_t`이지만 패킷 전송 시 `uint32_t`로 캐스트한다.  
  클라이언트도 `uint32_t`로 읽은 후 `int32_t`로 해석하면 된다.

---

## 미구현 항목 (향후 작업)

| 항목 | 상태 |
|---|---|
| UDP 송신 (`NetworkManager::SendUDP`) | 미구현 — TCP fallback 중 |
| AI 행동 로직 (`AGGRESSIVE` 어그로, `DEFENSIVE` 반격) | 미구현 — `EAIType` 필드만 존재 |
| AOI (Area of Interest) 필터링 | 미구현 — 현재 존 전체 브로드캐스트 |
| 전투 시스템 (공격 요청 처리, 사망 이벤트) | 미구현 |
| 몬스터 경험치 보상 / 드롭테이블 | 미구현 |
| 인벤토리 / 장비 슬롯 | 미구현 |
| 스킬 시스템 | 미구현 |
