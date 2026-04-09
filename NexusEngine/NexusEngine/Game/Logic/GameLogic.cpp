#include "GameLogic.h"
#include "../Actors/ZoneActor.h"
#include "../../Shared/Logger.h"

#include <algorithm>
#include <cmath>

GameLogic::GameLogic() = default;

void GameLogic::SetRealSecondsPerGameHour(float realSeconds)
{
    m_realSecondsPerGameHour = (realSeconds > 0.f) ? realSeconds : 1.f;
}

void GameLogic::SetStartHour(float hour)
{
    m_gameHour = std::fmod(hour, 24.0f);
    if (m_gameHour < 0.f) m_gameHour += 24.0f;
}

void GameLogic::SetDayNightHours(float dayStartHour, float nightStartHour)
{
    m_dayStartHour   = dayStartHour;
    m_nightStartHour = nightStartHour;
}

void GameLogic::RegisterEvent(WorldEventDef def)
{
    m_events.push_back(std::move(def));
}

void GameLogic::RegisterZone(uint32_t zoneId, ZoneActor* zone)
{
    m_zones[zoneId] = zone;
}

bool GameLogic::IsDay() const
{
    return m_gameHour >= m_dayStartHour && m_gameHour < m_nightStartHour;
}

bool GameLogic::IsNight() const
{
    return !IsDay();
}

// ─────────────────────────────────────────────────────────────────────────────
// Lifecycle
// ─────────────────────────────────────────────────────────────────────────────
void GameLogic::OnStart()
{
    m_lastTickTime = Clock::now();
    LOG_INFO("GameLogic 시작 — 게임 시각 {:.1f}시, 실시간 {:.0f}초 = 게임 1시간 (하루 = 실 {:.1f}분)",
             m_gameHour,
             m_realSecondsPerGameHour,
             m_realSecondsPerGameHour * 24.f / 60.f);
}

void GameLogic::OnStop()
{
    LOG_INFO("GameLogic 종료 — 마지막 게임 시각 {:.1f}시", m_gameHour);
}

// ─────────────────────────────────────────────────────────────────────────────
// Tick (1초마다 호출)
// ─────────────────────────────────────────────────────────────────────────────
void GameLogic::OnTick()
{
    const auto  now      = Clock::now();
    const float deltaSec = std::chrono::duration<float>(now - m_lastTickTime).count();
    m_lastTickTime       = now;

    AdvanceTime(deltaSec);
}

// ─────────────────────────────────────────────────────────────────────────────
// 시간 진행
// ─────────────────────────────────────────────────────────────────────────────
void GameLogic::AdvanceTime(float deltaSec)
{
    const float prevHour    = m_gameHour;
    const float gameHourAdv = deltaSec / m_realSecondsPerGameHour;

    m_gameHour += gameHourAdv;
    if (m_gameHour >= 24.0f)
        m_gameHour -= 24.0f;

    CheckAndFireEvents(prevHour, m_gameHour);
}

// ─────────────────────────────────────────────────────────────────────────────
// 이벤트 발동 검사
// prevHour → newHour 구간을 통과한 이벤트를 찾아 발동.
// 자정(24→0) 교차 처리 포함.
// ─────────────────────────────────────────────────────────────────────────────
void GameLogic::CheckAndFireEvents(float prevHour, float newHour)
{
    const bool wrappedDay = (newHour < prevHour);  // 자정을 넘음

    // 1회성 이벤트를 역순으로 제거하기 위해 인덱스 기록
    std::vector<size_t> toRemove;

    for (size_t i = 0; i < m_events.size(); ++i)
    {
        const auto& def = m_events[i];
        const float t   = def.triggerHour;

        bool shouldFire;
        if (wrappedDay)
            shouldFire = (t > prevHour) || (t <= newHour);
        else
            shouldFire = (t > prevHour) && (t <= newHour);

        if (!shouldFire) continue;

        BroadcastWorldEvent(def);
        if (def.callback) def.callback();
        if (!def.repeating) toRemove.push_back(i);
    }

    // 1회성 이벤트 역순 제거 (인덱스 유지)
    for (auto it = toRemove.rbegin(); it != toRemove.rend(); ++it)
        m_events.erase(m_events.begin() + static_cast<std::ptrdiff_t>(*it));
}

// ─────────────────────────────────────────────────────────────────────────────
// 월드 이벤트를 등록된 모든 ZoneActor에 브로드캐스트
// ─────────────────────────────────────────────────────────────────────────────
void GameLogic::BroadcastWorldEvent(const WorldEventDef& def)
{
    LOG_INFO("WorldEvent 발동: [{}] 게임 시각 {:.1f}시", def.name, m_gameHour);

    MsgGameLogic_WorldEvent msg{ def.id, def.name, m_gameHour };
    for (auto& [zoneId, zone] : m_zones)
    {
        if (zone)
            zone->Post(msg);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 메시지 처리
// ─────────────────────────────────────────────────────────────────────────────
void GameLogic::OnMessage(GameLogicMessage& msg)
{
    std::visit(overloaded{
        [this](MsgGameLogic_RegisterZone&   m) { Handle(m); },
        [this](MsgGameLogic_UnregisterZone& m) { Handle(m); },
    }, msg);
}

void GameLogic::Handle(MsgGameLogic_RegisterZone& msg)
{
    m_zones[msg.zoneId] = msg.zone;
    LOG_INFO("GameLogic: 존 등록 zoneId={}", msg.zoneId);
}

void GameLogic::Handle(MsgGameLogic_UnregisterZone& msg)
{
    m_zones.erase(msg.zoneId);
    LOG_INFO("GameLogic: 존 해제 zoneId={}", msg.zoneId);
}
