// NexusLauncher Tauri 백엔드 진입점
//
// - 상수: 서버 주소·포트 등 설정값
// - 모듈: arbiter (TCP 클라이언트), commands (Tauri 커맨드 정의)
// - Builder: managed state 등록, 커맨드 등록

mod arbiter;
mod commands;
mod server;

use arbiter::ArbiterState;

// ─── 서버 연결 상수 ───────────────────────────────────────────────────────────
pub const SERVER_HOST:    &str = "127.0.0.1";
pub const SERVER_TCP_PORT: u16 = 7070;
pub const SERVER_UDP_PORT: u16 = 7071;
pub const ARBITER_PORT:    u16 = 7072;

// ─── Tauri 앱 초기화 ──────────────────────────────────────────────────────────
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        // Arbiter 연결 상태를 앱 전체에서 공유
        .manage(ArbiterState::new())
        .invoke_handler(tauri::generate_handler![
            commands::connect_arbiter,
            commands::disconnect_arbiter,
            commands::get_server_status,
            commands::kick_player,
            commands::list_server_processes,
            commands::kill_server_processes,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
