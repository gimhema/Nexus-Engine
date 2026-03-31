#pragma once

#include "Mailbox.h"

#include <variant>
#include <thread>
#include <atomic>
#include <condition_variable>
#include <mutex>
#include <functional>
#include <concepts>

// ─────────────────────────────────────────────────────────────────────────────
// Actor<MessageVariant>
//
// CRTP 불필요 — 상속 + 오버라이드 방식.
//
// 두 가지 실행 모드:
//   1. Dedicated  — Start()로 전용 스레드 생성. ZoneActor, WorldActor 등에 사용.
//   2. Pooled     — ActorSystem의 스레드 풀에서 스케줄링. SessionActor 등에 사용.
//      (Pooled 모드는 ActorSystem이 Schedule()을 호출해 처리)
//
// 파생 클래스 구현 예:
//   class MyActor : public Actor<std::variant<MsgA, MsgB>>
//   {
//   protected:
//       void OnMessage(Message& msg) override
//       {
//           std::visit(overloaded{
//               [this](MsgA& m) { /* ... */ },
//               [this](MsgB& m) { /* ... */ },
//           }, msg);
//       }
//   };
// ─────────────────────────────────────────────────────────────────────────────

// std::visit 헬퍼 — overloaded pattern
template<typename... Ts>
struct overloaded : Ts... { using Ts::operator()...; };

template<typename TVariant>
class Actor
{
public:
    using Message = TVariant;

    Actor()  = default;
    virtual ~Actor() { Stop(); }

    Actor(const Actor&)            = delete;
    Actor& operator=(const Actor&) = delete;

    // ── 메시지 송신 (다중 스레드 안전) ───────────────────────────────────────
    void Post(Message msg)
    {
        m_mailbox.Post(std::move(msg));
        m_cv.notify_one();  // Dedicated 모드 대기 스레드 깨우기
    }

    // ── Dedicated 모드 — 전용 스레드 시작 ────────────────────────────────────
    // idle 시 조건변수 대기. ZoneActor는 이 대신 StartWithTick()을 사용.
    void Start()
    {
        if (m_running.exchange(true)) return;
        m_thread = std::thread([this] { DedicatedLoop(); });
    }

    // Dedicated 모드 — tick 주기를 가진 루프.
    // tickIntervalMs마다 OnTick()을 호출하면서, 그 사이에 메시지도 처리.
    void StartWithTick(uint32_t tickIntervalMs)
    {
        m_tickInterval = std::chrono::milliseconds(tickIntervalMs);
        if (m_running.exchange(true)) return;
        m_thread = std::thread([this] { TickLoop(); });
    }

    void Stop()
    {
        if (!m_running.exchange(false)) return;
        m_cv.notify_all();
        if (m_thread.joinable())
            m_thread.join();
    }

    [[nodiscard]] bool IsRunning() const { return m_running.load(); }

    // ── Pooled 모드 — ActorSystem이 호출 ─────────────────────────────────────
    // 메일박스를 드레인한다. 한 번에 처리할 최대 메시지 수 제한 가능.
    void ProcessMailbox(uint32_t maxMessages = 256)
    {
        uint32_t count = 0;
        while (count++ < maxMessages)
        {
            auto msg = m_mailbox.TryPop();
            if (!msg) break;
            OnMessage(*msg);
        }
    }

    [[nodiscard]] bool HasPendingMessages() const
    {
        return m_mailbox.HasMessages();
    }

protected:
    // 파생 클래스에서 구현 — 메시지 처리
    virtual void OnMessage(Message& msg) = 0;

    // tick 루프 사용 시 오버라이드 (선택)
    virtual void OnTick() {}

    // Start/Stop 시 훅 (선택)
    virtual void OnStart() {}
    virtual void OnStop()  {}

private:
    void DedicatedLoop()
    {
        OnStart();
        while (m_running.load())
        {
            std::unique_lock lock(m_mutex);
            m_cv.wait(lock, [this]
            {
                return !m_running.load() || m_mailbox.HasMessages();
            });
            lock.unlock();

            m_mailbox.DrainAll([this](Message msg) { OnMessage(msg); });
        }
        // 종료 전 남은 메시지 처리
        m_mailbox.DrainAll([this](Message msg) { OnMessage(msg); });
        OnStop();
    }

    void TickLoop()
    {
        using Clock = std::chrono::steady_clock;
        OnStart();
        auto nextTick = Clock::now() + m_tickInterval;

        while (m_running.load())
        {
            // tick 이전까지 메시지 처리
            m_mailbox.DrainAll([this](Message msg) { OnMessage(msg); });

            const auto now = Clock::now();
            if (now >= nextTick)
            {
                OnTick();
                nextTick += m_tickInterval;
                // 누적 지연 방지: 여러 tick이 밀렸으면 현재 시점으로 재설정
                if (Clock::now() > nextTick)
                    nextTick = Clock::now() + m_tickInterval;
            }

            // 다음 tick까지 또는 메시지 도착까지 대기
            std::unique_lock lock(m_mutex);
            m_cv.wait_until(lock, nextTick, [this]
            {
                return !m_running.load() || m_mailbox.HasMessages();
            });
        }
        m_mailbox.DrainAll([this](Message msg) { OnMessage(msg); });
        OnStop();
    }

    MpscMailbox<Message>          m_mailbox;
    std::thread                   m_thread;
    std::atomic<bool>             m_running{ false };
    std::mutex                    m_mutex;
    std::condition_variable       m_cv;
    std::chrono::milliseconds     m_tickInterval{ 50 };
};
