#pragma once

#include "../Messages/GameMessages.h"                   // Vec3
#include "../Data/GameDataEntities/EntityBase.h"        // EEntity::EID
#include "../Data/Faction/FactionTypes.h"               // EFactionId, EAIType

#include <cstdint>
#include <string>
#include <vector>

// ─────────────────────────────────────────────────────────────────────────────
// SpawnPoint — 플레이어 기본 스폰 위치 정의
// ─────────────────────────────────────────────────────────────────────────────
struct SpawnPoint
{
    Vec3  pos;
    float orientation{ 0.f };
};

// ─────────────────────────────────────────────────────────────────────────────
// NpcSpawnDef — 존 초기화 시 생성할 NPC/몬스터 정의
// ─────────────────────────────────────────────────────────────────────────────
struct NpcSpawnDef
{
    Vec3         pos;
    float        orientation{ 0.f };
    std::string  name;

    // 전투 스탯 — NpcEntityData 또는 MonsterEntityData 생성에 사용
    int32_t      maxHp{ 100 };
    int32_t      attack{ 10 };
    int32_t      defense{ 5 };
    float        moveSpeed{ 3.f };

    // 진영 / AI 설정
    EFactionId   factionId{ EFactionId::NONE };
    EAIType      aiType{ EAIType::PASSIVE };
    float        aggroRange{ 0.f };

    // NPC(대화/퀘스트) vs MONSTER(전투/드롭)
    EEntity::EID entityType{ EEntity::EID::NPC };
};

// ─────────────────────────────────────────────────────────────────────────────
// ZoneConfig — Zone 정적 설정. 서버 시작 시 1회 구성.
// ─────────────────────────────────────────────────────────────────────────────
struct ZoneConfig
{
    uint32_t    id{};
    std::string name;

    // 맵 경계 (AABB) — 플레이어 이동 검증용
    Vec3 boundsMin{ -1000.f, -1000.f, -1000.f };
    Vec3 boundsMax{  1000.f,  1000.f,  1000.f };

    // 플레이어 기본 스폰 위치 목록 (라운드 로빈 순환)
    std::vector<SpawnPoint> playerSpawnPoints;

    // 존 시작 시 초기 스폰할 NPC/몬스터 목록
    std::vector<NpcSpawnDef> npcSpawns;

    uint32_t maxPlayers{ 200 };
};

// ─────────────────────────────────────────────────────────────────────────────
// Zone — 존의 정적 데이터 보관 및 쿼리 인터페이스.
//
// ZoneActor가 값으로 소유하며 ZoneActor 전용 스레드에서만 접근.
// ─────────────────────────────────────────────────────────────────────────────
class Zone
{
public:
    explicit Zone(ZoneConfig config);

    [[nodiscard]] uint32_t           GetId()         const { return m_config.id; }
    [[nodiscard]] const std::string& GetName()       const { return m_config.name; }
    [[nodiscard]] uint32_t           GetMaxPlayers() const { return m_config.maxPlayers; }

    // 플레이어 스폰 위치 순환 선택 (playerSpawnPoints가 비어있으면 원점 반환)
    [[nodiscard]] Vec3 PickPlayerSpawn() const;

    // 위치가 존 경계(AABB) 내인지 검사
    [[nodiscard]] bool IsInBounds(const Vec3& pos) const;

    [[nodiscard]] const std::vector<NpcSpawnDef>& GetNpcSpawns() const;

private:
    ZoneConfig       m_config;
    mutable uint32_t m_spawnRoundRobin{ 0 };  // PickPlayerSpawn 순환 인덱스
};
