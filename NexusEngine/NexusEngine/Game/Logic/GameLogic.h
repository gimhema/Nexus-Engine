#pragma once

#include "../../Actor/Actor.h"
#include "../../Game/Messages/GameMessages.h"

#include <chrono>
#include <functional>
#include <string>
#include <unordered_map>
#include <vector>

class ZoneActor;

// ─────────────────────────────────────────────────────────────────────────────
// WorldEventDef — GameLogic 내부 이벤트 스케줄 항목
//
// triggerHour : 이벤트가 발동할 게임 시각 (0.0–24.0)
// repeating   : true → 매일 반복 / false → 1회 발동 후 제거
// callback    : 이벤트 발동 시 추가로 호출할 람다 (optional)
//               ZoneActor에 직접 Post하거나 서버 내부 상태를 바꾸는 데 사용.
//
// MsgGameLogic_WorldEvent 브로드캐스트는 callback 여부와 무관하게 항상 전송된다.
// ─────────────────────────────────────────────────────────────────────────────
struct WorldEventDef
{
    WorldEventId          id;
    std::string           name;
    float                 triggerHour;
    bool                  repeating{ true };
    std::function<void()> callback;       // null 허용
};

// ─────────────────────────────────────────────────────────────────────────────
// GameLogic — 월드 레벨 자동 이벤트 루프 (Dedicated Actor)
//
// 역할:
//   1. 인게임 세계 시간 관리 (낮/밤 주기)
//      - 실제 경과 시간을 기반으로 게임 시각(0–24h)을 진행
//      - 시간 비율 설정 가능 (예: 실 60초 = 게임 1시간 → 하루 = 실 24분)
//
//   2. 시간 기반 월드 이벤트 스케줄링 및 발동
//      - RegisterEvent()로 이벤트 등록
//      - 매 tick마다 시간 교차를 검사해 발동
//      - 발동 시 등록된 모든 ZoneActor에 MsgGameLogic_WorldEvent 브로드캐스트
//
//   3. 존 레지스트리 관리
//      - RegisterZone() / MsgGameLogic_RegisterZone 메시지로 ZoneActor 등록
//      - 이벤트 브로드캐스트 대상 관리
//
// 사용 예 (Server::Run() 내부):
//   m_gameLogic.SetRealSecondsPerGameHour(60.f); // 1분 = 1게임 시간
//   m_gameLogic.SetStartHour(8.f);
//   m_gameLogic.RegisterEvent({
//       WorldEventId::DungeonOpen, "던전 개방", 18.0f, true,
//       [&] { /* 던전 상태 변경 로직 */ }
//   });
//   m_gameLogic.RegisterZone(zoneId, m_zone.get());
//   m_gameLogic.StartWithTick(1000); // 1초 tick
// ─────────────────────────────────────────────────────────────────────────────
class GameLogic : public Actor<GameLogicMessage>
{
public:
    GameLogic();
    ~GameLogic() override = default;

    GameLogic(const GameLogic&)            = delete;
    GameLogic& operator=(const GameLogic&) = delete;

    // ── 설정 (Start() 전 단일 스레드에서 호출) ────────────────────────────
    // realSeconds: 현실 1게임 시간이 흐르는 데 걸리는 실제 초 수
    // 기본값 60 → 실 1분 = 게임 1시간 = 하루 24분
    void SetRealSecondsPerGameHour(float realSeconds);

    // 게임 시작 시각 설정 (0.0–24.0)
    void SetStartHour(float hour);

    // 낮/밤 경계 시각 설정
    void SetDayNightHours(float dayStartHour, float nightStartHour);

    // ── 이벤트 등록 (Start() 전 단일 스레드에서 호출) ────────────────────
    // 런타임 등록이 필요하면 MsgGameLogic_RegisterZone 메시지 대신
    // 이 함수를 Start() 이전에 직접 호출하는 것을 권장
    void RegisterEvent(WorldEventDef def);

    // ── 존 등록/해제 (Start() 전 또는 MsgGameLogic_RegisterZone 메시지로) ─
    // zone 포인터는 소유하지 않음 — ZoneActor 소멸 전에 반드시 해제할 것
    void RegisterZone(uint32_t zoneId, ZoneActor* zone);

    // ── 상태 조회 (GameLogic 전용 스레드 내에서만 안전) ──────────────────
    [[nodiscard]] float GetWorldHour() const { return m_gameHour; }
    [[nodiscard]] bool  IsDay()        const;
    [[nodiscard]] bool  IsNight()      const;

protected:
    void OnMessage(GameLogicMessage& msg) override;
    void OnTick()                         override;
    void OnStart()                        override;
    void OnStop()                         override;

private:
    // 시간 진행 및 이벤트 발동
    void AdvanceTime(float deltaSec);
    void CheckAndFireEvents(float prevHour, float newHour);
    void BroadcastWorldEvent(const WorldEventDef& def);

    // 메시지 핸들러
    void Handle(MsgGameLogic_RegisterZone&   msg);
    void Handle(MsgGameLogic_UnregisterZone& msg);

    // ── 월드 시간 ──────────────────────────────────────────────────────────
    float m_gameHour{ 8.0f };                // 현재 게임 시각 (0.0–24.0)
    float m_realSecondsPerGameHour{ 60.f };  // 실 N초 = 게임 1시간
    float m_dayStartHour{ 6.0f };
    float m_nightStartHour{ 20.0f };

    // ── 시간 추적 ──────────────────────────────────────────────────────────
    using Clock     = std::chrono::steady_clock;
    using TimePoint = Clock::time_point;
    TimePoint m_lastTickTime;

    // ── 이벤트 스케줄 ─────────────────────────────────────────────────────
    std::vector<WorldEventDef> m_events;

    // ── 존 레지스트리 (raw pointer — 소유권 없음) ────────────────────────
    std::unordered_map<uint32_t, ZoneActor*> m_zones;
};
