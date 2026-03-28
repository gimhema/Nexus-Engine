#include "Connection.h"
#include "ServerNet.h"   // OverlappedEx 정의, IOOperation, NET_RECV_BUFFER_SIZE

#include <cstring>
#include <algorithm>

// ─────────────────────────────────────────────────────────────────────────────
// RecvBuffer
// ─────────────────────────────────────────────────────────────────────────────

RecvBuffer::RecvBuffer(uint32_t capacity) : m_buf(capacity) {}

uint8_t* RecvBuffer::WritePtr()            { return m_buf.data() + m_writePos; }
uint32_t RecvBuffer::FreeSpace() const     { return static_cast<uint32_t>(m_buf.size()) - m_writePos; }
void     RecvBuffer::OnWritten(uint32_t b) { m_writePos += b; }

const uint8_t* RecvBuffer::ReadPtr() const { return m_buf.data() + m_readPos; }
uint32_t       RecvBuffer::Readable() const { return m_writePos - m_readPos; }
void           RecvBuffer::Consume(uint32_t b) { m_readPos += b; }

void RecvBuffer::Compact()
{
    if (m_readPos == 0) return;
    if (m_readPos < static_cast<uint32_t>(m_buf.size()) / 2) return;

    uint32_t readable = Readable();
    if (readable > 0)
        std::memmove(m_buf.data(), m_buf.data() + m_readPos, readable);
    m_writePos = readable;
    m_readPos  = 0;
}

// ─────────────────────────────────────────────────────────────────────────────
// Session — 공통 구현
// ─────────────────────────────────────────────────────────────────────────────

Session::Session(NxSocket socket, uint64_t sessionId, NxHandle ioHandle)
    : m_socket(socket)
    , m_sessionId(sessionId)
    , m_ioHandle(ioHandle)
    , m_recvBuffer(NET_RECV_BUFFER_SIZE)
    , m_recvOv(std::make_unique<OverlappedEx>())
    , m_sendOv(std::make_unique<OverlappedEx>())
{
}

Session::~Session()
{
    if (m_socket != NX_INVALID_SOCKET)
    {
        closesocket(m_socket);
        m_socket = NX_INVALID_SOCKET;
    }
}

void Session::Send(const uint8_t* data, uint16_t length)
{
    if (!IsConnected()) return;

    std::vector<uint8_t> packet(data, data + length);
    {
        std::lock_guard lock(m_sendMutex);
        m_sendQueue.push(std::move(packet));
    }

    // exchange(true) 가 false 를 반환 → 내가 최초로 전송을 개시
    if (!m_sendPending.exchange(true))
        FlushSendQueue();
}

// ─────────────────────────────────────────────────────────────────────────────
// Session — 플랫폼별 구현
// ─────────────────────────────────────────────────────────────────────────────

#ifdef _WIN32

void Session::PostRecv()
{
    m_recvBuffer.Compact();

    m_recvOv->operation  = IOOperation::TCP_RECV;
    m_recvOv->wsaBuf.buf = reinterpret_cast<CHAR*>(m_recvBuffer.WritePtr());
    m_recvOv->wsaBuf.len = m_recvBuffer.FreeSpace();

    DWORD flags = 0, bytes = 0;
    int result = ::WSARecv(m_socket, &m_recvOv->wsaBuf, 1,
                           &bytes, &flags, &m_recvOv->overlapped, nullptr);
    if (result == SOCKET_ERROR && ::WSAGetLastError() != WSA_IO_PENDING)
        SetState(SessionState::Closing);
}

void Session::FlushSendQueue()
{
    {
        std::lock_guard lock(m_sendMutex);
        if (m_sendQueue.empty())
        {
            m_sendPending.store(false);
            return;
        }
        m_sendBuffer = std::move(m_sendQueue.front());
        m_sendQueue.pop();
    }

    m_sendOv->operation  = IOOperation::TCP_SEND;
    m_sendOv->wsaBuf.buf = reinterpret_cast<CHAR*>(m_sendBuffer.data());
    m_sendOv->wsaBuf.len = static_cast<ULONG>(m_sendBuffer.size());

    DWORD bytes = 0;
    int result = ::WSASend(m_socket, &m_sendOv->wsaBuf, 1,
                           &bytes, 0, &m_sendOv->overlapped, nullptr);
    if (result == SOCKET_ERROR && ::WSAGetLastError() != WSA_IO_PENDING)
        SetState(SessionState::Closing);
}

#else // Linux ─────────────────────────────────────────────────────────────────

void Session::PostRecv()
{
    // epoll 에 EPOLLIN 등록 (최초 1회)
    m_recvOv->operation = IOOperation::TCP_RECV;
    m_recvOv->sessionId = m_sessionId;

    epoll_event ev{};
    ev.events   = EPOLLIN;
    ev.data.ptr = m_recvOv.get();
    ::epoll_ctl(m_ioHandle, EPOLL_CTL_ADD, m_socket, &ev);
}

void Session::FlushSendQueue()
{
    while (true)
    {
        std::vector<uint8_t> packet;
        {
            std::lock_guard lock(m_sendMutex);
            if (m_sendQueue.empty())
            {
                m_sendPending.store(false);
                // EPOLLOUT 해제 — EPOLLIN 만 유지
                epoll_event ev{};
                ev.events   = EPOLLIN;
                ev.data.ptr = m_recvOv.get();
                ::epoll_ctl(m_ioHandle, EPOLL_CTL_MOD, m_socket, &ev);
                return;
            }
            packet = std::move(m_sendQueue.front());
            m_sendQueue.pop();
        }

        const uint8_t* ptr   = packet.data();
        size_t         remaining = packet.size();

        while (remaining > 0)
        {
            ssize_t sent = ::send(m_socket, ptr, remaining, MSG_NOSIGNAL);
            if (sent > 0)
            {
                ptr       += sent;
                remaining -= static_cast<size_t>(sent);
            }
            else if (sent < 0 && (errno == EAGAIN || errno == EWOULDBLOCK))
            {
                // 소켓 버퍼 가득 참 — 남은 데이터를 큐 앞에 다시 삽입 후 EPOLLOUT 대기
                std::vector<uint8_t> leftover(ptr, ptr + remaining);
                {
                    std::lock_guard lock(m_sendMutex);
                    // std::queue 는 push_front 가 없으므로 임시 큐로 재조립
                    std::queue<std::vector<uint8_t>> tmp;
                    tmp.push(std::move(leftover));
                    while (!m_sendQueue.empty())
                    {
                        tmp.push(std::move(m_sendQueue.front()));
                        m_sendQueue.pop();
                    }
                    m_sendQueue = std::move(tmp);
                }
                epoll_event ev{};
                ev.events   = EPOLLIN | EPOLLOUT;
                ev.data.ptr = m_recvOv.get();
                ::epoll_ctl(m_ioHandle, EPOLL_CTL_MOD, m_socket, &ev);
                return;
            }
            else
            {
                SetState(SessionState::Closing);
                return;
            }
        }
    }
}

#endif
