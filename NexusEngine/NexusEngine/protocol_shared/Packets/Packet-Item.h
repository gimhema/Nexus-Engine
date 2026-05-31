#pragma once

#include "../NexusTypes.h"
#include "../NexusPacketBuilder.h"
#include "../PacketParser.h"
#include "../Opcodes.h"

#include <cstdint>

// 아이템 정보를 담아서 보낼수있는 별도의 통신용 구조체가 필요함

struct SMsg_StorItem
{

    [[nodiscard]] NexusByteArray  Encode() const;
    static SMsg_StorItem              Decode(NexusPacketParser& r);
};
struct CMsg_DropItem
{
    [[nodiscard]] NexusByteArray  Encode() const;
    static CMsg_DropItem              Decode(NexusPacketParser& r);
};

struct SMsg_DropItem
{
    [[nodiscard]] NexusByteArray  Encode() const;
    static SMsg_DropItem              Decode(NexusPacketParser& r);
};

struct CMsg_UseConsumable
{
    [[nodiscard]] NexusByteArray  Encode() const;
    static CMsg_UseConsumable              Decode(NexusPacketParser& r);
};

struct SMsg_UseConsumable
{
    [[nodiscard]] NexusByteArray  Encode() const;
    static SMsg_UseConsumable              Decode(NexusPacketParser& r);
};

struct CMsg_UseSkin
{
    [[nodiscard]] NexusByteArray  Encode() const;
    static CMsg_UseSkin              Decode(NexusPacketParser& r);
};

struct SMsg_UseSkin
{
    [[nodiscard]] NexusByteArray  Encode() const;
    static SMsg_UseSkin              Decode(NexusPacketParser& r);
};

struct CMsg_UseEquipment
{
    [[nodiscard]] NexusByteArray  Encode() const;
    static CMsg_UseEquipment              Decode(NexusPacketParser& r);
};

struct SMsg_UseEquipment
{
    [[nodiscard]] NexusByteArray  Encode() const;
    static SMsg_UseEquipment              Decode(NexusPacketParser& r);
};

