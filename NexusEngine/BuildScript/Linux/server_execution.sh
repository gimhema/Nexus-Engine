#!/bin/bash
# NexusEngine 서버 실행 스크립트
# 사용법: ./server_execution.sh [Debug|Release]

set -e

CONFIG=${1:-Debug}
REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SERVER_BIN="$REPO_ROOT/build/linux/$CONFIG/NexusEngine/NexusEngine"

if [ ! -f "$SERVER_BIN" ]; then
    echo "[NexusEngine] 바이너리를 찾을 수 없습니다: $SERVER_BIN"
    echo "먼저 source_build.sh 또는 source_build_execution.sh 를 실행하세요."
    exit 1
fi

echo "[NexusEngine] 서버 실행 (${CONFIG}): $SERVER_BIN"
"$SERVER_BIN"
