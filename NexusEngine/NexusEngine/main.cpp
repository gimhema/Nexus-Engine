#include "Server.h"
#include "Arbiter/Arbiter.h"

int main()
{
    // 비동기로 동작 — Server::Run() 과 독립적으로 실행됨
    // 런처가 연결되지 않아도 서버 동작에 영향 없음
    Arbiter arbiter;
    arbiter.Run();

    Server server;
    server.Run();

    arbiter.Stop();

    return 0;
}
