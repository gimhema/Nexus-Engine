#include "PacketBase.h"

#include <cstring>
#include <stdexcept>

// ─────────────────────────────────────────────────────────────────────────────
// ClientPacketWriter
// ─────────────────────────────────────────────────────────────────────────────

ClientPacketWriter::ClientPacketWriter(uint16_t opcode)
{
    // 헤더 플레이스홀더 (size는 Finalize에서 채움)
    m_buf.resize(CLIENT_PACKET_HEADER_SIZE);
    auto* hdr = reinterpret_cast<ClientPacketHeader*>(m_buf.data());
    hdr->size   = 0;
    hdr->opcode = opcode;
}

template<typename T>
ClientPacketWriter& ClientPacketWriter::WriteRaw(T v)
{
    const auto* p = reinterpret_cast<const uint8_t*>(&v);
    m_buf.insert(m_buf.end(), p, p + sizeof(T));
    return *this;
}

ClientPacketWriter& ClientPacketWriter::WriteUInt8 (uint8_t  v) { return WriteRaw(v); }
ClientPacketWriter& ClientPacketWriter::WriteUInt16(uint16_t v) { return WriteRaw(v); }
ClientPacketWriter& ClientPacketWriter::WriteUInt32(uint32_t v) { return WriteRaw(v); }
ClientPacketWriter& ClientPacketWriter::WriteUInt64(uint64_t v) { return WriteRaw(v); }
ClientPacketWriter& ClientPacketWriter::WriteFloat (float    v) { return WriteRaw(v); }

ClientPacketWriter& ClientPacketWriter::WriteString(const std::string& str)
{
    auto len = static_cast<uint16_t>(str.size());
    WriteRaw(len);
    m_buf.insert(m_buf.end(), str.begin(), str.end());
    return *this;
}

const std::vector<uint8_t>& ClientPacketWriter::Finalize()
{
    if (!m_finalized)
    {
        auto* hdr  = reinterpret_cast<ClientPacketHeader*>(m_buf.data());
        hdr->size  = static_cast<uint16_t>(m_buf.size());
        m_finalized = true;
    }
    return m_buf;
}

// ─────────────────────────────────────────────────────────────────────────────
// ClientPacketReader
// ─────────────────────────────────────────────────────────────────────────────

ClientPacketReader::ClientPacketReader(const uint8_t* data, uint32_t length)
    : m_data(data)
    , m_length(length)
    , m_pos(CLIENT_PACKET_HEADER_SIZE)
{
    auto* hdr  = reinterpret_cast<const ClientPacketHeader*>(data);
    m_opcode   = hdr->opcode;
}

template<typename T>
T ClientPacketReader::ReadRaw()
{
    if (m_pos + sizeof(T) > m_length)
        throw std::runtime_error("ClientPacketReader: 버퍼 초과");
    T v{};
    std::memcpy(&v, m_data + m_pos, sizeof(T));
    m_pos += sizeof(T);
    return v;
}

uint8_t     ClientPacketReader::ReadUInt8()  { return ReadRaw<uint8_t>();  }
uint16_t    ClientPacketReader::ReadUInt16() { return ReadRaw<uint16_t>(); }
uint32_t    ClientPacketReader::ReadUInt32() { return ReadRaw<uint32_t>(); }
uint64_t    ClientPacketReader::ReadUInt64() { return ReadRaw<uint64_t>(); }
float       ClientPacketReader::ReadFloat()  { return ReadRaw<float>();    }

std::string ClientPacketReader::ReadString()
{
    uint16_t len = ReadRaw<uint16_t>();
    if (m_pos + len > m_length)
        throw std::runtime_error("ClientPacketReader: 문자열 버퍼 초과");
    std::string s(reinterpret_cast<const char*>(m_data + m_pos), len);
    m_pos += len;
    return s;
}
