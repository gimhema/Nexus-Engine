#pragma once

// ─────────────────────────────────────────────────────────────────────────────
// PacketBase.h — 클라이언트 패킷 직렬화 / 역직렬화
//
// 서버와 동일한 바이너리 포맷 (Little Endian):
//   TCP : [ uint16 size ][ uint16 opcode ][ payload... ]
//
// size = 헤더 포함 전체 길이
// ─────────────────────────────────────────────────────────────────────────────

#include <cstdint>
#include <string>
#include <vector>
#include <cstring>

inline constexpr uint32_t CLIENT_PACKET_HEADER_SIZE = 4;   // uint16 size + uint16 opcode
inline constexpr uint16_t CLIENT_PACKET_MAX_SIZE    = 32768;

#pragma pack(push, 1)
struct ClientPacketHeader
{
    uint16_t size;
    uint16_t opcode;
};
#pragma pack(pop)

// ─────────────────────────────────────────────────────────────────────────────
// PacketWriter — 패킷 직렬화
// ─────────────────────────────────────────────────────────────────────────────
class ClientPacketWriter
{
public:
    explicit ClientPacketWriter(uint16_t opcode);

    ClientPacketWriter& WriteUInt8 (uint8_t  v);
    ClientPacketWriter& WriteUInt16(uint16_t v);
    ClientPacketWriter& WriteUInt32(uint32_t v);
    ClientPacketWriter& WriteUInt64(uint64_t v);
    ClientPacketWriter& WriteFloat (float    v);
    ClientPacketWriter& WriteString(const std::string& str);

    // size 필드 확정 후 완성된 버퍼 반환. Send 직전 한 번만 호출.
    [[nodiscard]] const std::vector<uint8_t>& Finalize();

private:
    std::vector<uint8_t> m_buf;
    bool                 m_finalized{ false };

    template<typename T>
    ClientPacketWriter& WriteRaw(T v);
};

// ─────────────────────────────────────────────────────────────────────────────
// PacketReader — 패킷 역직렬화
// ─────────────────────────────────────────────────────────────────────────────
class ClientPacketReader
{
public:
    // data = 헤더 시작 포인터, length = 전체 패킷 길이
    ClientPacketReader(const uint8_t* data, uint32_t length);

    uint16_t GetOpcode()   const { return m_opcode; }
    uint32_t Remaining()   const { return m_length - m_pos; }
    bool     IsEOF()       const { return m_pos >= m_length; }

    uint8_t     ReadUInt8();
    uint16_t    ReadUInt16();
    uint32_t    ReadUInt32();
    uint64_t    ReadUInt64();
    float       ReadFloat();
    std::string ReadString();

    std::vector<uint8_t> GetPayload() const
    {
        return { m_data + m_pos, m_data + m_length };
    }

private:
    const uint8_t* m_data{ nullptr };
    uint32_t       m_length{ 0 };
    uint32_t       m_pos{ 0 };
    uint16_t       m_opcode{ 0 };

    template<typename T>
    T ReadRaw();
};
