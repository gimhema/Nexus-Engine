#include "ArbiterSession.h"
#include "../Shared/Logger.h"

ArbiterSession::ArbiterSession(NxSocket socket)
    : m_socket(socket)
{}

ArbiterSession::~ArbiterSession()
{
    Close();
    if (m_recvThread.joinable())
    {
        // RecvLoop → onClose 콜백 체인으로 소멸자가 호출되면 recv 스레드 내부에서 실행 중.
        // 자기 자신을 join하면 EDEADLK → detach로 우회 (RecvLoop는 곧바로 반환하므로 안전)
        if (m_recvThread.get_id() == std::this_thread::get_id())
            m_recvThread.detach();
        else
            m_recvThread.join();
    }
}

void ArbiterSession::Start(OnPacketFunc onPacket, OnCloseFunc onClose)
{
    m_recvThread = std::thread(
        [this,
         onPacket = std::move(onPacket),
         onClose  = std::move(onClose)]() mutable
        {
            RecvLoop(std::move(onPacket), std::move(onClose));
        });
}

void ArbiterSession::Send(const std::vector<uint8_t>& packet)
{
    if (m_closed.load()) return;

    std::lock_guard lock(m_sendMutex);

    const char* data  = reinterpret_cast<const char*>(packet.data());
    int         total = static_cast<int>(packet.size());
    int         sent  = 0;

    while (sent < total)
    {
        int n = ::send(m_socket, data + sent, total - sent, 0);
        if (n <= 0) { Close(); return; }
        sent += n;
    }
}

void ArbiterSession::Close()
{
    // exchange로 중복 호출 방지
    if (m_closed.exchange(true)) return;

    if (m_socket != NX_INVALID_SOCKET)
    {
#ifdef _WIN32
        ::shutdown(m_socket, SD_BOTH);
#else
        ::shutdown(m_socket, SHUT_RDWR);
#endif
        closesocket(m_socket);
        m_socket = NX_INVALID_SOCKET;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 수신 루프 — 전용 스레드에서 실행
// ─────────────────────────────────────────────────────────────────────────────
void ArbiterSession::RecvLoop(OnPacketFunc onPacket, OnCloseFunc onClose)
{
    static constexpr int TEMP_BUF = 4096;
    uint8_t tmp[TEMP_BUF];

    while (!m_closed.load())
    {
        int n = ::recv(m_socket, reinterpret_cast<char*>(tmp), TEMP_BUF, 0);
        if (n <= 0) break;  // 연결 종료 또는 오류

        // 수신 데이터를 누적 버퍼에 추가
        m_recvBuf.insert(m_recvBuf.end(), tmp, tmp + n);

        // 완성된 패킷을 하나씩 처리
        while (m_recvBuf.size() >= PACKET_HEADER_SIZE)
        {
            const auto* hdr = reinterpret_cast<const PacketHeader*>(m_recvBuf.data());

            // size 유효성 검사
            if (hdr->size < PACKET_HEADER_SIZE || hdr->size > PACKET_MAX_SIZE)
            {
                LOG_WARN("ArbiterSession: 잘못된 패킷 크기 size={}", hdr->size);
                Close();
                goto exit_loop;
            }

            // 패킷이 아직 완성되지 않음 — 추가 수신 대기
            if (static_cast<uint32_t>(m_recvBuf.size()) < hdr->size) break;

            // 페이로드 추출 (헤더 제외)
            std::vector<uint8_t> payload(
                m_recvBuf.begin() + PACKET_HEADER_SIZE,
                m_recvBuf.begin() + hdr->size);

            const uint16_t opcode = hdr->opcode;
            m_recvBuf.erase(m_recvBuf.begin(), m_recvBuf.begin() + hdr->size);

            onPacket(*this, opcode, std::move(payload));
        }
    }

exit_loop:
    Close();
    onClose(*this);
}
