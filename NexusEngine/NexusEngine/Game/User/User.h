#pragma once

#include <cstdint>
#include <string>

// ─────────────────────────────────────────────────────────────────────────────
// EUser::Status — 유저 연결/인증 상태
//
// 생명주기: CONNECTED → AUTHENTICATED → WAIT_DISCONNECT → (소멸)
// ─────────────────────────────────────────────────────────────────────────────
namespace EUser
{
    enum Status : uint8_t
    {
        CONNECTED       = 0,   // TCP 연결됨, 미인증 (로그인 전)
        AUTHENTICATED   = 1,   // 로그인 완료, 월드/존 이용 가능
        WAIT_DISCONNECT = 2,   // 연결 해제 대기 중 (정리 진행)
    };
}

// ─────────────────────────────────────────────────────────────────────────────
// UserIdentification — 계정 식별 정보
// ─────────────────────────────────────────────────────────────────────────────
struct UserIdentification
{
    std::string accountId;
    std::string token;      // 인증 토큰 (Phase 4: 비밀번호 해시 또는 JWT로 교체)
};

// ─────────────────────────────────────────────────────────────────────────────
// UserProfile — 유저 공개 프로필
// ─────────────────────────────────────────────────────────────────────────────
struct UserProfile
{
    std::string userName;   // 게임 내 표시 이름
};

// ─────────────────────────────────────────────────────────────────────────────
// User — 인증된 계정 정보 + 세션 연결 상태
//
// 역할:
//   SessionActor: 저수준 네트워크 관리 (패킷 송수신)
//   User        : 계정·프로필·인증 상태 관리  ← 여기
//   PlayerPawn  : 순수 인게임 데이터
//
// 소유권:
//   WorldActor가 sessionId → User 맵으로 보관.
//   로그인 성공 시 생성, 로그아웃/연결 해제 시 제거.
//
// 세 객체의 공통 키:
//   sessionId 하나로 SessionActor · User · PlayerPawn을 모두 역참조 가능.
// ─────────────────────────────────────────────────────────────────────────────
class User
{
public:
    explicit User(uint64_t sessionId);
    ~User() = default;

    // ── 식별 ─────────────────────────────────────────────────────────────
    [[nodiscard]] uint64_t           GetSessionId() const { return m_sessionId; }
    [[nodiscard]] const std::string& GetAccountId() const { return m_identification.accountId; }

    // ── 상태 ─────────────────────────────────────────────────────────────
    [[nodiscard]] EUser::Status GetStatus()         const { return m_status; }
    [[nodiscard]] bool          IsAuthenticated()   const { return m_status == EUser::AUTHENTICATED; }
    [[nodiscard]] bool          IsWaitDisconnect()  const { return m_status == EUser::WAIT_DISCONNECT; }
    void SetStatus(EUser::Status status) { m_status = status; }

    // ── 계정 정보 ─────────────────────────────────────────────────────────
    void SetIdentification(std::string accountId, std::string token);
    [[nodiscard]] const UserIdentification& GetIdentification() const { return m_identification; }

    // ── 프로필 ───────────────────────────────────────────────────────────
    void SetProfile(UserProfile profile);
    [[nodiscard]] const UserProfile& GetProfile() const { return m_profile; }

private:
    uint64_t           m_sessionId{};
    EUser::Status      m_status{ EUser::CONNECTED };
    UserIdentification m_identification;
    UserProfile        m_profile;
};
