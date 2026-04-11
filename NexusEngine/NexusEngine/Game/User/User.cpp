#include "User.h"

User::User(uint64_t sessionId)
    : m_sessionId(sessionId)
{}

void User::SetIdentification(std::string accountId, std::string token)
{
    m_identification.accountId = std::move(accountId);
    m_identification.token     = std::move(token);
}

void User::SetProfile(UserProfile profile)
{
    m_profile = std::move(profile);
}
