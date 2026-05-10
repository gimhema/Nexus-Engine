#!/bin/bash
# NexusEngine Release 패키징 스크립트
# Release 빌드 결과물을 RELEASE/ 디렉토리에 복사합니다.
#
# 사전 조건:
#   source_build.sh Release                          — 게임서버 / 더미클라이언트 Release 빌드
#   (NexusDBRelay) cargo build --release             — DB 릴레이 Release 빌드

set -e

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BUILD_DIR="$REPO_ROOT/build/linux/Release"
RELEASE_DIR="$REPO_ROOT/RELEASE"

SERVER_BIN="$BUILD_DIR/NexusEngine/NexusEngine"
CLIENT_BIN="$BUILD_DIR/NexusEngine/NexusClient/DummyClient/NexusDummyClient"
DBRELAY_BIN="$REPO_ROOT/NexusDBRelay/NexusDBRelay/target/release/NexusDBRelay"
DBRELAY_CFG="$REPO_ROOT/NexusDBRelay/NexusDBRelay/config.toml"

# ── 바이너리 존재 확인 ────────────────────────────────────────────────────────
check_bin() {
    if [ ! -f "$1" ]; then
        echo "[오류] 바이너리 없음: $1"
        echo "       Release 빌드를 먼저 실행하세요."
        exit 1
    fi
}

echo "[Packager] 바이너리 확인 중..."
check_bin "$SERVER_BIN"
check_bin "$CLIENT_BIN"
check_bin "$DBRELAY_BIN"

# ── RELEASE 디렉토리 복사 ────────────────────────────────────────────────────
echo "[Packager] RELEASE 디렉토리로 복사 중..."

cp "$SERVER_BIN"  "$RELEASE_DIR/NexusEngine"
cp "$CLIENT_BIN"  "$RELEASE_DIR/NexusDummyClient"
cp "$DBRELAY_BIN" "$RELEASE_DIR/NexusDBRelay"

# config.toml: 이미 존재하면 덮어쓰지 않음 (운영자 설정 보호)
if [ ! -f "$RELEASE_DIR/config.toml" ]; then
    cp "$DBRELAY_CFG" "$RELEASE_DIR/config.toml"
    echo "[Packager] config.toml 복사 완료 (신규)"
else
    echo "[Packager] config.toml 이미 존재 — 덮어쓰기 건너뜀"
fi

# ── 완료 ─────────────────────────────────────────────────────────────────────
echo ""
echo "[Packager] 패키징 완료 → $RELEASE_DIR"
echo "  NexusEngine"
echo "  NexusDummyClient"
echo "  NexusDBRelay"
