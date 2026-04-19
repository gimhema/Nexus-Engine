# NexusLauncher — CLAUDE.md

이 파일은 Claude Code가 NexusLauncher 프로젝트에서 작업할 때 참고하는 가이드입니다.

---

## 프로젝트 개요

NexusLauncher는 Nexus MMORPG의 로컬 서버 호스팅 및 관리를 위한 데스크탑 런처입니다.

### 배경 및 방향성

- 게임은 **멀티플레이 기반 단일 구조** — 싱글/멀티 코드 분기 없음
- 싱글플레이 시 유저가 **로컬 서버를 직접 호스팅**하는 방식
- 멀티플레이(친구 초대) 시 호스트가 IP를 공유하면 됨
- 서버는 런처가 백그라운드 실행 관리. 유저는 런처만 실행하면 됨

### 런처 실행 흐름

```
런처 실행
  └─ 서버 프로세스 백그라운드 spawn
       └─ Arbiter 포트(7072) 응답 확인 (준비 폴링)
            └─ UE5 클라이언트 실행 (localhost 접속)
```

### 클라이언트와의 역할 분리

- **런처**: 서버 생명주기, 호스트 권한 기능, 모니터링
- **UE5 클라이언트**: 서버 주소를 받아 접속만 담당 — 서버 관리 코드 없음

---

## 기술 스택

| 항목 | 선택 | 버전 |
|---|---|---|
| 데스크탑 프레임워크 | Tauri | 2.x |
| 백엔드 언어 | Rust | 1.95.0 |
| UI 프레임워크 | SvelteKit | 2.x |
| UI 언어 | TypeScript | 5.6.x |
| 번들러 | Vite | 6.x |
| 패키지 매니저 | npm | 10.x |

---

## 프로젝트 구조

```
NexustLauncher/
├── CLAUDE.md                          # 이 파일
├── Packages.md                        # 의존성 및 설치 가이드
└── NexusLauncher/                     # Tauri 프로젝트 루트
    ├── package.json
    ├── svelte.config.js
    ├── vite.config.js
    ├── tsconfig.json
    ├── src/                           # 프론트엔드 (SvelteKit + TypeScript)
    │   ├── app.html                   # HTML 진입점
    │   └── routes/
    │       ├── +layout.ts             # SSR 비활성화 (SPA 모드)
    │       └── +page.svelte           # 메인 페이지 (현재 기본 데모)
    ├── static/                        # 정적 자산 (아이콘, 이미지)
    └── src-tauri/                     # Rust 백엔드
        ├── Cargo.toml
        ├── tauri.conf.json            # 앱 설정 (창 크기, 번들 등)
        ├── capabilities/
        │   └── default.json           # 권한 설정
        ├── icons/                     # 플랫폼별 앱 아이콘
        └── src/
            ├── main.rs                # 진입점 (Windows 콘솔 숨김)
            └── lib.rs                 # Tauri Builder + 커맨드 등록
```

### 앞으로 추가할 Rust 모듈 위치

```
src-tauri/src/
├── main.rs
├── lib.rs                # Builder 설정 — 커맨드 등록은 여기서
├── arbiter.rs            # Arbiter TCP 클라이언트 (서버 통신)
├── server.rs             # 서버 프로세스 spawn/종료 (추후)
└── commands.rs           # Tauri 커맨드 모음 (프론트↔백엔드 브릿지)
```

### 앞으로 추가할 프론트엔드 구조

```
src/routes/
├── +layout.ts
├── +page.svelte          # 메인 화면 (서버 상태, 시작/종료)
├── players/
│   └── +page.svelte      # 접속자 목록 / 킥
└── settings/
    └── +page.svelte      # 서버 설정
```

---

## 개발 명령어

프로젝트 루트는 `NexustLauncher/NexusLauncher/` 입니다.

```bash
cd NexustLauncher/NexusLauncher

# 의존성 설치 (최초 1회)
npm install

# 개발 서버 실행 (핫리로드)
cargo tauri dev

# 프로덕션 빌드
cargo tauri build

# 프론트엔드 타입 검사
npm run check

# 프론트엔드만 개발 서버 (Tauri 없이 브라우저로 확인)
npm run dev
```

### ⚠️ tauri.conf.json 수정 필요

현재 `tauri.conf.json`의 빌드 명령이 `deno task`로 잘못 설정되어 있음.
`cargo tauri dev` / `cargo tauri build` 실행 전 반드시 수정:

```json
"build": {
  "beforeDevCommand": "npm run dev",
  "beforeBuildCommand": "npm run build",
  "devUrl": "http://localhost:1420",
  "frontendDist": "../build"
}
```

---

## Tauri 통신 패턴

Rust 백엔드와 Svelte 프론트엔드는 **Tauri Command** 로 통신한다.

### Rust 커맨드 정의 (lib.rs 또는 commands.rs)

```rust
#[tauri::command]
fn get_server_status() -> ServerStatus {
    // Arbiter에 LMSG_GET_STATUS 전송 후 결과 반환
}

// lib.rs의 Builder에 등록
.invoke_handler(tauri::generate_handler![get_server_status])
```

### Svelte에서 호출 (TypeScript)

```typescript
import { invoke } from '@tauri-apps/api/core';

// 커맨드 호출
const status = await invoke<ServerStatus>('get_server_status');

// 이벤트 수신 (Rust → Svelte 푸시)
import { listen } from '@tauri-apps/api/event';
await listen<PlayerEvent>('player_joined', (event) => {
    console.log(event.payload);
});
```

### Rust에서 Svelte로 이벤트 푸시 (Arbiter 이벤트 전달용)

```rust
use tauri::Emitter;

// Arbiter에서 AMSG_EVENT_PLAYER_JOIN 수신 시
app_handle.emit("player_joined", PlayerEvent { session_id, name })?;
```

---

## 서버(NexusEngine) 연동 — Arbiter 프로토콜

NexusEngine에는 **Arbiter** (포트 7072)가 구현되어 있다.
게임 클라이언트↔서버 프로토콜과 동일한 바이너리 포맷을 사용한다.

```
패킷 포맷: [ uint16 size ][ uint16 opcode ][ payload... ]  (Little Endian)
```

### Opcode 목록 (protocol_shared/ArbiterOpcodes.h 참조)

| Opcode | 방향 | 페이로드 | 설명 |
|---|---|---|---|
| `LMSG_AUTH` (0x0F01) | L→S | `[string secret]` | 런처 인증 |
| `AMSG_AUTH_RESULT` (0x0F02) | S→L | `[uint8 success][string message]` | 인증 결과 |
| `LMSG_GET_STATUS` (0x0F03) | L→S | (없음) | 서버 상태 조회 |
| `AMSG_STATUS` (0x0F04) | S→L | `[uint32 playerCount][uint32 uptimeSeconds]` | 서버 상태 |
| `LMSG_KICK_PLAYER` (0x0F05) | L→S | `[uint64 sessionId][string reason]` | 플레이어 킥 |
| `AMSG_KICK_RESULT` (0x0F06) | S→L | `[uint8 success][string message]` | 킥 결과 |
| `AMSG_EVENT_SERVER_READY` (0x0F07) | S→L | (없음) | 서버 준비 완료 푸시 |
| `AMSG_EVENT_PLAYER_JOIN` (0x0F08) | S→L | `[uint64 sessionId][string name]` | 접속 이벤트 푸시 |
| `AMSG_EVENT_PLAYER_LEAVE` (0x0F09) | S→L | `[uint64 sessionId]` | 퇴장 이벤트 푸시 |

### 연동 흐름

```
Svelte UI
  └─ invoke('connect_arbiter')
       └─ Rust: TcpStream::connect("127.0.0.1:7072")
            └─ LMSG_AUTH 전송
                 └─ AMSG_AUTH_RESULT 수신 → 연결 확립
                      └─ 수신 루프: AMSG_EVENT_* → app_handle.emit()
```

### 서버 연결 상수

| 항목 | 값 |
|---|---|
| 게임 TCP 포트 | 7070 |
| 게임 UDP 포트 | 7071 |
| Arbiter 관리 포트 | **7072** |
| 서버 실행 파일 (Linux) | `NexusEngine` |
| 서버 실행 파일 (Windows) | `NexusEngine.exe` |

---

## 현재 구현 상태

| 항목 | 상태 |
|---|---|
| 프로젝트 방향 확정 | ✅ 완료 |
| 기술 스택 확정 | ✅ 완료 |
| Tauri + SvelteKit 프로젝트 생성 | ✅ 완료 |
| tauri.conf.json `deno task` → `npm run` 수정 | ✅ 완료 |
| Arbiter TCP 클라이언트 (Rust) | ✅ 완료 (`src-tauri/src/arbiter.rs`) |
| Arbiter 이벤트 → Svelte 전달 | ✅ 완료 (`commands.rs` + Tauri emit) |
| 서버 상태 표시 UI | ✅ 완료 (`ServerStats.svelte`) |
| 접속자 목록 / 킥 UI | ✅ 완료 (`PlayerList.svelte`) |
| 서버 프로세스 spawn/종료 | ❌ 미착수 |
| UE5 클라이언트 실행 연동 | ❌ 미착수 |

---

## 개발 규칙

- 소스 코드 주석은 한국어로 작성
- Tauri 커맨드 정의는 `commands.rs`에 집중, `lib.rs`에서 등록만
- 서버 주소/포트 등 상수는 `lib.rs` 상단에 모아서 관리
- Svelte 컴포넌트는 `src/lib/components/`에 분리 (재사용 컴포넌트)
- 페이지별 라우트는 `src/routes/` 하위 SvelteKit 컨벤션 준수
