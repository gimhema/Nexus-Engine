# 패킷 추가 가이드

새 게임 메시지를 추가할 때 반드시 이 순서를 따른다.  
각 단계는 독립적이지 않으며, 앞 단계가 완료되어야 다음 단계가 의미를 갖는다.

---

## 개요: 파일 역할 분담

| 파일 | 역할 | 수정 주체 |
|------|------|-----------|
| `protocol_shared/Opcodes.h` | Opcode enum — 서버·클라이언트 단일 소스 | 새 메시지마다 추가 |
| `Game/Data/GamePackets/Packet-{Group}.h/cpp` | 패킷 클래스 — 직렬화 계약 | 새 메시지마다 추가 |
| `Game/Actors/SessionActor.cpp` or `ZoneActor.cpp` | 서버 수신 핸들러 / 송신 호출부 | 메시지 처리 로직 |
| `NexusClient/DummyClient/main.cpp` | 클라이언트 수신·송신 예시 | UE 가이드라인용 |

---

## 클래스 패턴

### CMsg (Client → Server 수신)

```cpp
// 평범한 구조체. 서버가 PacketReader 로 역직렬화.
struct CMsg_Foo
{
    /* 필드 */

    static CMsg_Foo Decode(PacketReader& r);
};
```

### SMsg (Server → Client 송신)

```cpp
// PacketWriter 상속. 생성자가 필드를 기록. Finalize() 로 바이트 버퍼 획득.
class SMsg_Bar : public PacketWriter
{
public:
    SMsg_Bar(/* 필드 파라미터 */);
};

// 사용
session->Send(SMsg_Bar(/* ... */).Finalize());
zone.BroadcastTcp(excludeId, SMsg_Bar(/* ... */).Finalize());
```

---

## 그룹 파일 배정 기준

| 그룹 | 파일 | Opcode 범위 | 포함 메시지 |
|------|------|-------------|------------|
| 인증 / 입장 | `Packet-Auth.h/cpp` | 0x0100 | LOGIN, CHAR_SETUP, ENTER_WORLD |
| 이동 | `Packet-Movement.h/cpp` | 0x0200 | MOVE, MOVE_UDP |
| 채팅 | `Packet-Chat.h/cpp` | 0x0300 | CHAT, WORLD_CHAT |
| 스폰 / 디스폰 | `Packet-Spawn.h/cpp` | 0x0400 | SPAWN_PLAYER, DESPAWN_PLAYER |

새 그룹이 필요하면 `Opcodes.h` 에 `_XXX_BASE = 0x0N00` 을 추가하고  
`Packet-{Group}.h/cpp` 를 새로 만든 뒤 `CMakeLists.txt` 의 SOURCES 에 등록한다.

---

## 단계별 절차

### 1단계 — Opcode 선언 (`protocol_shared/Opcodes.h`)

```cpp
// ── 전투 (0x0500번대) ─────────────────────────────────────────────────────────
_COMBAT_BASE        = 0x0500,
CMSG_ATTACK,        // C→S (TCP) [uint64 targetPawnId]
SMSG_ATTACK_RESULT, // S→C (TCP) [uint64 attackerPawnId][uint64 targetPawnId][int32 damage][uint32 remainHp]
_COMBAT_END,
```

규칙:
- `CMSG_` = Client → Server
- `SMSG_` = Server → Client
- 인라인 주석에 전송 방향·프로토콜·필드를 명시

---

### 2단계 — 패킷 클래스 구현 (`Game/Data/GamePackets/Packet-{Group}.h/cpp`)

**헤더 (`Packet-Combat.h`)**

```cpp
#pragma once
#include "../../../Packets/PacketBase.h"
#include "../../../protocol_shared/Opcodes.h"
#include <cstdint>

// ── CMSG_ATTACK (0x0501) ──────────────────────────────────────────────────────
// C→S (TCP) [uint64 targetPawnId]
struct CMsg_Attack
{
    uint64_t targetPawnId;

    static CMsg_Attack Decode(PacketReader& r);
};

// ── SMSG_ATTACK_RESULT (0x0502) ───────────────────────────────────────────────
// S→C (TCP) [uint64 attackerPawnId][uint64 targetPawnId][int32 damage][uint32 remainHp]
class SMsg_AttackResult : public PacketWriter
{
public:
    SMsg_AttackResult(uint64_t attackerPawnId,
                      uint64_t targetPawnId,
                      int32_t  damage,
                      uint32_t remainHp);
};
```

**소스 (`Packet-Combat.cpp`)**

```cpp
#include "Packet-Combat.h"

CMsg_Attack CMsg_Attack::Decode(PacketReader& r)
{
    CMsg_Attack msg;
    msg.targetPawnId = r.ReadUInt64();
    return msg;
}

SMsg_AttackResult::SMsg_AttackResult(uint64_t attackerPawnId,
                                      uint64_t targetPawnId,
                                      int32_t  damage,
                                      uint32_t remainHp)
    : PacketWriter(SMSG_ATTACK_RESULT)
{
    WriteUInt64(attackerPawnId)
        .WriteUInt64(targetPawnId)
        .WriteUInt32(static_cast<uint32_t>(damage))   // int32 → uint32 재해석; 클라에서 부호 복원
        .WriteUInt32(remainHp);
}
```

> **주의**: WriteInt32 가 없으므로 음수 damage 는 WriteUInt32(bit_cast) 후 클라이언트에서 복원한다.  
> 필요 시 `PacketBase.h` 에 WriteInt32/ReadInt32 를 추가하면 된다.

---

### 3단계 — 서버 핸들러 연결

#### 3-A. 수신 핸들러 (CMSG — SessionActor 또는 ZoneActor)

`SessionActor.cpp` 의 `Handle(MsgNet_PacketReceived&)` switch 에 case 추가:

```cpp
case CMSG_ATTACK:
{
    auto msg = CMsg_Attack::Decode(reader);
    if (auto* zone = m_zone.load(std::memory_order_acquire))
        zone->Post(MsgSession_Attack{ m_sessionId, msg.targetPawnId });
    break;
}
```

`ZoneActor.h` 에 메시지 핸들러 선언:

```cpp
void Handle(MsgSession_Attack& msg);
```

`ZoneActor.cpp` 에 핸들러 구현:

```cpp
void ZoneActor::Handle(MsgSession_Attack& msg)
{
    auto* attacker = FindPlayerPawn(msg.sessionId);
    auto* target   = FindPawnById(msg.targetPawnId);
    if (!attacker || !target) return;

    const int32_t damage = attacker->GetAttack() - target->GetDefense();
    target->ApplyDamage(damage);

    BroadcastTcp(0, SMsg_AttackResult(
        attacker->GetPawnId(), target->GetPawnId(),
        damage, static_cast<uint32_t>(target->GetHp())
    ).Finalize());
}
```

#### 3-B. 송신 호출 (SMSG — 이미 핸들러 내에서 처리)

송신은 핸들러 내부에서 `SMsg_*(...).Finalize()` 를 직접 호출하거나  
`BroadcastTcp` / `BroadcastUdp` 에 전달한다.  
단일 세션 대상이면:

```cpp
if (auto sa = pawn->GetSession())
    sa->Post(MsgZone_SendTcp{ SMsg_AttackResult(/* ... */).Finalize() });
```

---

### 4단계 — 클라이언트 핸들러 연결

#### 4-A. 더미클라이언트 (`NexusClient/DummyClient/main.cpp`)

```cpp
case SMSG_ATTACK_RESULT:
{
    const uint64_t attackerPawnId = r.ReadU64();
    const uint64_t targetPawnId   = r.ReadU64();
    const int32_t  damage         = static_cast<int32_t>(r.ReadU32());
    const uint32_t remainHp       = r.ReadU32();
    std::printf("[←] SMSG_ATTACK_RESULT  attacker=%llu  target=%llu  dmg=%d  hp=%u\n",
                (unsigned long long)attackerPawnId,
                (unsigned long long)targetPawnId,
                damage, remainHp);

    // UE 가이드:
    //   ANexusPawn* Target = FindPawnByPawnId(targetPawnId);
    //   if (Target) Target->PlayHitReaction(damage);
    break;
}
```

#### 4-B. UE 클라이언트 연동 포인트

| 패킷 | UE 호출 |
|------|---------|
| `SMSG_SPAWN_PLAYER` | `GetWorld()->SpawnActor<ANexusOtherPlayer>(Class, SpawnTransform)` |
| `SMSG_DESPAWN_PLAYER` | `Actor->Destroy()` |
| `SMSG_MOVE_BROADCAST` | `Actor->SetActorLocationAndRotation(Loc, Rot)` |
| `SMSG_MOVE_UDP` | `Actor->SetNetworkTargetLocation(Loc)` + Tick 보간 |
| `SMSG_ATTACK_RESULT` | `Target->PlayHitReaction(damage)` |

---

## CMakeLists.txt 등록

새 `.cpp` 파일은 반드시 `NexusEngine/CMakeLists.txt` 의 `SOURCES` 에 추가한다.  
헤더는 `HEADERS` 에 추가한다 (IDE 탐색용, 빌드 필수 아님).

```cmake
set(SOURCES
    # ...
    Game/Data/GamePackets/Packet-Auth.cpp
    Game/Data/GamePackets/Packet-Movement.cpp
    Game/Data/GamePackets/Packet-Chat.cpp
    Game/Data/GamePackets/Packet-Spawn.cpp
    Game/Data/GamePackets/Packet-Combat.cpp   # 새로 추가한 그룹
)
```

---

## 자동화 후크 (향후 코드 생성기 연동)

아래 규칙을 지키면 Opcode → 패킷 클래스 → 핸들러 stub 을 자동 생성하는 스크립트 작성이 가능하다.

1. **Opcodes.h 주석 형식** — `C→S` / `S→C` 와 `[타입 필드명]` 패턴을 유지  
2. **클래스 명명** — `CMsg_{PascalCase}` / `SMsg_{PascalCase}` 엄수  
3. **그룹 파일** — 새 Opcode 그룹은 반드시 `Packet-{Group}.h/cpp` 쌍으로 생성  
4. **Decode 시그니처** — `static CMsg_Foo Decode(PacketReader& r)` 고정  
5. **Encode 시그니처** — `SMsg_Bar(필드...)` 생성자 + `Finalize()` 고정
