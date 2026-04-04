set(DUMMYCLIENT_SOURCES
    main.cpp
)

set(DUMMYCLIENT_HEADERS
    Packets/GamePackets.hpp
)

add_executable(NexusDummyClient ${DUMMYCLIENT_SOURCES} ${DUMMYCLIENT_HEADERS})

target_link_libraries(NexusDummyClient PRIVATE NexusClientLib)

# ── 플랫폼별 설정 ──────────────────────────────────────────────────────────────
if(WIN32)
    target_compile_options(NexusDummyClient PRIVATE /W3 /utf-8)
else()
    target_compile_options(NexusDummyClient PRIVATE -Wall -Wextra)
endif()
