set(SOURCES
    main.cpp
    Server.cpp
    ServerNet.cpp
    Connection.cpp
)

set(HEADERS
    Server.h
    ServerNet.h
    Connection.h
    Packets/PacketBase.h
    Platform/Platform.h
)

add_executable(NexusEngine ${SOURCES} ${HEADERS})

target_include_directories(NexusEngine PRIVATE ${CMAKE_CURRENT_SOURCE_DIR})

# ── 플랫폼별 설정 ──────────────────────────────────────────────────────────────
if(WIN32)
    # Windows: Winsock2, MsWSock 링크
    target_compile_options(NexusEngine PRIVATE /W3 /utf-8)
    target_link_libraries(NexusEngine PRIVATE ws2_32 mswsock)
else()
    # Linux: pthreads (std::thread 내부 사용)
    find_package(Threads REQUIRED)
    target_compile_options(NexusEngine PRIVATE -Wall -Wextra)
    target_link_libraries(NexusEngine PRIVATE Threads::Threads)
endif()
