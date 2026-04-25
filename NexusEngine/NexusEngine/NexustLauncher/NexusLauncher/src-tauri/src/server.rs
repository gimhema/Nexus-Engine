// 로컬 서버 프로세스 관리 — 프로세스 조회 및 강제 종료
//
// sysinfo 크레이트로 Windows/Linux 양쪽에서 동작한다.
// - list_server_processes : 실행 중인 NexusEngine 프로세스 목록 반환
// - kill_all_server_processes : 발견된 모든 NexusEngine 프로세스를 강제 종료

use serde::Serialize;
use sysinfo::{ProcessesToUpdate, System};

// 플랫폼별 서버 실행 파일명
#[cfg(windows)]
const SERVER_EXE: &str = "NexusEngine.exe";
#[cfg(not(windows))]
const SERVER_EXE: &str = "NexusEngine";

#[derive(Debug, Clone, Serialize)]
pub struct ServerProcess {
    pub pid:  u32,
    pub name: String,
}

/// 현재 실행 중인 NexusEngine 프로세스 목록을 반환한다.
pub fn list_server_processes() -> Vec<ServerProcess> {
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);

    sys.processes()
        .values()
        .filter(|p| p.name().to_string_lossy().eq_ignore_ascii_case(SERVER_EXE))
        .map(|p| ServerProcess {
            pid:  p.pid().as_u32(),
            name: p.name().to_string_lossy().into_owned(),
        })
        .collect()
}

/// 발견된 모든 NexusEngine 프로세스를 강제 종료(SIGKILL / TerminateProcess)한다.
/// 실제로 종료된 프로세스 수를 반환한다.
pub fn kill_all_server_processes() -> u32 {
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);

    sys.processes()
        .values()
        .filter(|p| p.name().to_string_lossy().eq_ignore_ascii_case(SERVER_EXE))
        .filter(|p| p.kill())
        .count() as u32
}
