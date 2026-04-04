set(CLIENTLIB_SOURCES
    Packets/PacketBase.cpp
    NetClient.cpp
)

set(CLIENTLIB_HEADERS
    Platform/Platform.h
    Packets/PacketBase.h
    NetClient.h
)

add_library(NexusClientLib STATIC ${CLIENTLIB_SOURCES} ${CLIENTLIB_HEADERS})

target_include_directories(NexusClientLib PUBLIC ${CMAKE_CURRENT_SOURCE_DIR})

# ── 플랫폼별 설정 ──────────────────────────────────────────────────────────────
if(WIN32)
    target_compile_options(NexusClientLib PRIVATE /W3 /utf-8)
    target_link_libraries(NexusClientLib PUBLIC ws2_32)
else()
    find_package(Threads REQUIRED)
    target_compile_options(NexusClientLib PRIVATE -Wall -Wextra)
    target_link_libraries(NexusClientLib PUBLIC Threads::Threads)
endif()
