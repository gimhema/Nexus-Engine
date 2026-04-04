#include "User.h"


User::User(std::shared_ptr<Session> session)
    : m_session(std::move(session))
{
}

User::~User()
{

}

