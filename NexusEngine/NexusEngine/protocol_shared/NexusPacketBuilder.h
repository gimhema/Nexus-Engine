#pragma once

#include "NexusTypes.h"

#include <cstdint>
#include <cstring>

// ─────────────────────────────────────────────────────────────────────────────
// NexusPacketBuilder — 공용 패킷 직렬화 빌더 (NexusPacketParser 의 역방향)
//
// 서버(표준 C++), DummyClient, UE5 클라이언트 모두 동일하게 사용한다.
// NexusTypes.h 의 타입 추상화를 통해 각 환경에서 동작한다.
//
// TCP 포맷 (Little Endian):
//   [ uint16 size ][ uint16 opcode ][ payload... ]
//   size = 헤더 포함 전체 바이트 수
//
// 사용 예 (C++ / 서버):
//   NexusPacketBuilder b(SMSG_LOGIN_RESULT);
//   b.WriteU8(1).WriteString("OK");
//   session->Send(b.Finalize());
//
// 사용 예 (UE5):
//   NexusPacketBuilder b(CMSG_MOVE);
//   b.WriteFloat(X).WriteFloat(Y).WriteFloat(Z).WriteFloat(Yaw);
//   NetClient->Send(b.Finalize());
// ─────────────────────────────────────────────────────────────────────────────
class NexusPacketBuilder
{
public:
    explicit NexusPacketBuilder(uint16_t opcode)
    {
        // 헤더 4바이트 예약: [uint16 size=0 placeholder][uint16 opcode]
        uint8_t hdr[4];
        const uint16_t zero = 0;
        NEXUS_MEMCPY(hdr,     &zero,   sizeof(uint16_t));
        NEXUS_MEMCPY(hdr + 2, &opcode, sizeof(uint16_t));
#ifdef NEXUS_UE5
        m_buf.Append(hdr, 4);
#else
        m_buf.insert(m_buf.end(), hdr, hdr + 4);
#endif
    }

    NexusPacketBuilder& WriteU8 (uint8_t  v) { return WriteRaw(v); }
    NexusPacketBuilder& WriteU16(uint16_t v) { return WriteRaw(v); }
    NexusPacketBuilder& WriteU32(uint32_t v) { return WriteRaw(v); }
    NexusPacketBuilder& WriteU64(uint64_t v) { return WriteRaw(v); }
    NexusPacketBuilder& WriteFloat(float  v) { return WriteRaw(v); }

    // [uint16 길이][UTF-8 바이트...] 형식으로 기록
    NexusPacketBuilder& WriteString(const NexusStringType& str)
    {
#ifdef NEXUS_UE5
        FTCHARToUTF8 conv(*str);
        const uint16_t len = static_cast<uint16_t>(conv.Length());
        WriteRaw(len);
        m_buf.Append(reinterpret_cast<const NexusByte*>(conv.Get()),
                     static_cast<int32>(len));
#else
        const uint16_t len = static_cast<uint16_t>(str.size());
        WriteRaw(len);
        const auto* p = reinterpret_cast<const NexusByte*>(str.data());
        m_buf.insert(m_buf.end(), p, p + len);
#endif
        return *this;
    }

    // size 필드를 확정하고 완성된 버퍼를 반환. Send 직전 한 번만 호출.
    [[nodiscard]] NexusByteArray Finalize()
    {
#ifdef NEXUS_UE5
        const uint16_t sz = static_cast<uint16_t>(m_buf.Num());
        NEXUS_MEMCPY(m_buf.GetData(), &sz, sizeof(uint16_t));
#else
        const uint16_t sz = static_cast<uint16_t>(m_buf.size());
        NEXUS_MEMCPY(m_buf.data(),    &sz, sizeof(uint16_t));
#endif
        return m_buf;
    }

private:
    NexusByteArray m_buf;

    template<typename T>
    NexusPacketBuilder& WriteRaw(T v)
    {
        const NexusByte* p = reinterpret_cast<const NexusByte*>(&v);
#ifdef NEXUS_UE5
        m_buf.Append(p, static_cast<int32>(sizeof(T)));
#else
        m_buf.insert(m_buf.end(), p, p + sizeof(T));
#endif
        return *this;
    }
};
