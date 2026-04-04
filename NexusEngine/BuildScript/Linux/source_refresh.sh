#!/bin/bash
# NexusEngine Linux source_refresh 스크립트
#
# 소스 디렉토리를 재귀 탐색하여 새로운 .cpp / .h / .hpp 파일을
# NexusEngine/CMakeLists.txt 의 SOURCES / HEADERS 목록에 자동으로 추가합니다.
# - 기존 항목은 변경하지 않음
# - 빌드 산출물 디렉토리(build, x64, Debug, Release 등)는 제외
#
# 사용법:
#   ./source_refresh.sh         # 일반 실행
#   ./source_refresh.sh -v      # 상세 출력 (verbose)

set -euo pipefail

# ── 옵션 파싱 ─────────────────────────────────────────────────────────────────
VERBOSE=0
for arg in "$@"; do
    case "$arg" in -v|--verbose) VERBOSE=1 ;; esac
done

# ── 경로 설정 ─────────────────────────────────────────────────────────────────
# BuildScript/Linux/ 기준으로 소스 루트는 ../../NexusEngine/
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SOURCE_ROOT="$(cd "$SCRIPT_DIR/../../NexusEngine" && pwd)"
CMAKE_FILE="$SOURCE_ROOT/CMakeLists.txt"

# ── 색상 ──────────────────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

echo -e "${CYAN}=== NexusEngine source_refresh ===${NC}"

# ── 사전 검사 ─────────────────────────────────────────────────────────────────
if [[ ! -f "$CMAKE_FILE" ]]; then
    echo -e "${RED}CMakeLists.txt 없음: $CMAKE_FILE${NC}"
    exit 1
fi

if ! command -v python3 &>/dev/null; then
    echo -e "${RED}python3 가 설치되어 있지 않습니다.${NC}"
    exit 1
fi

if [[ $VERBOSE -eq 1 ]]; then
    echo "  소스 루트    : $SOURCE_ROOT"
    echo "  CMakeLists.txt: $CMAKE_FILE"
fi

# ── Python3으로 CMakeLists.txt 파싱 및 갱신 ──────────────────────────────────
export NX_SOURCE_ROOT="$SOURCE_ROOT"
export NX_CMAKE_FILE="$CMAKE_FILE"
export NX_VERBOSE="$VERBOSE"

python3 << 'PYEOF'
import os, sys, re
from pathlib import Path

source_root = Path(os.environ['NX_SOURCE_ROOT'])
cmake_file  = Path(os.environ['NX_CMAKE_FILE'])
verbose     = os.environ.get('NX_VERBOSE', '0') == '1'

# Windows 스크립트와 동일하게 제외할 디렉토리
EXCLUDE_DIRS = {'build', '.git', '.vs', 'x64', 'Debug', 'Release', 'ipch'}

RED    = '\033[0;31m'
GREEN  = '\033[0;32m'
YELLOW = '\033[1;33m'
NC     = '\033[0m'


def collect_files(root: Path) -> tuple[list[str], list[str]]:
    """SOURCE_ROOT 하위 .cpp / .h .hpp 파일 수집 (CMakeLists.txt 기준 상대경로)"""
    cpp_files    = []
    header_files = []

    for path in sorted(root.rglob('*')):
        if not path.is_file():
            continue
        rel   = path.relative_to(root)
        parts = rel.parts
        # 제외 디렉토리 포함 여부 확인 (파일명 제외한 상위 경로)
        if any(p in EXCLUDE_DIRS for p in parts[:-1]):
            continue
        rel_str = '/'.join(parts)  # CMake는 슬래시 사용
        if path.suffix == '.cpp':
            cpp_files.append(rel_str)
        elif path.suffix in ('.h', '.hpp'):
            header_files.append(rel_str)

    return cpp_files, header_files


def parse_cmake_set(content: str, var_name: str) -> set[str]:
    """set(VAR_NAME ...) 블록에서 파일 목록 파싱"""
    # [ \t]*\n 으로 VAR_NAME 뒤의 줄바꿈까지만 prefix에 포함 → body는 실제 항목부터 시작
    pattern = rf'set\s*\(\s*{var_name}[ \t]*\n(.*?)\)'
    m = re.search(pattern, content, re.DOTALL)
    if not m:
        return set()
    return {item.strip() for item in m.group(1).split() if item.strip()}


def insert_into_set_block(content: str, var_name: str, new_files: list[str]) -> str:
    """set(VAR_NAME ...) 블록 끝(닫는 괄호 앞)에 새 파일 삽입"""
    if not new_files:
        return content

    # prefix는 'set(VAR_NAME\n' 까지 — body의 첫 줄이 실제 항목이 되도록
    pattern = rf'(set\s*\(\s*{var_name}[ \t]*\n)(.*?)(\))'

    def replacer(m: re.Match) -> str:
        prefix = m.group(1)
        body   = m.group(2)
        suffix = m.group(3)

        # 기존 항목의 들여쓰기 수준 파악 (첫 번째 실제 항목 기준)
        indent = '    '
        for line in body.splitlines():
            stripped = line.lstrip()
            if stripped:
                indent = line[:len(line) - len(stripped)]
                break

        additions = ''.join(f'\n{indent}{f}' for f in sorted(new_files))
        return f'{prefix}{body.rstrip()}{additions}\n{suffix}'

    return re.sub(pattern, replacer, content, flags=re.DOTALL)


# ── 1. 파일 수집 ──────────────────────────────────────────────────────────────
cpp_files, header_files = collect_files(source_root)
if verbose:
    print(f'  디스크 파일 수: {len(cpp_files) + len(header_files)}')

# ── 2. CMakeLists.txt 읽기 ────────────────────────────────────────────────────
original = cmake_file.read_text(encoding='utf-8')

# ── 3. 기존 등록 목록 파싱 ───────────────────────────────────────────────────
existing_sources = parse_cmake_set(original, 'SOURCES')
existing_headers = parse_cmake_set(original, 'HEADERS')

if verbose:
    print(f'  기존 SOURCES 항목: {len(existing_sources)}개')
    print(f'  기존 HEADERS 항목: {len(existing_headers)}개')

# ── 4. 신규 파일 추려내기 ─────────────────────────────────────────────────────
new_cpp     = [f for f in cpp_files    if f not in existing_sources]
new_headers = [f for f in header_files if f not in existing_headers]

if not new_cpp and not new_headers:
    print(f'{GREEN}새로 추가된 파일 없음 — CMakeLists.txt가 최신 상태입니다.{NC}')
    sys.exit(0)

total = len(new_cpp) + len(new_headers)
print(f'{YELLOW}새 파일 {total}개 발견:{NC}')
for f in sorted(new_cpp):     print(f'  + {f}  (소스)')
for f in sorted(new_headers): print(f'  + {f}  (헤더)')

# ── 5. CMakeLists.txt 갱신 ────────────────────────────────────────────────────
updated = original
updated = insert_into_set_block(updated, 'SOURCES', new_cpp)
updated = insert_into_set_block(updated, 'HEADERS', new_headers)

cmake_file.write_text(updated, encoding='utf-8')
print(f'{GREEN}완료: {total}개 파일 등록, CMakeLists.txt 갱신됨{NC}')
PYEOF
