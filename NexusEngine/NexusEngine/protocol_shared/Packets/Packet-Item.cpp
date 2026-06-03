#include "Packet-Item.h"

// ── SMsg_StoreItem ────────────────────────────────────────────────────────────
NexusByteArray SMsg_StoreItem::Encode() const
{
    NexusPacketBuilder b(SMSG_STORE_ITEM);
    b.WriteU8(itemType).WriteU64(itemId).WriteU32(bagPos).WriteU8(subType);
    return b.Finalize();
}

SMsg_StoreItem SMsg_StoreItem::Decode(NexusPacketParser& r)
{
    SMsg_StoreItem pkt;
    pkt.itemType = r.ReadU8();
    pkt.itemId   = r.ReadU64();
    pkt.bagPos   = r.ReadU32();
    pkt.subType  = r.ReadU8();
    return pkt;
}

// ── CMsg_DropItem ─────────────────────────────────────────────────────────────
NexusByteArray CMsg_DropItem::Encode() const
{
    NexusPacketBuilder b(CMSG_DROP_ITEM);
    b.WriteU8(itemType).WriteU32(bagPos);
    return b.Finalize();
}

CMsg_DropItem CMsg_DropItem::Decode(NexusPacketParser& r)
{
    CMsg_DropItem pkt;
    pkt.itemType = r.ReadU8();
    pkt.bagPos   = r.ReadU32();
    return pkt;
}

// ── SMsg_DropItem ─────────────────────────────────────────────────────────────
NexusByteArray SMsg_DropItem::Encode() const
{
    NexusPacketBuilder b(SMSG_DROP_ITEM);
    b.WriteU8(success).WriteU8(itemType).WriteU32(bagPos);
    return b.Finalize();
}

SMsg_DropItem SMsg_DropItem::Decode(NexusPacketParser& r)
{
    SMsg_DropItem pkt;
    pkt.success  = r.ReadU8();
    pkt.itemType = r.ReadU8();
    pkt.bagPos   = r.ReadU32();
    return pkt;
}

// ── CMsg_UseConsumable ────────────────────────────────────────────────────────
NexusByteArray CMsg_UseConsumable::Encode() const
{
    NexusPacketBuilder b(CMSG_USE_CONSUMABLE);
    b.WriteU32(quickSlotPos);
    return b.Finalize();
}

CMsg_UseConsumable CMsg_UseConsumable::Decode(NexusPacketParser& r)
{
    CMsg_UseConsumable pkt;
    pkt.quickSlotPos = r.ReadU32();
    return pkt;
}

// ── SMsg_UseConsumable ────────────────────────────────────────────────────────
NexusByteArray SMsg_UseConsumable::Encode() const
{
    NexusPacketBuilder b(SMSG_USE_CONSUMABLE);
    b.WriteU64(sessionId).WriteU32(quickSlotPos);
    return b.Finalize();
}

SMsg_UseConsumable SMsg_UseConsumable::Decode(NexusPacketParser& r)
{
    SMsg_UseConsumable pkt;
    pkt.sessionId    = r.ReadU64();
    pkt.quickSlotPos = r.ReadU32();
    return pkt;
}

// ── CMsg_UseSkin ──────────────────────────────────────────────────────────────
NexusByteArray CMsg_UseSkin::Encode() const
{
    NexusPacketBuilder b(CMSG_USE_SKIN);
    b.WriteU32(bagPos);
    return b.Finalize();
}

CMsg_UseSkin CMsg_UseSkin::Decode(NexusPacketParser& r)
{
    CMsg_UseSkin pkt;
    pkt.bagPos = r.ReadU32();
    return pkt;
}

// ── SMsg_UseSkin ──────────────────────────────────────────────────────────────
NexusByteArray SMsg_UseSkin::Encode() const
{
    NexusPacketBuilder b(SMSG_USE_SKIN);
    b.WriteU64(sessionId).WriteU8(partsType).WriteU64(itemId);
    return b.Finalize();
}

SMsg_UseSkin SMsg_UseSkin::Decode(NexusPacketParser& r)
{
    SMsg_UseSkin pkt;
    pkt.sessionId  = r.ReadU64();
    pkt.partsType  = r.ReadU8();
    pkt.itemId     = r.ReadU64();
    return pkt;
}

// ── CMsg_UseEquipment ─────────────────────────────────────────────────────────
NexusByteArray CMsg_UseEquipment::Encode() const
{
    NexusPacketBuilder b(CMSG_USE_EQUIPMENT);
    b.WriteU32(bagPos);
    return b.Finalize();
}

CMsg_UseEquipment CMsg_UseEquipment::Decode(NexusPacketParser& r)
{
    CMsg_UseEquipment pkt;
    pkt.bagPos = r.ReadU32();
    return pkt;
}

// ── SMsg_UseEquipment ─────────────────────────────────────────────────────────
NexusByteArray SMsg_UseEquipment::Encode() const
{
    NexusPacketBuilder b(SMSG_USE_EQUIPMENT);
    b.WriteU8(success).WriteU8(posType).WriteU64(itemId);
    return b.Finalize();
}

SMsg_UseEquipment SMsg_UseEquipment::Decode(NexusPacketParser& r)
{
    SMsg_UseEquipment pkt;
    pkt.success = r.ReadU8();
    pkt.posType = r.ReadU8();
    pkt.itemId  = r.ReadU64();
    return pkt;
}
