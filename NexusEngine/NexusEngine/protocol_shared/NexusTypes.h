#pragma once

// ─────────────────────────────────────────────────────────────────────────────
// NexusTypes.h — 서버(표준 C++)와 UE5 클라이언트 공통 타입 추상화
//
// UE5 프로젝트에서는 빌드 설정에 NEXUS_UE5 define을 추가한다.
//   - UE5: Project Settings → C++ → Additional Definitions → NEXUS_UE5
//   - CMake: target_compile_definitions(... NEXUS_UE5)
//
// 사용 예:
//   NexusByte        → uint8  (UE5) / uint8_t  (C++)
//   NexusByteArray   → TArray<uint8> (UE5) / std::vector<uint8_t> (C++)
//   NexusStringType  → FString (UE5) / std::string (C++)
//   NEXUS_MEMCPY     → FMemory::Memcpy (UE5) / std::memcpy (C++)
//   NexusStringFromBytes(data, len) → 공통 문자열 생성 헬퍼
// ─────────────────────────────────────────────────────────────────────────────

#ifdef NEXUS_UE5

// ── UE5 환경 ──────────────────────────────────────────────────────────────────
#include "CoreMinimal.h"

using NexusByte      = uint8;
using NexusByteArray = TArray<uint8>;
using NexusStringType = FString;

#define NEXUS_MEMCPY(dst, src, n) FMemory::Memcpy(dst, src, n)

// 서버 프로토콜 문자열은 UTF-8. ANSICHAR로 읽어 FString으로 변환.
inline FString NexusStringFromBytes(const NexusByte* data, uint32 len)
{
    if (len == 0) return FString();
    FUTF8ToTCHAR conv(reinterpret_cast<const ANSICHAR*>(data), static_cast<int32>(len));
    return FString(conv.Length(), conv.Get());
}

// FString → UTF-8 바이트 배열 (패킷 송신 시 사용)
inline TArray<uint8> NexusStringToBytes(const FString& str)
{
    FTCHARToUTF8 conv(*str);
    const int32 len = conv.Length();
    TArray<uint8> result;
    result.SetNumUninitialized(sizeof(uint16) + len);
    const uint16 u16len = static_cast<uint16>(len);
    FMemory::Memcpy(result.GetData(), &u16len, sizeof(uint16));
    FMemory::Memcpy(result.GetData() + sizeof(uint16), conv.Get(), len);
    return result;
}

#else

// ── 표준 C++ 환경 (서버, DummyClient) ─────────────────────────────────────────
#include <cstdint>
#include <cstring>
#include <string>
#include <vector>

using NexusByte      = uint8_t;
using NexusByteArray = std::vector<uint8_t>;
using NexusStringType = std::string;

#define NEXUS_MEMCPY(dst, src, n) std::memcpy(dst, src, n)

inline std::string NexusStringFromBytes(const NexusByte* data, uint32_t len)
{
    if (len == 0) return std::string();
    return std::string(reinterpret_cast<const char*>(data), len);
}

#endif // NEXUS_UE5
