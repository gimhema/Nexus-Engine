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
