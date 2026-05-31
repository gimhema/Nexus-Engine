#pragma once

#include "../NexusTypes.h"
#include "../NexusPacketBuilder.h"
#include "../PacketParser.h"
#include "../Opcodes.h"

#include <cstdint>

    // SMSG_STORE_ITEM,                   // S-C (TCP) 아이템 획득시
    // CMSG_DROP_ITEM,                    // C-S (TCP) 아이템 드랍 요청
    // SMSG_DROP_ITEM,                    // S-C (TCP) 아이템 드랍 요청 서버처리

    // // Consumable Action
    // CMSG_USE_CONSUMABLE,
    // SMSG_USE_CONSUMABLE,
    
    // // Skin Action
    // CMSG_USE_SKIN,
    // SMSG_USE_SKIN,

    // // Equipment Action
    // CMSG_USE_EQUIPMENT,
    // SMSG_USE_EQUIPMENT,


struct SMsg_StorItem
{

};

