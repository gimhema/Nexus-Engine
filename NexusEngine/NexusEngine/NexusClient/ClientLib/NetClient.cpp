#include "NetClient.h"
#include <cstring>

#ifdef _WIN32
    #pragma comment(lib, "ws2_32.lib")
#endif

NetClient::NetClient()
{
#ifdef _WIN32
    WSADATA wsa{};
    ::WSAStartup(MAKEWORD(2, 2), &wsa);
#endif
    m_recvBuf.reserve(8192);
}

NetClient::~NetClient()
{
    Disconnect();
#ifdef _WIN32
    ::WSACleanup();
#endif
}

bool NetClient::Connect(const std::string& host, uint16_t port)
{
    m_socket = ::socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
    if (m_socket == NX_INVALID_SOCKET) return false;

    sockaddr_in addr{};
    addr.sin_family = AF_INET;
    addr.sin_port   = htons(port);
    if (::inet_pton(AF_INET, host.c_str(), &addr.sin_addr) <= 0)
    {
        closesocket_nx(m_socket);
        m_socket = NX_INVALID_SOCKET;
        return false;
    }

    if (::connect(m_socket,
                  reinterpret_cast<sockaddr*>(&addr),
                  static_cast<int>(sizeof(addr))) != 0)
    {
        closesocket_nx(m_socket);
        m_socket = NX_INVALID_SOCKET;
        return false;
    }

    m_connected.store(true);
    m_recvThread = std::thread([this] { RecvLoop(); });

    if (m_onConnected)
        m_onConnected();

    return true;
}

void NetClient::Disconnect()
{
    if (!m_connected.exchange(false)) return;

    closesocket_nx(m_socket);
    m_socket = NX_INVALID_SOCKET;

    if (m_recvThread.joinable())
        m_recvThread.join();
}

void NetClient::Send(const std::vector<uint8_t>& packet)
{
    std::lock_guard lock(m_sendMutex);
    if (!m_connected.load()) return;

    const uint8_t* data      = packet.data();
    size_t         remaining = packet.size();

    while (remaining > 0)
    {
#ifdef _WIN32
        int sent = ::send(m_socket,
                          reinterpret_cast<const char*>(data),
                          static_cast<int>(remaining), 0);
#else
        ssize_t sent = ::send(m_socket, data, remaining, MSG_NOSIGNAL);
#endif
        if (sent <= 0) break;
        data      += sent;
        remaining -= static_cast<size_t>(sent);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// RecvLoop — 수신 스레드 전용
// TCP 스트림을 패킷 단위로 재조립해 콜백 호출
// ─────────────────────────────────────────────────────────────────────────────
void NetClient::RecvLoop()
{
    constexpr uint32_t TMP_SIZE = 4096;
    uint8_t tmp[TMP_SIZE];

    while (m_connected.load())
    {
#ifdef _WIN32
        int n = ::recv(m_socket,
                       reinterpret_cast<char*>(tmp), TMP_SIZE, 0);
#else
        ssize_t n = ::recv(m_socket, tmp, TMP_SIZE, 0);
#endif
        if (n <= 0)
        {
            // 연결 종료 또는 오류
            OnDisconnectInternal();
            return;
        }

        m_recvBuf.insert(m_recvBuf.end(), tmp, tmp + n);

        // 완성된 패킷 추출
        while (m_recvBuf.size() >= CLIENT_PACKET_HEADER_SIZE)
        {
            auto* hdr = reinterpret_cast<const ClientPacketHeader*>(
                            m_recvBuf.data());

            if (hdr->size < CLIENT_PACKET_HEADER_SIZE) { OnDisconnectInternal(); return; }
            if (m_recvBuf.size() < hdr->size) break;  // 아직 완전하지 않음

            if (m_onPacket)
            {
                ClientPacketReader reader(m_recvBuf.data(), hdr->size);
                m_onPacket(reader.GetOpcode(), reader.GetPayload());
            }

            m_recvBuf.erase(m_recvBuf.begin(),
                            m_recvBuf.begin() + hdr->size);
        }
    }
}

void NetClient::OnDisconnectInternal()
{
    if (!m_connected.exchange(false)) return;

    closesocket_nx(m_socket);
    m_socket = NX_INVALID_SOCKET;

    if (m_onDisconnected)
        m_onDisconnected();
}
