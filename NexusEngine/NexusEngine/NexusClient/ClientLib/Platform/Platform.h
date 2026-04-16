#pragma once

// ─────────────────────────────────────────────────────────────────────────────
// Platform.h — 클라이언트 플랫폼 추상화
//
// Windows / Linux / Unreal Engine 이식을 위해
// 소켓 타입과 플랫폼별 헤더를 한 곳에서 관리.
// ─────────────────────────────────────────────────────────────────────────────

#ifdef _WIN32
    #include "Windows/AllowWindowsPlatformTypes.h"
    #ifndef WIN32_LEAN_AND_MEAN
        #define WIN32_LEAN_AND_MEAN
    #endif
    #include <winsock2.h>
    #include <ws2tcpip.h>
    #include "Windows/HideWindowsPlatformTypes.h"

    using NxSocket = SOCKET;
    inline constexpr NxSocket NX_INVALID_SOCKET = INVALID_SOCKET;

    inline int closesocket_nx(NxSocket s) { return ::closesocket(s); }

#else
    #include <sys/socket.h>
    #include <netinet/in.h>
    #include <arpa/inet.h>
    #include <unistd.h>
    #include <cerrno>

    using NxSocket = int;
    inline constexpr NxSocket NX_INVALID_SOCKET = -1;

    inline int closesocket_nx(NxSocket s) { return ::close(s); }
#endif

#include <cstdint>
