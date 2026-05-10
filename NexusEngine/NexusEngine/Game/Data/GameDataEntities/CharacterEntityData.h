#pragma once

#include "EntityBase.h"

#include <cstdint>

// ─────────────────────────────────────────────────────────────────────────────
// CharacterEntityData — 플레이어 캐릭터 전용 인게임 데이터
//
// PlayerPawn이 소유.
// 기본 전투 스탯 외에 레벨·경험치를 관리.
// 레벨업 시 스탯(maxHp, attack, defense)이 자동 증가.
//
// 향후 추가 예정:
//   - 인벤토리 (ItemSlot 배열)
//   - 장비 슬롯 (EquipSlot enum 기반 배열)
//   - 스킬 목록
// ─────────────────────────────────────────────────────────────────────────────
class CharacterEntityData : public GameDataEntityBase
{
public:
    // 신규 캐릭터 기본값으로 생성 (진영은 캐릭터 생성 시 선택, 임시 기본값 ALLIANCE)
    explicit CharacterEntityData(EFactionId factionId = EFactionId::ALLIANCE);

    // DB에서 로드한 데이터로 생성
    CharacterEntityData(int32_t maxHp, int32_t attack, int32_t defense, float moveSpeed,
                        uint32_t level, uint64_t experience,
                        EFactionId factionId = EFactionId::ALLIANCE);

    EEntity::EID GetEntityType() const override { return EEntity::EID::PLAYER; }

    // ── 레벨 / 경험치 ──────────────────────────────────────────────────────
    [[nodiscard]] uint32_t GetLevel()                const { return m_level; }
    [[nodiscard]] uint64_t GetExperience()            const { return m_experience; }
    [[nodiscard]] uint64_t GetExperienceToNextLevel() const { return m_expToNextLevel; }

    // 경험치 추가. 레벨업이 발생했으면 true 반환 (연속 레벨업 가능).
    bool AddExperience(uint64_t amount);

    // ── MP ─────────────────────────────────────────────────────────────────
    [[nodiscard]] int32_t GetMp()    const { return m_mp; }
    [[nodiscard]] int32_t GetMaxMp() const { return m_maxMp; }

    // MP 소모. 부족하면 false 반환 (소모 안 함).
    bool ConsumeMp(int32_t cost);
    void RestoreMp(int32_t amount);

private:
    // 레벨업 시 스탯 증가 적용 (maxHp, attack, defense, maxMp)
    void ApplyLevelUp();

    // 다음 레벨에 필요한 경험치 계산 (level^2 * 100)
    [[nodiscard]] static uint64_t CalcExpToNextLevel(uint32_t level);

    uint32_t m_level{ 1 };
    uint64_t m_experience{ 0 };
    uint64_t m_expToNextLevel{ 100 };   // CalcExpToNextLevel(1)

    int32_t  m_maxMp{ 50 };            // 기본 최대 MP
    int32_t  m_mp{ 50 };               // 현재 MP
};
