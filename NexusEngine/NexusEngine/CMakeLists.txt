set(SOURCES
    main.cpp
    Server.cpp
    ServerNet.cpp
    Connection.cpp
    Packets/PacketBase.cpp
    Shared/Logger.cpp
    Arbiter/Arbiter.cpp
    Arbiter/ArbiterSession.cpp
    Game/Data/GameDataEntities/CharacterEntityData.cpp
    Game/Data/GameDataEntities/NpcEntityData.cpp
    Game/Data/GameDataEntities/MonsterEntityData.cpp
    Game/Data/Faction/FactionRegistry.cpp
    Game/Data/Skill/SkillRegistry.cpp
    Game/Logic/Combat/CombatProcessor.cpp
    Game/Logic/GameLogic.cpp
    Game/Logic/Pawn/Pawn.cpp
    Game/Logic/Pawn/PlayerPawn.cpp
    Game/Data/GameDataEntities/EntityBase.cpp
    Game/User/User.cpp
    Game/Zone/Zone.cpp

    protocol_shared/Packets/Packet-Auth.cpp
    protocol_shared/Packets/Packet-Movement.cpp
    protocol_shared/Packets/Packet-Chat.cpp
    protocol_shared/Packets/Packet-Spawn.cpp
    protocol_shared/Packets/Packet-Combat.cpp

    Game/Actors/SessionActor.cpp
    Game/Actors/WorldActor.cpp
    Game/Actors/ZoneActor.cpp
    Game/Logic/Equipment/Equipment.cpp
    Game/Logic/Inventory/Inventory.cpp
    Game/Logic/Skin/Skin.cpp
    Game/Table/TableBase.cpp
    Game/Logic/Consumable/Consumable.cpp
    Game/Logic/Item.cpp
    Game/Logic/Pawn/PlayerItemComponent.cpp
    Game/Logic/Pawn/ItemBag.cpp
    Game/Logic/Pawn/ItemSlot.cpp
)

set(HEADERS
    Server.h
    ServerNet.h
    Arbiter/Arbiter.h
    Arbiter/ArbiterSession.h
    protocol_shared/ArbiterOpcodes.h
    Connection.h
    Packets/PacketBase.h
    Platform/Platform.h
    Shared/Logger.h
    Actor/Mailbox.h
    Actor/Actor.h
    Actor/ActorSystem.h
    Game/Logic/GameLogic.h
    Game/Logic/Pawn/Pawn.h
    Game/Logic/Pawn/PlayerPawn.h
    Game/User/User.h
    Game/Zone/Zone.h

    Game/Data/GameDataEntities/EntityBase.h
    Game/Data/GameDataEntities/CharacterEntityData.h
    Game/Data/GameDataEntities/NpcEntityData.h
    Game/Data/GameDataEntities/MonsterEntityData.h
    Game/Data/Faction/FactionTypes.h
    Game/Data/Faction/FactionRegistry.h
    Game/Data/Skill/SkillDef.h
    Game/Data/Skill/SkillRegistry.h
    protocol_shared/NexusPacketBuilder.h
    protocol_shared/Packets/Packet-Auth.h
    protocol_shared/Packets/Packet-Movement.h
    protocol_shared/Packets/Packet-Chat.h
    protocol_shared/Packets/Packet-Spawn.h
    protocol_shared/Packets/Packet-Combat.h
    Game/Messages/GameMessages.h
    Game/Actors/SessionActor.h
    Game/Actors/WorldActor.h
    Game/Actors/ZoneActor.h
    Game/Data/GamePackets/Packet-Example.hpp
    Game/Logic/Combat/CombatProcessor.h
    Game/Logic/Equipment/Equipment.h
    Game/Logic/Inventory/Inventory.h
    Game/Logic/Skin/Skin.h
    NexusClient/ClientLib/NetClient.h
    NexusClient/ClientLib/Packets/PacketBase.h
    NexusClient/ClientLib/Platform/Platform.h
    NexusClient/DummyClient/Packets/GamePackets.hpp
    protocol_shared/NexusTypes.h
    protocol_shared/Opcodes.h
    protocol_shared/PacketParser.h
    Game/Table/TableBase.h
    Game/Logic/Consumable/Consumable.h
    Game/Logic/Item.h
    Game/Logic/CommonLogicType.h
    Game/Logic/Pawn/PlayerItemComponent.h
    Game/Logic/Pawn/ItemBag.h
    Game/Logic/Pawn/ItemSlot.h
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
