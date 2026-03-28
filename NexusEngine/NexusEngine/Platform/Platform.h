#pragma once

// ─────────────────────────────────────────────────────────────────────────────
// Platform.h
//
// 플랫폼(Windows / Linux)별 소켓·핸들 타입과 시스템 헤더를 통합.
// 네트워크 관련 파일은 winsock2.h / sys/socket.h 대신 이 파일만 include.
//
// 제공 타입:
//   NxSocket  — 소켓 디스크립터  (Windows: SOCKET / Linux: int)
//   NxHandle  — I/O 핸들        (Windows: HANDLE / Linux: epoll fd int)
//   NxDword   — 32비트 정수      (Windows: DWORD  / Linux: uint32_t)
//
// 제공 상수:
//   NX_INVALID_SOCKET  — 유효하지 않은 소켓 값
//   NX_INVALID_HANDLE  — 유효하지 않은 핸들 값
// ─────────────────────────────────────────────────────────────────────────────

#include <cstdint>

#ifdef _WIN32

    #ifndef WIN32_LEAN_AND_MEAN
    #define WIN32_LEAN_AND_MEAN
    #endif
    #include <winsock2.h>
    #include <mswsock.h>   // AcceptEx, GetAcceptExSockaddrs
    #include <windows.h>
    #include <ws2tcpip.h>

    using NxSocket = SOCKET;
    using NxHandle = HANDLE;
    using NxDword  = DWORD;

    inline constexpr NxSocket NX_INVALID_SOCKET = INVALID_SOCKET;
    inline constexpr NxHandle NX_INVALID_HANDLE = INVALID_HANDLE_VALUE;

#else // Linux

    #include <sys/socket.h>
    #include <sys/epoll.h>
    #include <netinet/in.h>
    #include <arpa/inet.h>
    #include <unistd.h>
    #include <fcntl.h>

    using NxSocket = int;
    using NxHandle = int;        // epoll fd
    using NxDword  = uint32_t;

    inline constexpr NxSocket NX_INVALID_SOCKET = -1;
    inline constexpr NxHandle NX_INVALID_HANDLE = -1;

    // Windows API 호환 래퍼 (구현 코드의 #ifdef 최소화)
    inline int  closesocket(NxSocket s) { return ::close(s); }

#endif
