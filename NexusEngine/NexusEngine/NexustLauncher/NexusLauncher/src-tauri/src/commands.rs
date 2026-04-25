// Tauri 커맨드 정의 — Svelte(프론트엔드) ↔ Rust(백엔드) 브릿지
//
// 모든 커맨드는 lib.rs의 invoke_handler에 등록한다.

use serde::Serialize;
use tauri::State;
use crate::arbiter::{ArbiterState, ServerStatusPayload, KickResult};
use crate::server::{
    ServerProcess,
    list_server_processes  as server_list_processes,
    kill_all_server_processes as server_kill_all,
};

/// Arbiter에 연결하고 인증을 완료한다.
/// 성공 시 Svelte로 "arbiter_connected" 이벤트가 emit된다.
#[tauri::command]
pub async fn connect_arbiter(
    state:      State<'_, ArbiterState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    crate::arbiter::connect(&state, app_handle).await
}

/// Arbiter 연결을 해제한다.
#[tauri::command]
pub async fn disconnect_arbiter(
    state: State<'_, ArbiterState>,
) -> Result<(), String> {
    crate::arbiter::disconnect(&state).await;
    Ok(())
}

/// 서버 상태(접속자 수, 가동 시간)를 조회해 반환한다.
/// Arbiter가 연결되어 있지 않으면 에러를 반환한다.
#[tauri::command]
pub async fn get_server_status(
    state: State<'_, ArbiterState>,
) -> Result<ServerStatusPayload, String> {
    crate::arbiter::get_status(&state).await
}

/// 특정 플레이어를 킥한다.
#[tauri::command]
pub async fn kick_player(
    state:      State<'_, ArbiterState>,
    session_id: u64,
    reason:     String,
) -> Result<KickResult, String> {
    crate::arbiter::kick_player(&state, session_id, &reason).await
}

/// 실행 중인 NexusEngine 프로세스 목록을 반환한다.
/// Arbiter 연결 여부와 무관하게 항상 호출 가능.
#[tauri::command]
pub async fn list_server_processes() -> Result<Vec<ServerProcess>, String> {
    // sysinfo는 동기 API이므로 블로킹 스레드에서 실행
    tokio::task::spawn_blocking(server_list_processes)
        .await
        .map_err(|e| e.to_string())
}

#[derive(Debug, Serialize)]
pub struct KillResult {
    pub killed: u32,
}

/// 실행 중인 모든 NexusEngine 프로세스를 강제 종료한다.
#[tauri::command]
pub async fn kill_server_processes() -> Result<KillResult, String> {
    let killed = tokio::task::spawn_blocking(server_kill_all)
        .await
        .map_err(|e| e.to_string())?;
    Ok(KillResult { killed })
}
