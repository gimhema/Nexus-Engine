#pragma once
#include <cstdint>
#include <string>
#include <string_view>
#include <vector>
#include <stdexcept>
#include <cstring>

// ─────────────────────────────────────────────────────────────────────────────
// Packet layout (Little Endian)
//
//  TCP  : [ uint16 size ][ uint16 opcode ][ payload... ]
//  UDP  : [ uint16 size ][ uint16 opcode ][ uint64 sessionToken ][ payload... ]
//
//  size  = 헤더 포함 전체 길이 (header + payload)
//  opcode = 패킷 종류 식별자
// ─────────────────────────────────────────────────────────────────────────────

inline constexpr uint32_t PACKET_HEADER_SIZE     = 4;      // uint16 size + uint16 opcode
inline constexpr uint32_t UDP_SESSION_TOKEN_SIZE = 8;      // uint64 sessionToken
inline constexpr uint32_t UDP_HEADER_SIZE        = PACKET_HEADER_SIZE + UDP_SESSION_TOKEN_SIZE;
inline constexpr uint16_t PACKET_MAX_SIZE        = 32768;

#pragma pack(push, 1)
struct PacketHeader
{
    uint16_t size;    // 헤더 포함 전체 바이트 수
    uint16_t opcode;
};
#pragma pack(pop)

// ─────────────────────────────────────────────────────────────────────────────
// PacketReader
//
// 원시 바이트 버퍼에서 필드를 순차 읽기.
// 버퍼 소유권 없음 — 호출자가 생존 기간 보장.
// ─────────────────────────────────────────────────────────────────────────────
class PacketReader
{
public:
    // TCP용: data = 헤더 시작 포인터, length = 전체 패킷 길이
    PacketReader(const uint8_t* data, uint32_t length);

    // UDP용: sessionToken을 추가로 파싱
    static PacketReader FromUDP(const uint8_t* data, uint32_t length);

    uint16_t    GetOpcode()      const { return m_opcode; }
    uint64_t    GetSessionToken()const { return m_sessionToken; }
    uint32_t    Remaining()      const { return m_length - m_pos; }
    bool        IsEOF()          const { return m_pos >= m_length; }

    // 현재 읽기 위치 이후 남은 바이트를 복사 반환 (MsgNet_PacketReceived payload 용)
    std::vector<uint8_t> GetPayload() const
    {
        return { m_data + m_pos, m_data + m_length };
    }

    uint8_t     ReadUInt8();
    uint16_t    ReadUInt16();
    uint32_t    ReadUInt32();
    uint64_t    ReadUInt64();
    float       ReadFloat();
    // uint16 길이 프리픽스 + UTF-8 바이트열
    std::string ReadString();

private:
    PacketReader() = default;

    const uint8_t* m_data{ nullptr };
    uint32_t       m_length{ 0 };
    uint32_t       m_pos{ 0 };
    uint16_t       m_opcode{ 0 };
    uint64_t       m_sessionToken{ 0 };  // UDP 전용

    template<typename T>
    T ReadRaw();
};

// ─────────────────────────────────────────────────────────────────────────────
// PacketWriter
//
// 내부 버퍼에 패킷을 조립. Finalize() 호출 시 size 필드 확정.
// ─────────────────────────────────────────────────────────────────────────────
class PacketWriter
{
public:
    explicit PacketWriter(uint16_t opcode);

    // UDP용: sessionToken을 헤더 직후에 삽입
    static PacketWriter ForUDP(uint16_t opcode, uint64_t sessionToken);

    PacketWriter& WriteUInt8 (uint8_t  v);
    PacketWriter& WriteUInt16(uint16_t v);
    PacketWriter& WriteUInt32(uint32_t v);
    PacketWriter& WriteUInt64(uint64_t v);
    PacketWriter& WriteFloat (float    v);
    // uint16 길이 프리픽스 + UTF-8 바이트열
    PacketWriter& WriteString(std::string_view str);

    // size 필드를 채우고 완성된 버퍼 반환. Send 직전 한 번만 호출.
    [[nodiscard]] const std::vector<uint8_t>& Finalize();

    uint16_t Size() const { return static_cast<uint16_t>(m_buf.size()); }

private:
    PacketWriter() = default;

    std::vector<uint8_t> m_buf;
    bool                 m_finalized{ false };

    template<typename T>
    PacketWriter& WriteRaw(T v);
};
