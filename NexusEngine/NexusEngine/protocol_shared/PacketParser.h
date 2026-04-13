#pragma once

#include "NexusTypes.h"

// ─────────────────────────────────────────────────────────────────────────────
// NexusPacketParser — 수신 패킷 역직렬화 파서
//
// 서버(표준 C++), DummyClient, UE5 클라이언트 모두 동일하게 사용한다.
// NexusTypes.h의 전처리기 분기를 통해 각 환경에 맞는 타입으로 동작한다.
//
// 사용 예 (C++):
//   NexusPacketParser p{ payload.data(), (uint32_t)payload.size() };
//   uint8_t  success = p.ReadU8();
//   uint64_t pawnId  = p.ReadU64();
//   auto     name    = p.ReadString();   // std::string (C++) / FString (UE5)
//
// 사용 예 (UE5 — NEXUS_UE5 define 필요):
//   NexusPacketParser p{ PacketData.GetData(), (uint32_t)PacketData.Num() };
//   uint8   success = p.ReadU8();
//   uint64  pawnId  = p.ReadU64();
//   FString name    = p.ReadString();
//
// 주의:
//   - Little-Endian 전용 (서버 프로토콜과 동일).
//   - 읽기 범위 초과 시 0 / 빈 문자열 반환 후 내부 오류 상태로 진입.
//     HasError()로 확인 가능.
// ─────────────────────────────────────────────────────────────────────────────
struct NexusPacketParser
{
    const NexusByte* data{ nullptr };
    uint32_t         size{ 0 };
    uint32_t         pos{ 0 };
    bool             error{ false };

    // ── 생성 ──────────────────────────────────────────────────────────────────
    NexusPacketParser(const NexusByte* inData, uint32_t inSize)
        : data(inData), size(inSize) {}

    [[nodiscard]] bool HasError()    const { return error; }
    [[nodiscard]] bool IsExhausted() const { return pos >= size; }
    [[nodiscard]] uint32_t Remaining() const { return (pos < size) ? (size - pos) : 0; }

    // ── 정수 읽기 ──────────────────────────────────────────────────────────────
    uint8_t ReadU8()
    {
        if (!Ensure(1)) return 0;
        return data[pos++];
    }

    uint16_t ReadU16()
    {
        if (!Ensure(2)) return 0;
        uint16_t v{};
        NEXUS_MEMCPY(&v, data + pos, 2);
        pos += 2;
        return v;
    }

    uint32_t ReadU32()
    {
        if (!Ensure(4)) return 0;
        uint32_t v{};
        NEXUS_MEMCPY(&v, data + pos, 4);
        pos += 4;
        return v;
    }

    uint64_t ReadU64()
    {
        if (!Ensure(8)) return 0;
        uint64_t v{};
        NEXUS_MEMCPY(&v, data + pos, 8);
        pos += 8;
        return v;
    }

    float ReadFloat()
    {
        if (!Ensure(4)) return 0.f;
        float v{};
        NEXUS_MEMCPY(&v, data + pos, 4);
        pos += 4;
        return v;
    }

    // ── 문자열 읽기 ───────────────────────────────────────────────────────────
    // 반환 타입: std::string (C++) / FString (UE5)
    // 프로토콜 형식: [uint16 길이][UTF-8 바이트...]
    auto ReadString()
    {
        const uint16_t len = ReadU16();
        if (!Ensure(len))
            return NexusStringFromBytes(data, 0);   // 빈 문자열 반환

        auto result = NexusStringFromBytes(data + pos, len);
        pos += len;
        return result;
    }

private:
    bool Ensure(uint32_t bytes)
    {
        if (pos + bytes <= size) return true;
        error = true;
        return false;
    }
};
