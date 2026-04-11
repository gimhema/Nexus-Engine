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
