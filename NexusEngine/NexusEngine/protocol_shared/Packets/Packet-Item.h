#pragma once

#include "../NexusTypes.h"
#include "../NexusPacketBuilder.h"
#include "../PacketParser.h"
#include "../Opcodes.h"

#include <cstdint>

// ─────────────────────────────────────────────────────────────────────────────
// Packet-Item.h — 아이템 그룹 패킷  (0x0600번대)
// ─────────────────────────────────────────────────────────────────────────────

// ── SMSG_STORE_ITEM (0x0601) ──────────────────────────────────────────────────
// S→C (TCP) [uint8 itemType][uint64 itemId][uint32 bagPos][uint8 subType]
//   itemType : ITEM_TYPE
//   itemId   : 아이템 고유 ID
//   bagPos   : 저장된 가방 슬롯 번호
//   subType  : Skin → SKIN_PARTS_TYPE, Equipment → EQUIPMENT_POS_TYPE, 나머지 0
struct SMsg_StoreItem
{
    uint8_t  itemType{ 0 };
    uint64_t itemId{ 0 };
    uint32_t bagPos{ 0 };
    uint8_t  subType{ 0 };

    [[nodiscard]] NexusByteArray  Encode() const;
    static SMsg_StoreItem         Decode(NexusPacketParser& r);
};

// ── CMSG_DROP_ITEM (0x0602) ───────────────────────────────────────────────────
// C→S (TCP) [uint8 itemType][uint32 bagPos]
struct CMsg_DropItem
{
    uint8_t  itemType{ 0 };
    uint32_t bagPos{ 0 };

    [[nodiscard]] NexusByteArray  Encode() const;
    static CMsg_DropItem          Decode(NexusPacketParser& r);
};

// ── SMSG_DROP_ITEM (0x0603) ───────────────────────────────────────────────────
// S→C (TCP) [uint8 success][uint8 itemType][uint32 bagPos]
struct SMsg_DropItem
{
    uint8_t  success{ 0 };
    uint8_t  itemType{ 0 };
    uint32_t bagPos{ 0 };

    [[nodiscard]] NexusByteArray  Encode() const;
    static SMsg_DropItem          Decode(NexusPacketParser& r);
};

// ── CMSG_USE_CONSUMABLE (0x0604) ──────────────────────────────────────────────
// C→S (TCP) [uint32 quickSlotPos]
struct CMsg_UseConsumable
{
    uint32_t quickSlotPos{ 0 };

    [[nodiscard]] NexusByteArray  Encode() const;
    static CMsg_UseConsumable     Decode(NexusPacketParser& r);
};

// ── SMSG_USE_CONSUMABLE (0x0605) ──────────────────────────────────────────────
// S→C (TCP) [uint64 sessionId][uint32 quickSlotPos]
struct SMsg_UseConsumable
{
    uint64_t sessionId{ 0 };
    uint32_t quickSlotPos{ 0 };

    [[nodiscard]] NexusByteArray  Encode() const;
    static SMsg_UseConsumable     Decode(NexusPacketParser& r);
};

// ── CMSG_USE_SKIN (0x0606) ────────────────────────────────────────────────────
// C→S (TCP) [uint32 bagPos]
//   bagPos : SkinBag 슬롯 번호 → 해당 스킨의 partsType 슬롯으로 자동 장착
struct CMsg_UseSkin
{
    uint32_t bagPos{ 0 };

    [[nodiscard]] NexusByteArray  Encode() const;
    static CMsg_UseSkin           Decode(NexusPacketParser& r);
};

// ── SMSG_USE_SKIN (0x0607) ────────────────────────────────────────────────────
// S→C (TCP) [uint64 sessionId][uint8 partsType][uint64 itemId]
//   partsType : SKIN_PARTS_TYPE
//   itemId    : 장착된 스킨 아이템 ID (0 = 해제 / 기본 외형)
struct SMsg_UseSkin
{
    uint64_t sessionId{ 0 };
    uint8_t  partsType{ 0 };
    uint64_t itemId{ 0 };

    [[nodiscard]] NexusByteArray  Encode() const;
    static SMsg_UseSkin           Decode(NexusPacketParser& r);
};

// ── CMSG_USE_EQUIPMENT (0x0608) ───────────────────────────────────────────────
// C→S (TCP) [uint32 bagPos]
//   bagPos : EquipmentBag 슬롯 번호 → 해당 장비의 posType 슬롯으로 자동 장착
struct CMsg_UseEquipment
{
    uint32_t bagPos{ 0 };

    [[nodiscard]] NexusByteArray  Encode() const;
    static CMsg_UseEquipment      Decode(NexusPacketParser& r);
};

// ── SMSG_USE_EQUIPMENT (0x0609) ───────────────────────────────────────────────
// S→C (TCP) [uint8 success][uint8 posType][uint64 itemId]
//   posType : EQUIPMENT_POS_TYPE
//   itemId  : 장착된 장비 아이템 ID
struct SMsg_UseEquipment
{
    uint8_t  success{ 0 };
    uint8_t  posType{ 0 };
    uint64_t itemId{ 0 };

    [[nodiscard]] NexusByteArray  Encode() const;
    static SMsg_UseEquipment      Decode(NexusPacketParser& r);
};
