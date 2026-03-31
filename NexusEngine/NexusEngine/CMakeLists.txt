set(SOURCES
    main.cpp
    Server.cpp
    ServerNet.cpp
    Connection.cpp
    Packets/PacketBase.cpp
    Shared/Logger.cpp
    Game/Logic/GameLogic.cpp
    Game/User/User.cpp
    Game/Zone/Zone.cpp
    Game/Actors/SessionActor.cpp
    Game/Actors/WorldActor.cpp
    Game/Actors/ZoneActor.cpp
)

set(HEADERS
    Server.h
    ServerNet.h
    Connection.h
    Packets/PacketBase.h
    Platform/Platform.h
    Shared/Logger.h
    Actor/Mailbox.h
    Actor/Actor.h
    Actor/ActorSystem.h
    Game/Logic/GameLogic.h
    Game/User/User.h
    Game/Zone/Zone.h
    Game/Data/GameDataEntities/EntityBase.h
    Game/Data/GamePackets/Packet-Example.hpp
    Game/Messages/GameMessages.h
    Game/Actors/SessionActor.h
    Game/Actors/WorldActor.h
    Game/Actors/ZoneActor.h
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
