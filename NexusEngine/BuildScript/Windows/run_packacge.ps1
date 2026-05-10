# NexusEngine Release 패키징 스크립트 (Windows)
# Release 빌드 결과물을 RELEASE\ 디렉토리에 복사합니다.
#
# 사전 조건:
#   source_build.ps1                                 — 게임서버 / 더미클라이언트 Release 빌드
#   (NexusDBRelay) cargo build --release             — DB 릴레이 Release 빌드

$ErrorActionPreference = "Stop"

$RepoRoot   = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$BuildDir   = Join-Path $RepoRoot "build\windows\Release"
$ReleaseDir = Join-Path $RepoRoot "RELEASE"

$ServerBin  = Join-Path $BuildDir   "NexusEngine\NexusEngine.exe"
$ClientBin  = Join-Path $BuildDir   "NexusEngine\NexusClient\DummyClient\NexusDummyClient.exe"
$DBRelayBin = Join-Path $RepoRoot   "NexusDBRelay\NexusDBRelay\target\release\NexusDBRelay.exe"
$DBRelayCfg = Join-Path $RepoRoot   "NexusDBRelay\NexusDBRelay\config.toml"

# ── 바이너리 존재 확인 ────────────────────────────────────────────────────────
function Check-Bin($Path) {
    if (-not (Test-Path $Path)) {
        Write-Error "[오류] 바이너리 없음: $Path`n       Release 빌드를 먼저 실행하세요."
        exit 1
    }
}

Write-Host "[Packager] 바이너리 확인 중..."
Check-Bin $ServerBin
Check-Bin $ClientBin
Check-Bin $DBRelayBin

# ── RELEASE 디렉토리 복사 ────────────────────────────────────────────────────
Write-Host "[Packager] RELEASE 디렉토리로 복사 중..."

Copy-Item $ServerBin  (Join-Path $ReleaseDir "NexusEngine.exe")
Copy-Item $ClientBin  (Join-Path $ReleaseDir "NexusDummyClient.exe")
Copy-Item $DBRelayBin (Join-Path $ReleaseDir "NexusDBRelay.exe")

# config.toml: 이미 존재하면 덮어쓰지 않음 (운영자 설정 보호)
$CfgDest = Join-Path $ReleaseDir "config.toml"
if (-not (Test-Path $CfgDest)) {
    Copy-Item $DBRelayCfg $CfgDest
    Write-Host "[Packager] config.toml 복사 완료 (신규)"
} else {
    Write-Host "[Packager] config.toml 이미 존재 — 덮어쓰기 건너뜀"
}

# ── 완료 ─────────────────────────────────────────────────────────────────────
Write-Host ""
Write-Host "[Packager] 패키징 완료 → $ReleaseDir"
Write-Host "  NexusEngine.exe"
Write-Host "  NexusDummyClient.exe"
Write-Host "  NexusDBRelay.exe"
