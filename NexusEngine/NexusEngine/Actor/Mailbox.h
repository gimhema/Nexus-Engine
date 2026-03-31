#pragma once

#include <atomic>
#include <memory>
#include <optional>
#include <cstddef>

// ─────────────────────────────────────────────────────────────────────────────
// MpscMailbox<T>
//
// Multiple-Producer Single-Consumer lock-free 큐.
// Michael-Scott 알고리즘 기반 (더미 헤드 노드 사용).
//
// 보장:
//   - Post()  : 복수 스레드에서 동시 호출 가능 (lock-free)
//   - TryPop(): 반드시 단일 소비자 스레드에서만 호출
//   - DrainAll(): 소비자 스레드에서 배치 처리 시 사용
// ─────────────────────────────────────────────────────────────────────────────
template<typename T>
class MpscMailbox
{
public:
    MpscMailbox()
    {
        // 더미 헤드 노드 — 항상 존재, 실제 데이터 없음
        Node* dummy = new Node{};
        m_head.store(dummy, std::memory_order_relaxed);
        m_tail = dummy;
    }

    ~MpscMailbox()
    {
        // 남은 노드 정리
        while (Node* node = m_head.load(std::memory_order_relaxed))
        {
            Node* next = node->next.load(std::memory_order_relaxed);
            delete node;
            m_head.store(next, std::memory_order_relaxed);
        }
    }

    MpscMailbox(const MpscMailbox&)            = delete;
    MpscMailbox& operator=(const MpscMailbox&) = delete;

    // ── 생산자 (다중 스레드 안전) ──────────────────────────────────────────
    void Post(T msg)
    {
        Node* node = new Node{ std::move(msg) };
        // tail을 새 노드로 교체하고, 이전 tail의 next를 연결
        Node* prev = m_tail.exchange(node, std::memory_order_acq_rel);
        prev->next.store(node, std::memory_order_release);
    }

    // ── 소비자 (단일 스레드 전용) ─────────────────────────────────────────
    [[nodiscard]] std::optional<T> TryPop()
    {
        Node* head = m_head.load(std::memory_order_relaxed);
        Node* next = head->next.load(std::memory_order_acquire);
        if (!next)
            return std::nullopt;

        // next가 실제 데이터 노드 — head(더미)를 next로 교체
        T val = std::move(next->val);
        delete head;
        m_head.store(next, std::memory_order_relaxed);
        return val;
    }

    // 메일박스를 비울 때까지 콜백 호출.
    // ZoneActor tick 루프에서 메시지 배치 처리 시 사용.
    template<typename Fn>
    void DrainAll(Fn&& fn)
    {
        while (auto msg = TryPop())
            fn(std::move(*msg));
    }

    // 대기 중인 메시지가 있는지 빠르게 확인 (소비자 스레드)
    [[nodiscard]] bool HasMessages() const
    {
        Node* head = m_head.load(std::memory_order_relaxed);
        return head->next.load(std::memory_order_acquire) != nullptr;
    }

private:
    struct Node
    {
        T                    val{};
        std::atomic<Node*>   next{ nullptr };
    };

    // m_head: 소비자만 갱신 (relaxed 읽기)
    // m_tail: 생산자가 exchange (acq_rel)
    alignas(64) std::atomic<Node*> m_head;
    alignas(64) std::atomic<Node*> m_tail;
};
