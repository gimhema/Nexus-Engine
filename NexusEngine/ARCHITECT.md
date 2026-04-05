# Nexus-Engine 아키텍처

## 레이어 구조

서버는 두 계층으로 분리됩니다. 네트워크 레이어는 I/O만 담당하고, 게임 로직 레이어는 Actor 모델로 상태를 관리합니다.

```
┌─────────────────────────────────────────────────────────┐
│                      클라이언트                           │
└───────────────────────┬─────────────────────────────────┘
                        │ TCP / UDP 패킷 (바이너리 직렬화)
┌───────────────────────▼─────────────────────────────────┐
│               네트워크 레이어 (ServerNet)                  │
│   epoll / IOCP · 워커 스레드 풀 · 패킷 수신/송신            │
│   패킷 역직렬화 → 내부 메시지 변환 (경계층)                  │
└───────────────────────┬─────────────────────────────────┘
                        │ 내부 메시지 (C++ 값 타입, 직렬화 없음)
┌───────────────────────▼─────────────────────────────────┐
│               게임 로직 레이어 (Actor 모델)                 │
│   SessionActor · WorldActor · ZoneActor                  │
│   메일박스 기반 메시지 패싱 · 락-프리 MPSC 큐               │
└─────────────────────────────────────────────────────────┘
```

---

## Actor 관계도

```mermaid
graph TD
    Client(["👤 클라이언트\n(TCP/UDP)"])

    subgraph NET["네트워크 레이어"]
        direction TB
        NW["NetworkManager\n워커 스레드 풀 (CPU×2)"]
    end

    subgraph GAME["게임 로직 레이어 (Actor 모델)"]
        direction TB
        SA["SessionActor\n[Pooled]\n클라이언트 1개 대응\n패킷 파싱 · 송신 위임"]
        WA["WorldActor\n[Dedicated]\n글로벌 싱글턴\nZone 레지스트리 · 로그인"]
        ZA["ZoneActor\n[Dedicated + Tick 20Hz]\n존 내 플레이어 상태 독점\n이동 · 채팅 · 브로드캐스트"]
    end

    Client <-->|"바이너리 패킷\nCMSG_* / SMSG_*"| NW

    NW -->|"MsgNet_PacketReceived\n(onAccept / onPacket 콜백)"| SA

    SA -->|"MsgSession_Login\nMsgSession_EnterWorld\nMsgSession_Logout"| WA
    SA -->|"MsgSession_Move\nMsgSession_MoveUdp\nMsgSession_Chat"| ZA

    WA -->|"MsgWorld_LoginResult"| SA
    WA -->|"MsgWorld_AddPlayer\nMsgWorld_RemovePlayer"| ZA

    ZA -->|"MsgZone_SendTcp\nMsgZone_SendUdp\nMsgZone_Disconnect"| SA
    ZA -->|"MsgZone_TeleportRequest"| WA

    style NET fill:#1a3a5c,color:#fff,stroke:#2d6a9f
    style GAME fill:#1a3a2a,color:#fff,stroke:#2d7a4f
    style Client fill:#3a1a1a,color:#fff,stroke:#9f2d2d
```

---

## 스레드 모델

```mermaid
graph LR
    subgraph IO["I/O 워커 스레드 (CPU×2)"]
        W1["Worker 1"]
        W2["Worker 2"]
        Wn["Worker N"]
    end

    subgraph POOL["ActorSystem 스레드 풀 (CPU 코어 수)"]
        P1["Pool 1"]
        P2["Pool 2"]
        Pn["Pool N"]
    end

    subgraph DEDICATED["전용 스레드"]
        WAT["WorldActor 스레드\n(조건변수 대기)"]
        ZAT["ZoneActor 스레드\n(50ms Tick 루프)"]
    end

    W1 & W2 & Wn -->|"Post(MsgNet_PacketReceived)"| MB_SA[/"SessionActor\n메일박스"/]
    MB_SA -->|"Schedule(ProcessMailbox)"| P1 & P2 & Pn

    W1 & W2 & Wn -->|"Post(MsgServer_Register*)"| MB_WA[/"WorldActor\n메일박스"/]
    MB_WA --> WAT

    P1 & P2 & Pn -->|"Post(MsgSession_*)"| MB_WA
    P1 & P2 & Pn -->|"Post(MsgSession_*)"| MB_ZA[/"ZoneActor\n메일박스"/]
    MB_ZA --> ZAT

    WAT -->|"Post(MsgWorld_*)"| MB_SA
    WAT -->|"Post(MsgWorld_*)"| MB_ZA
    ZAT -->|"Post(MsgZone_*)"| MB_SA

    style IO fill:#1a2a3a,color:#fff,stroke:#2d5a8f
    style POOL fill:#1a3a2a,color:#fff,stroke:#2d7a4f
    style DEDICATED fill:#2a1a3a,color:#fff,stroke:#6d2d9f
```

> **핵심 원칙**: 각 Actor는 자신의 스레드에서만 상태를 읽고 씁니다.
> 외부에서 직접 접근하지 않고 반드시 `Post()`를 통해 메시지를 보냅니다.

---

## 주요 메시지 흐름

### 로그인 시퀀스

```mermaid
sequenceDiagram
    actor C as 클라이언트
    participant N as NetworkManager
    participant S as SessionActor
    participant W as WorldActor
    participant Z as ZoneActor

    C->>N: CMSG_LOGIN [account, token]
    N->>S: Post(MsgNet_PacketReceived)
    S->>W: Post(MsgSession_Login)
    Note over W: TODO: DB 인증 (Phase 4)
    W->>S: Post(MsgWorld_LoginResult { success })
    S->>C: SMSG_LOGIN_RESULT

    C->>N: CMSG_ENTER_WORLD [characterId]
    N->>S: Post(MsgNet_PacketReceived)
    S->>W: Post(MsgSession_EnterWorld)
    Note over W: 기본 Zone(ID=1) 배정
    W->>S: SetZone(zone)  [atomic store]
    W->>Z: Post(MsgWorld_AddPlayer + weak_ptr<SessionActor>)
    Note over Z: m_sessionActors 등록 (ZoneActor 스레드)
    Z->>S: Post(MsgZone_SendTcp)
    S->>C: SMSG_ENTER_WORLD [pos]
```

### 이동 / 채팅 시퀀스

```mermaid
sequenceDiagram
    actor C as 클라이언트
    participant N as NetworkManager
    participant S as SessionActor
    participant Z as ZoneActor
    actor Others as 다른 클라이언트들

    C->>N: CMSG_MOVE [x, y, z, orientation]
    N->>S: Post(MsgNet_PacketReceived)
    S->>Z: Post(MsgSession_Move)
    Note over Z: PlayerState 갱신 (ZoneActor 독점)
    Z->>S: Post(MsgZone_SendTcp) × N명
    S->>Others: SMSG_MOVE_BROADCAST [sessionId, pos]

    C->>N: CMSG_CHAT [text]
    N->>S: Post(MsgNet_PacketReceived)
    S->>Z: Post(MsgSession_Chat)
    Z->>S: Post(MsgZone_SendTcp) × 전체
    S->>Others: SMSG_CHAT [sessionId, name, text]
```

### 연결 해제 시퀀스

```mermaid
sequenceDiagram
    actor C as 클라이언트
    participant N as NetworkManager
    participant SRV as Server
    participant W as WorldActor
    participant Z as ZoneActor
    participant S as SessionActor

    C->>N: 연결 끊김 (TCP FIN / 오류)
    N->>SRV: onDisconnect(sessionId)
    SRV->>SRV: m_sessionActors.erase() [shared_ptr 해제]
    SRV->>W: Post(MsgServer_UnregisterSession)
    SRV->>W: Post(MsgSession_Logout)
    Note over W: m_sessions.erase() → SessionActor shared_ptr 소멸
    W->>S: ClearZone() [atomic store nullptr]
    W->>Z: Post(MsgWorld_RemovePlayer)
    Note over Z: m_sessionActors에서 weak_ptr 제거\nlock() 실패 시 자동 무효화
```

---

## Actor 별 책임 요약

| Actor | 실행 모드 | 스레드 | 소유 상태 | 주요 역할 |
|---|---|---|---|---|
| `SessionActor` | Pooled | ActorSystem 풀 | 없음 (상태 없는 라우터) | 패킷 파싱, 메시지 라우팅, 클라이언트 송신 |
| `WorldActor` | Dedicated | 전용 1개 | Zone 레지스트리, Session 레지스트리 | 로그인, 존 배정, 텔레포트 중개 |
| `ZoneActor` | Dedicated + Tick | 전용 1개 | `m_players` (PlayerState 맵) | 게임 로직, 이동 검증, AOI 브로드캐스트 |

---

## 패킷 vs 내부 메시지 경계

`SessionActor::Handle(MsgNet_PacketReceived)`가 두 레이어의 경계입니다.

```
클라이언트 패킷 (바이너리)          내부 메시지 (C++ 값 타입)
─────────────────────────     ─────────────────────────────────
직렬화 필요                    직렬화 없음
opcode + payload bytes         타입 안전 struct
최소 크기, MTU 제한             shared_ptr, string 자유롭게 사용
보안 검증 필요                  신뢰된 내부 레이어
클라이언트 호환성 고려           서버 내부 변경 자유
```

```
CMSG_LOGIN [string account][string token]   (바이너리 와이어)
        │
        │  역직렬화 (SessionActor)
        ▼
MsgSession_Login { sessionId, accountName, token }  (C++ 타입)
```

---

## 스레드 안전성 설계

| 항목 | 해결 방법 |
|---|---|
| `WorldActor::m_sessions` 외부 접근 | `MsgServer_RegisterSession` Post로 전환 — WorldActor 스레드 독점 |
| `ZoneActor::m_sessionActors` 외부 등록 | `MsgWorld_AddPlayer`에 `weak_ptr<SessionActor>` 실어서 ZoneActor 스레드 내부 등록 |
| `SessionActor::m_zone` 크로스 스레드 쓰기 | `std::atomic<ZoneActor*>` + acquire/release |
| `SessionActor` 댕글링 포인터 | `m_sessionActors`를 `weak_ptr`로 교체, `lock()` 실패 시 무시 |
| `MpscMailbox` 다중 생산자 | Michael-Scott 알고리즘 lock-free 큐 |
| false sharing | `m_head`/`m_tail` `alignas(64)` 적용 |
