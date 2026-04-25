#!/bin/bash
# NexusEngine 소스 빌드 후 서버 실행 스크립트
# 사용법: ./source_build_execution.sh [Debug|Release]

set -e

CONFIG=${1:-Debug}
REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BUILD_DIR="$REPO_ROOT/build/linux/$CONFIG"
SERVER_BIN="$BUILD_DIR/NexusEngine/NexusEngine"

echo "[NexusEngine] 빌드 시작 (${CONFIG})"

cmake -S "$REPO_ROOT" \
      -B "$BUILD_DIR" \
      -DCMAKE_BUILD_TYPE="$CONFIG" \
      -G "Unix Makefiles"

cmake --build "$BUILD_DIR" --config "$CONFIG" --parallel

echo "[NexusEngine] 빌드 완료 → $SERVER_BIN"
echo "[NexusEngine] 서버 실행 (${CONFIG})"
"$SERVER_BIN"
