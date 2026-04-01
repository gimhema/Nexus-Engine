# Nexus-Engine
C++20 MMORPG Server Project

## 빌드

### Linux (Fedora)

```bash
# 의존 패키지 설치
sudo dnf install gcc-c++ cmake make

# 빌드 (Debug)
cd NexusEngine
./BuildScript/Linux/source_build.sh

# 빌드 (Release)
./BuildScript/Linux/source_build.sh Release
```

결과물: `NexusEngine/build/linux/Debug/NexusEngine/NexusEngine`

### Windows

Visual Studio 2022에서 `NexusEngine/NexusEngine.sln`을 열거나:

```
msbuild NexusEngine/NexusEngine.sln /p:Configuration=Debug /p:Platform=x64
```

---

## 프로젝트 파일 갱신 (Windows)

Visual Studio 외부에서 소스 파일을 추가하거나 가져온 경우, `.vcxproj` / `.vcxproj.filters` 를 자동으로 갱신할 수 있습니다.

### source_refresh.ps1

```powershell
cd NexusEngine
.\BuildScript\Windows\source_refresh.ps1
```

**동작 방식**
- `NexusEngine/NexusEngine/` 하위를 재귀 탐색하여 `.cpp` / `.h` / `.hpp` 파일을 수집
- `.vcxproj`에 등록되지 않은 파일만 추가 (기존 항목 변경 없음)
- 파일 경로에 맞게 VS 솔루션 탐색기 필터(`소스 파일\...` / `헤더 파일\...`)를 자동 생성
- `x64`, `.vs`, `Debug`, `Release` 등 빌드 산출물 디렉토리는 제외

**언제 사용하나**
- Claude Code 또는 외부 도구로 소스 파일을 생성한 뒤 VS에서 열기 전
- Git으로 다른 브랜치의 변경사항을 병합한 뒤 프로젝트 파일이 업데이트되지 않은 경우

> 실행 후 Visual Studio가 열려 있다면 프로젝트를 다시 로드하세요.
