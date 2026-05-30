#pragma once

#include "../../Messages/GameMessages.h"   // Vec3
#include "../../Zone/Zone.h"

#include <cstdint>

class Pawn;

// ─────────────────────────────────────────────────────────────────────────────
// MovementProcessor — 이동 검증 + 상태 적용
//
// ZoneActor 전용 스레드에서만 호출. 상태 없는 순수 함수 모음.
//
// TCP 처리 순서:
//   1. 존 경계 검증 (AABB)
//   2. Pawn 위치·방향 갱신
//
// UDP 경로:
//   고빈도 이동이므로 경계 검증 없이 바로 적용.
//   TCP 에서 이미 경계 보정이 이루어진 것으로 간주한다.
// ─────────────────────────────────────────────────────────────────────────────

namespace MovementProcessor
{
    enum class EResult : uint8_t
    {
        Ok,           // 이동 적용 성공
        OutOfBounds,  // 존 경계 초과
    };

    // TCP 이동 — 경계 검증 포함
    // flags: MsgSession_Move::movementFlags (향후 dash/jump 상태 검증에 사용)
    EResult ApplyMove(Pawn& pawn, const Vec3& pos, float orientation,
                      uint32_t flags, const Zone& zone);

    // UDP 이동 — 경계 검증 없음 (고빈도 경로)
    void ApplyMoveUdp(Pawn& pawn, const Vec3& pos, float orientation);

} // namespace MovementProcessor
