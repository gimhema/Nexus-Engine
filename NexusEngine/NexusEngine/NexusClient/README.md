# NexusClient

NexusEngine 서버와 통신하는 클라이언트 라이브러리 및 더미 클라이언트입니다.

## 디렉토리 구조

```
NexusClient/
├── ClientLib/                  # 이식 가능한 정적 라이브러리
│   ├── Platform/Platform.h     # 소켓 타입 추상화 (Windows / Linux 공통)
│   ├── Packets/
│   │   ├── PacketBase.h        # 패킷 직렬화 / 역직렬화
│   │   └── PacketBase.cpp
│   ├── NetClient.h             # TCP 클라이언트 인터페이스
│   └── NetClient.cpp
└── DummyClient/                # 테스트용 실행 파일
    ├── Packets/GamePackets.hpp # 서버와 동기화된 Opcode 정의
    └── main.cpp                # 자동화 접속 테스트 시나리오
```

---

## 빌드

NexusClient는 NexusEngine 루트 CMake 프로젝트에 포함되어 있으므로, **서버와 함께 빌드**됩니다.

### Linux

```bash
# 저장소 루트 기준
cd NexusEngine

# Debug (기본값)
./BuildScript/Linux/source_build.sh

# Release
./BuildScript/Linux/source_build.sh Release
```

빌드 결과물:
```
build/linux/Debug/
├── NexusEngine/NexusEngine          # 서버 실행 파일
└── NexusEngine/NexusClient/
    ├── ClientLib/libNexusClientLib.a  # 정적 라이브러리
    └── DummyClient/NexusDummyClient   # 더미 클라이언트 실행 파일
```

수동 cmake:
```bash
cmake -S . -B build/linux/Debug -DCMAKE_BUILD_TYPE=Debug -G "Unix Makefiles"
cmake --build build/linux/Debug --parallel
```

### Windows

Visual Studio 2022에서 `NexusEngine.sln`을 열거나:
```
msbuild NexusEngine.sln /p:Configuration=Debug /p:Platform=x64
```

빌드 결과물:
```
build/windows/Debug/
├── NexusEngine/NexusEngine.exe
└── NexusEngine/NexusClient/
    ├── ClientLib/NexusClientLib.lib
    └── DummyClient/NexusDummyClient.exe
```

---

## 실행

서버와 더미 클라이언트를 **각각 별도 터미널**에서 실행합니다.

### 1. 서버 실행

```bash
./build/linux/Debug/NexusEngine/NexusEngine
```

서버가 준비되면 다음 로그가 출력됩니다:
```
[INFO] 서버 시작 (TCP:7070, UDP:7071)
```

### 2. 더미 클라이언트 실행

```bash
./build/linux/Debug/NexusEngine/NexusClient/DummyClient/NexusDummyClient
```

**정상 실행 시 출력 예시:**
```
=== NexusClient DummyClient ===
서버 접속 시도: 127.0.0.1:7070

[연결] 서버 접속 성공
[→] CMSG_LOGIN  account=test_user
[←] SMSG_LOGIN_RESULT  success=1 message="OK"
[→] CMSG_ENTER_WORLD  characterId=1
[←] SMSG_ENTER_WORLD  success=1 pos=(0.0, 0.0, 0.0)
[→] CMSG_CHAT  text="안녕하세요!"
[→] CMSG_MOVE  pos=(10.0, 0.0, 5.0)

테스트 종료 — 연결 해제 중...
```

### 접속 대상 서버 변경

`DummyClient/main.cpp` 상단의 상수를 수정합니다:

```cpp
static constexpr const char* SERVER_HOST  = "127.0.0.1";  // 서버 IP
static constexpr uint16_t    SERVER_PORT  = 7070;          // 서버 포트
```

---

## 이식

`ClientLib`는 서버 코드에 대한 의존이 없는 독립 라이브러리입니다.  
소켓 추상화(`Platform.h`)와 콜백 인터페이스(`NetClient.h`)만으로 구성되어 있어 다른 프로젝트에 그대로 복사해 사용할 수 있습니다.

### 일반 C++ 프로젝트

1. `ClientLib/` 폴더 전체를 대상 프로젝트에 복사
2. `ClientLib/` 경로를 include 경로에 추가
3. `Packets/PacketBase.cpp`, `NetClient.cpp` 를 빌드 대상에 추가
4. Windows이면 `ws2_32.lib` 링크, Linux이면 `-lpthread` 링크

```cmake
add_subdirectory(ClientLib)
target_link_libraries(YourTarget PRIVATE NexusClientLib)
```

### Unreal Engine

UE는 자체 빌드 시스템(UnrealBuildTool)을 사용하므로 CMake 없이 직접 통합합니다.

**1. 파일 복사**

플러그인 또는 게임 모듈의 `Source/` 아래에 복사합니다:
```
YourPlugin/Source/YourModule/
└── NexusClient/
    ├── Platform/Platform.h
    ├── Packets/PacketBase.h
    ├── Packets/PacketBase.cpp
    ├── NetClient.h
    └── NetClient.cpp
```

**2. Build.cs 설정**

```csharp
public class YourModule : ModuleRules
{
    public YourModule(ReadOnlyTargetRules Target) : base(Target)
    {
        PCHUsage = PCHUsageMode.UseExplicitOrSharedPCHs;

        PublicIncludePaths.Add(Path.Combine(ModuleDirectory, "NexusClient"));

        // Windows Winsock 링크
        if (Target.Platform == UnrealTargetPlatform.Win64)
            PublicSystemLibraries.Add("ws2_32");

        PrivateDependencyModuleNames.AddRange(new string[] { "Core", "CoreUObject" });
    }
}
```

**3. UE 환경 주의사항**

| 항목 | 내용 |
|---|---|
| `std::thread` | UE에서 사용 가능. 단, Game Thread 접근 시 `AsyncTask(ENamedThreads::GameThread, ...)` 경유 필요 |
| 콜백 스레드 | `SetOnPacket` 콜백은 수신 스레드에서 호출됨 — UObject 직접 접근 금지 |
| WSAStartup | `NetClient` 생성자에서 자동 호출. UE가 먼저 초기화했을 경우 중복 호출은 무해함 |
| `Platform.h` 충돌 | UE 자체 `Platform.h`와 이름 충돌 가능. 파일명을 `NexusPlatform.h`로 변경하고 include 경로 수정 |

**4. UE 사용 예시**

```cpp
// MyNetworkComponent.h
#include "NexusClient/NetClient.h"

UCLASS()
class UMyNetworkComponent : public UActorComponent
{
    GENERATED_BODY()

    TUniquePtr<NetClient> Client;

public:
    void BeginPlay() override
    {
        Client = MakeUnique<NetClient>();

        Client->SetOnPacket([this](uint16_t Opcode, std::vector<uint8_t> Payload)
        {
            // 수신 스레드 → Game Thread로 전환
            AsyncTask(ENamedThreads::GameThread, [this, Opcode, Payload = std::move(Payload)]()
            {
                HandlePacket(Opcode, Payload);
            });
        });

        Client->Connect("127.0.0.1", 7070);
    }
};
```

---

## Opcode 동기화

서버의 `Game/Data/GamePackets/Packet-Example.hpp`와  
클라이언트의 `DummyClient/Packets/GamePackets.hpp`의 Opcode 값은 항상 동일해야 합니다.

서버에 새 Opcode를 추가할 때 반드시 클라이언트 파일도 함께 업데이트하세요.
