// Arbiter TCP 클라이언트
//
// NexusEngine Arbiter(포트 7072)에 연결해 런처 관리 기능을 제공한다.
// 패킷 형식: [uint16 size LE][uint16 opcode LE][payload...]
// 문자열:   [uint16 len LE][UTF-8 bytes]

use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, tcp::OwnedWriteHalf};
use tokio::sync::{Mutex, oneshot};
use tauri::{AppHandle, Emitter};
use serde::Serialize;

// ─── 연결 상수 ────────────────────────────────────────────────────────────────
pub const ARBITER_HOST: &str = "127.0.0.1";
pub const ARBITER_PORT: u16  = 7072;

// ─── 오피코드 (protocol_shared/ArbiterOpcodes.h 와 동기화 유지) ──────────────
const LMSG_AUTH: u16                = 0x0F01;
const AMSG_AUTH_RESULT: u16         = 0x0F02;
const LMSG_GET_STATUS: u16          = 0x0F03;
const AMSG_STATUS: u16              = 0x0F04;
const LMSG_KICK_PLAYER: u16         = 0x0F05;
const AMSG_KICK_RESULT: u16         = 0x0F06;
const AMSG_EVENT_SERVER_READY: u16  = 0x0F07;
const AMSG_EVENT_PLAYER_JOIN: u16   = 0x0F08;
const AMSG_EVENT_PLAYER_LEAVE: u16  = 0x0F09;
const LMSG_GET_PLAYERS: u16         = 0x0F0A;
const AMSG_PLAYERS: u16             = 0x0F0B;

// ─── Svelte로 emit되는 이벤트 페이로드 타입 ──────────────────────────────────

#[derive(Serialize, Clone)]
pub struct ServerStatusPayload {
    pub player_count:   u32,
    pub uptime_seconds: u32,
}

#[derive(Serialize, Clone)]
pub struct PlayerJoinPayload {
    pub session_id: u64,
    pub name:       String,
}

#[derive(Serialize, Clone)]
pub struct PlayerLeavePayload {
    pub session_id: u64,
}

#[derive(Serialize, Clone)]
pub struct PlayerEntry {
    pub session_id: u64,
    pub name:       String,
}

#[derive(Serialize, Clone)]
pub struct PlayersSnapshotPayload {
    pub players: Vec<PlayerEntry>,
}

/// kick_player 커맨드 반환 타입
#[derive(Serialize, Clone)]
pub struct KickResult {
    pub success: bool,
    pub message: String,
}

// ─── 내부 상태 (커맨드 응답 대기 채널 보관) ──────────────────────────────────
struct ArbiterInner {
    connected:      bool,
    pending_status: Option<oneshot::Sender<ServerStatusPayload>>,
    pending_kick:   Option<oneshot::Sender<KickResult>>,
}

impl ArbiterInner {
    fn new() -> Self {
        Self {
            connected:      false,
            pending_status: None,
            pending_kick:   None,
        }
    }
}

// ─── Tauri managed state ──────────────────────────────────────────────────────

/// Tauri가 관리하는 Arbiter 연결 상태.
/// `inner`는 연결 플래그와 대기 채널, `writer`는 송신 전용 (I/O 중 inner를 홀드하지 않기 위해 분리).
pub struct ArbiterState {
    inner:  Arc<Mutex<ArbiterInner>>,
    writer: Arc<Mutex<Option<OwnedWriteHalf>>>,
}

impl ArbiterState {
    pub fn new() -> Self {
        Self {
            inner:  Arc::new(Mutex::new(ArbiterInner::new())),
            writer: Arc::new(Mutex::new(None)),
        }
    }
}

// ─── 패킷 직렬화 헬퍼 ────────────────────────────────────────────────────────

/// 헤더 포함 패킷 바이트열 조립
fn build_packet(opcode: u16, payload: &[u8]) -> Vec<u8> {
    let total = (4 + payload.len()) as u16;
    let mut buf = Vec::with_capacity(total as usize);
    buf.extend_from_slice(&total.to_le_bytes());
    buf.extend_from_slice(&opcode.to_le_bytes());
    buf.extend_from_slice(payload);
    buf
}

/// 문자열 페이로드 직렬화 (uint16 길이 프리픽스 + UTF-8)
fn encode_string(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(2 + bytes.len());
    out.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
    out.extend_from_slice(bytes);
    out
}

/// 페이로드에서 문자열 역직렬화 (uint16 길이 프리픽스 + UTF-8)
fn decode_string(payload: &[u8], pos: &mut usize) -> Option<String> {
    if *pos + 2 > payload.len() { return None; }
    let len = u16::from_le_bytes([payload[*pos], payload[*pos + 1]]) as usize;
    *pos += 2;
    if *pos + len > payload.len() { return None; }
    let s = String::from_utf8_lossy(&payload[*pos..*pos + len]).into_owned();
    *pos += len;
    Some(s)
}

// ─── 수신 루프 ────────────────────────────────────────────────────────────────

/// 백그라운드 태스크로 실행. 패킷을 조립해 dispatch_packet에 전달.
async fn read_loop(
    mut read_half: tokio::net::tcp::OwnedReadHalf,
    inner:      Arc<Mutex<ArbiterInner>>,
    writer:     Arc<Mutex<Option<OwnedWriteHalf>>>,
    app_handle: AppHandle,
) {
    loop {
        // 헤더 4바이트 읽기
        let mut header = [0u8; 4];
        if read_half.read_exact(&mut header).await.is_err() {
            break;
        }

        let size   = u16::from_le_bytes([header[0], header[1]]) as usize;
        let opcode = u16::from_le_bytes([header[2], header[3]]);

        if size < 4 { break; }

        // 페이로드 읽기
        let payload_len = size - 4;
        let mut payload = vec![0u8; payload_len];
        if payload_len > 0 && read_half.read_exact(&mut payload).await.is_err() {
            break;
        }

        dispatch_packet(opcode, &payload, &inner, &writer, &app_handle).await;
    }

    // 연결 정리 — 대기 중인 모든 채널을 드롭해 에러 전파
    {
        let mut g = inner.lock().await;
        g.connected      = false;
        g.pending_status = None;
        g.pending_kick   = None;
    }
    *writer.lock().await = None;

    let _ = app_handle.emit("arbiter_disconnected", ());
}

/// 수신된 패킷 오피코드별 처리
async fn dispatch_packet(
    opcode:     u16,
    payload:    &[u8],
    inner:      &Arc<Mutex<ArbiterInner>>,
    writer:     &Arc<Mutex<Option<OwnedWriteHalf>>>,
    app_handle: &AppHandle,
) {
    match opcode {
        // ── 인증 결과 ─────────────────────────────────────────────────────────
        AMSG_AUTH_RESULT => {
            if payload.is_empty() { return; }
            let success = payload[0] != 0;
            let mut pos = 1;
            let message = decode_string(payload, &mut pos).unwrap_or_default();

            if success {
                let _ = app_handle.emit("arbiter_connected", ());
                // 인증 직후 현재 접속자 목록 스냅샷 요청
                let packet = build_packet(LMSG_GET_PLAYERS, &[]);
                let mut wg = writer.lock().await;
                if let Some(w) = wg.as_mut() {
                    let _ = w.write_all(&packet).await;
                }
            }
            // 실패 시 read_loop가 종료되면서 arbiter_disconnected가 emit됨
            let _ = app_handle.emit("arbiter_auth_result", serde_json::json!({
                "success": success,
                "message": message,
            }));
        }

        // ── 서버 상태 응답 ────────────────────────────────────────────────────
        AMSG_STATUS => {
            if payload.len() < 8 { return; }
            let player_count   = u32::from_le_bytes(payload[0..4].try_into().unwrap());
            let uptime_seconds = u32::from_le_bytes(payload[4..8].try_into().unwrap());
            let data = ServerStatusPayload { player_count, uptime_seconds };

            // 커맨드 대기 채널에 전달
            let tx = inner.lock().await.pending_status.take();
            if let Some(tx) = tx { let _ = tx.send(data.clone()); }

            // 이벤트로도 브로드캐스트 (폴링 없이 실시간 업데이트 가능)
            let _ = app_handle.emit("server_status", data);
        }

        // ── 킥 결과 ───────────────────────────────────────────────────────────
        AMSG_KICK_RESULT => {
            if payload.is_empty() { return; }
            let success = payload[0] != 0;
            let mut pos = 1;
            let message = decode_string(payload, &mut pos).unwrap_or_default();
            let result = KickResult { success, message };

            let tx = inner.lock().await.pending_kick.take();
            if let Some(tx) = tx { let _ = tx.send(result); }
        }

        // ── 서버 준비 푸시 ────────────────────────────────────────────────────
        AMSG_EVENT_SERVER_READY => {
            let _ = app_handle.emit("server_ready", ());
        }

        // ── 플레이어 접속 푸시 ────────────────────────────────────────────────
        AMSG_EVENT_PLAYER_JOIN => {
            if payload.len() < 8 { return; }
            let session_id = u64::from_le_bytes(payload[0..8].try_into().unwrap());
            let mut pos = 8;
            let name = decode_string(payload, &mut pos).unwrap_or_default();
            let _ = app_handle.emit("player_joined", PlayerJoinPayload { session_id, name });
        }

        // ── 플레이어 퇴장 푸시 ────────────────────────────────────────────────
        AMSG_EVENT_PLAYER_LEAVE => {
            if payload.len() < 8 { return; }
            let session_id = u64::from_le_bytes(payload[0..8].try_into().unwrap());
            let _ = app_handle.emit("player_left", PlayerLeavePayload { session_id });
        }

        // ── 접속자 목록 스냅샷 ────────────────────────────────────────────────
        AMSG_PLAYERS => {
            if payload.len() < 4 { return; }
            let count = u32::from_le_bytes(payload[0..4].try_into().unwrap()) as usize;
            let mut pos = 4;
            let mut players = Vec::with_capacity(count);
            for _ in 0..count {
                if pos + 8 > payload.len() { break; }
                let session_id = u64::from_le_bytes(payload[pos..pos + 8].try_into().unwrap());
                pos += 8;
                let name = decode_string(payload, &mut pos).unwrap_or_default();
                players.push(PlayerEntry { session_id, name });
            }
            let _ = app_handle.emit("players_snapshot", PlayersSnapshotPayload { players });
        }

        _ => {} // 알 수 없는 오피코드 무시
    }
}

// ─── 공개 API ─────────────────────────────────────────────────────────────────

/// Arbiter에 연결하고 인증 패킷을 전송한다.
/// 이미 연결된 경우 즉시 Ok 반환.
pub async fn connect(state: &ArbiterState, app_handle: AppHandle) -> Result<(), String> {
    {
        if state.inner.lock().await.connected {
            return Ok(());
        }
    }

    let addr = format!("{}:{}", ARBITER_HOST, ARBITER_PORT);
    let stream = TcpStream::connect(&addr)
        .await
        .map_err(|e| format!("Arbiter 연결 실패: {}", e))?;

    let (read_half, write_half) = stream.into_split();

    *state.writer.lock().await = Some(write_half);
    state.inner.lock().await.connected = true;

    // 수신 루프 백그라운드 태스크 시작
    let inner_c  = Arc::clone(&state.inner);
    let writer_c = Arc::clone(&state.writer);
    let app_c    = app_handle.clone();
    tokio::spawn(async move {
        read_loop(read_half, inner_c, writer_c, app_c).await;
    });

    // LMSG_AUTH 전송 (현재 서버는 시크릿 검증 TODO — 빈 문자열)
    let packet = build_packet(LMSG_AUTH, &encode_string(""));
    send_raw(state, &packet).await
}

/// 서버 상태를 조회하고 응답을 기다려 반환한다.
pub async fn get_status(state: &ArbiterState) -> Result<ServerStatusPayload, String> {
    let (tx, rx) = oneshot::channel();

    {
        let mut g = state.inner.lock().await;
        if !g.connected { return Err("Arbiter에 연결되어 있지 않음".into()); }
        if g.pending_status.is_some() { return Err("이미 상태 조회 중".into()); }
        g.pending_status = Some(tx);
    }

    let packet = build_packet(LMSG_GET_STATUS, &[]);
    send_raw(state, &packet).await?;

    rx.await.map_err(|_| "상태 응답 대기 중 연결 끊김".into())
}

/// 플레이어를 킥하고 결과를 기다려 반환한다.
pub async fn kick_player(
    state:      &ArbiterState,
    session_id: u64,
    reason:     &str,
) -> Result<KickResult, String> {
    let (tx, rx) = oneshot::channel();

    {
        let mut g = state.inner.lock().await;
        if !g.connected { return Err("Arbiter에 연결되어 있지 않음".into()); }
        if g.pending_kick.is_some() { return Err("이미 킥 처리 중".into()); }
        g.pending_kick = Some(tx);
    }

    let mut payload = Vec::new();
    payload.extend_from_slice(&session_id.to_le_bytes());
    payload.extend_from_slice(&encode_string(reason));
    let packet = build_packet(LMSG_KICK_PLAYER, &payload);
    send_raw(state, &packet).await?;

    rx.await.map_err(|_| "킥 응답 대기 중 연결 끊김".into())
}

/// Arbiter 연결을 명시적으로 해제한다.
pub async fn disconnect(state: &ArbiterState) {
    *state.writer.lock().await = None;
    let mut g = state.inner.lock().await;
    g.connected      = false;
    g.pending_status = None;
    g.pending_kick   = None;
}

/// writer 뮤텍스만 잠그고 전송 (inner 뮤텍스와 교차 잠금 없음)
async fn send_raw(state: &ArbiterState, data: &[u8]) -> Result<(), String> {
    let mut w = state.writer.lock().await;
    match w.as_mut() {
        Some(writer) => writer
            .write_all(data)
            .await
            .map_err(|e| format!("Arbiter 전송 실패: {}", e)),
        None => Err("Arbiter에 연결되어 있지 않음".into()),
    }
}
