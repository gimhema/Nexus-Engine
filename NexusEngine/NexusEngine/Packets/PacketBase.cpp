#include "PacketBase.h"

#include <cstring>
#include <stdexcept>

// ─────────────────────────────────────────────────────────────────────────────
// PacketReader
// ─────────────────────────────────────────────────────────────────────────────

PacketReader::PacketReader(const uint8_t* data, uint32_t length)
    : m_data(data)
    , m_length(length)
    , m_pos(PACKET_HEADER_SIZE)   // 헤더 skip, payload 부터 읽기 시작
{
    if (length < PACKET_HEADER_SIZE)
        throw std::runtime_error("PacketReader: 패킷이 너무 짧습니다");

    const auto* hdr = reinterpret_cast<const PacketHeader*>(data);
    m_opcode = hdr->opcode;
}

PacketReader PacketReader::FromPayload(uint16_t opcode, const uint8_t* data, uint32_t length)
{
    PacketReader reader;
    reader.m_data   = data;
    reader.m_length = length;
    reader.m_pos    = 0;   // 헤더 없음 — 바로 payload 시작
    reader.m_opcode = opcode;
    return reader;
}

PacketReader PacketReader::FromUDP(const uint8_t* data, uint32_t length)
{
    PacketReader reader;
    reader.m_data   = data;
    reader.m_length = length;
    reader.m_pos    = UDP_HEADER_SIZE;   // 헤더 + sessionToken skip

    if (length < UDP_HEADER_SIZE)
        throw std::runtime_error("PacketReader::FromUDP: 패킷이 너무 짧습니다");

    const auto* hdr = reinterpret_cast<const PacketHeader*>(data);
    reader.m_opcode = hdr->opcode;

    uint64_t token = 0;
    std::memcpy(&token, data + PACKET_HEADER_SIZE, sizeof(token));
    reader.m_sessionToken = token;

    return reader;
}

template<typename T>
T PacketReader::ReadRaw()
{
    if (m_pos + sizeof(T) > m_length)
        throw std::runtime_error("PacketReader: 버퍼 오버런");
    T val;
    std::memcpy(&val, m_data + m_pos, sizeof(T));
    m_pos += static_cast<uint32_t>(sizeof(T));
    return val;
}

uint8_t  PacketReader::ReadUInt8()  { return ReadRaw<uint8_t>(); }
uint16_t PacketReader::ReadUInt16() { return ReadRaw<uint16_t>(); }
uint32_t PacketReader::ReadUInt32() { return ReadRaw<uint32_t>(); }
uint64_t PacketReader::ReadUInt64() { return ReadRaw<uint64_t>(); }
float    PacketReader::ReadFloat()  { return ReadRaw<float>(); }

std::string PacketReader::ReadString()
{
    uint16_t len = ReadUInt16();
    if (m_pos + len > m_length)
        throw std::runtime_error("PacketReader::ReadString: 버퍼 오버런");
    std::string s(reinterpret_cast<const char*>(m_data + m_pos), len);
    m_pos += len;
    return s;
}

// ─────────────────────────────────────────────────────────────────────────────
// PacketWriter
// ─────────────────────────────────────────────────────────────────────────────

PacketWriter::PacketWriter(uint16_t opcode)
{
    // 헤더 자리 예약 (size 는 Finalize() 에서 채움)
    m_buf.resize(PACKET_HEADER_SIZE);
    auto* hdr    = reinterpret_cast<PacketHeader*>(m_buf.data());
    hdr->size    = 0;
    hdr->opcode  = opcode;
}

PacketWriter PacketWriter::ForUDP(uint16_t opcode, uint64_t sessionToken)
{
    PacketWriter pw;
    pw.m_buf.resize(UDP_HEADER_SIZE);
    auto* hdr   = reinterpret_cast<PacketHeader*>(pw.m_buf.data());
    hdr->size   = 0;
    hdr->opcode = opcode;
    std::memcpy(pw.m_buf.data() + PACKET_HEADER_SIZE, &sessionToken, sizeof(sessionToken));
    return pw;
}

template<typename T>
PacketWriter& PacketWriter::WriteRaw(T v)
{
    const auto* p = reinterpret_cast<const uint8_t*>(&v);
    m_buf.insert(m_buf.end(), p, p + sizeof(T));
    return *this;
}

PacketWriter& PacketWriter::WriteUInt8 (uint8_t  v) { return WriteRaw(v); }
PacketWriter& PacketWriter::WriteUInt16(uint16_t v) { return WriteRaw(v); }
PacketWriter& PacketWriter::WriteUInt32(uint32_t v) { return WriteRaw(v); }
PacketWriter& PacketWriter::WriteUInt64(uint64_t v) { return WriteRaw(v); }
PacketWriter& PacketWriter::WriteFloat (float    v) { return WriteRaw(v); }

PacketWriter& PacketWriter::WriteString(std::string_view str)
{
    auto len = static_cast<uint16_t>(str.size());
    WriteUInt16(len);
    m_buf.insert(m_buf.end(),
                 reinterpret_cast<const uint8_t*>(str.data()),
                 reinterpret_cast<const uint8_t*>(str.data()) + len);
    return *this;
}

const std::vector<uint8_t>& PacketWriter::Finalize()
{
    if (!m_finalized)
    {
        reinterpret_cast<PacketHeader*>(m_buf.data())->size =
            static_cast<uint16_t>(m_buf.size());
        m_finalized = true;
    }
    return m_buf;
}
