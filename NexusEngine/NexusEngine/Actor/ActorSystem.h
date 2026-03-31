#pragma once

#include "Actor.h"

#include <vector>
#include <thread>
#include <mutex>
#include <condition_variable>
#include <queue>
#include <functional>
#include <atomic>
#include <memory>

// ─────────────────────────────────────────────────────────────────────────────
// ActorSystem
//
// Pooled 모드 Actor를 스케줄링하는 스레드 풀.
// SessionActor처럼 수천 개가 존재하는 Actor에 사용.
//
// Dedicated 모드 Actor (ZoneActor, WorldActor)는
// 직접 Start()/StartWithTick()을 호출해 전용 스레드를 가진다.
// ActorSystem은 이들을 관리하지 않는다.
//
// 사용법:
//   ActorSystem::Instance().Start(threadCount);
//   ActorSystem::Instance().Schedule([actor]{ actor->ProcessMailbox(); });
//   ActorSystem::Instance().Stop();
// ─────────────────────────────────────────────────────────────────────────────
class ActorSystem
{
public:
    static ActorSystem& Instance()
    {
        static ActorSystem s_instance;
        return s_instance;
    }

    // threadCount = 0 이면 CPU 코어 수 사용
    void Start(uint32_t threadCount = 0)
    {
        if (m_running.exchange(true)) return;

        if (threadCount == 0)
            threadCount = std::max(1u, std::thread::hardware_concurrency());

        m_workers.reserve(threadCount);
        for (uint32_t i = 0; i < threadCount; ++i)
            m_workers.emplace_back([this] { WorkerLoop(); });
    }

    void Stop()
    {
        if (!m_running.exchange(false)) return;
        m_cv.notify_all();
        for (auto& t : m_workers)
            if (t.joinable()) t.join();
        m_workers.clear();
    }

    // 임의의 태스크를 풀에 제출. Actor::Post() 이후 이 함수로 ProcessMailbox를 예약.
    void Schedule(std::function<void()> task)
    {
        {
            std::lock_guard lock(m_mutex);
            m_queue.push(std::move(task));
        }
        m_cv.notify_one();
    }

    [[nodiscard]] bool IsRunning() const { return m_running.load(); }

private:
    ActorSystem()  = default;
    ~ActorSystem() { Stop(); }

    void WorkerLoop()
    {
        while (m_running.load())
        {
            std::function<void()> task;
            {
                std::unique_lock lock(m_mutex);
                m_cv.wait(lock, [this]
                {
                    return !m_running.load() || !m_queue.empty();
                });
                if (!m_running.load() && m_queue.empty()) break;
                task = std::move(m_queue.front());
                m_queue.pop();
            }
            task();
        }
    }

    std::atomic<bool>                    m_running{ false };
    std::vector<std::thread>             m_workers;
    std::mutex                           m_mutex;
    std::condition_variable              m_cv;
    std::queue<std::function<void()>>    m_queue;
};
