#include "PlayerItemComponent.h"

// ── Store ─────────────────────────────────────────────────────────────────────
// 해당 가방의 첫 번째 빈 슬롯에 삽입한다.
void ItemComponent::Store(ItemBase* item)
{
    if (!item) return;

    switch (item->GetItemType())
    {
    case ITEM_TYPE::EQUIPMENT:
    {
        auto* eq = static_cast<Equipment*>(item);
        for (int i = 0; i < MAX_EQUIPMENT_BAG_NUM; ++i)
        {
            if (itemBag.equipmentBag.IsEmpty(i))
            {
                itemBag.equipmentBag.Add(eq, i);
                return;
            }
        }
        break;
    }
    case ITEM_TYPE::SKIN:
    {
        auto* sk = static_cast<Skin*>(item);
        for (int i = 0; i < MAX_SKIN_BAG_NUM; ++i)
        {
            if (itemBag.skinBag.IsEmpty(i))
            {
                itemBag.skinBag.Add(sk, i);
                return;
            }
        }
        break;
    }
    case ITEM_TYPE::CONSUMABLE:
    {
        auto* cs = static_cast<Consumable*>(item);
        for (int i = 0; i < MAX_CONSUMABLE_BAG_NUM; ++i)
        {
            if (itemBag.consumableBag.IsEmpty(i))
            {
                itemBag.consumableBag.Add(cs, i);
                return;
            }
        }
        break;
    }
    default:
        break;
    }
}

// ── DropItem ──────────────────────────────────────────────────────────────────
void ItemComponent::DropItem(ITEM_TYPE bagType, int pos)
{
    switch (bagType)
    {
    case ITEM_TYPE::EQUIPMENT:  itemBag.equipmentBag.Drop(pos);  break;
    case ITEM_TYPE::SKIN:       itemBag.skinBag.Drop(pos);       break;
    case ITEM_TYPE::CONSUMABLE: itemBag.consumableBag.Drop(pos); break;
    default: break;
    }
}

// ── SwapEquip ─────────────────────────────────────────────────────────────────
// 가방[pos] 의 장비를 GetPosType() 이 가리키는 장착 슬롯으로 이동.
// 기존 장착 장비가 있으면 가방[pos] 로 돌아온다.
void ItemComponent::SwapEquip(int pos)
{
    if (!itemBag.equipmentBag.HasItem(pos)) return;

    Equipment* incoming = &itemBag.equipmentBag.GetSlot(pos).Item();
    int slotIdx = static_cast<int>(incoming->GetPosType());

    if (itemSlot.CurrentEquipments[slotIdx].HasItem())
    {
        Equipment* outgoing = &itemSlot.CurrentEquipments[slotIdx].Item();
        itemSlot.CurrentEquipments[slotIdx].Unequip();          // 스탯 제거
        itemBag.equipmentBag.GetSlot(pos).Equip(outgoing);     // 가방 복귀
    }
    else
    {
        itemBag.equipmentBag.Drop(pos);
    }

    itemSlot.CurrentEquipments[slotIdx].Equip(incoming);        // 스탯 적용
}

// ── SwapSkin ──────────────────────────────────────────────────────────────────
// 가방[pos] 의 외형을 GetPartsType() 이 가리키는 외형 슬롯으로 이동.
// 기존 외형이 있으면 가방[pos] 로 돌아온다.
// 성공 시 장착된 스킨의 partsType·itemId 를 반환.
SkinSwapResult ItemComponent::SwapSkin(int pos)
{
    if (!itemBag.skinBag.HasItem(pos))
        return {};

    Skin* incoming = &itemBag.skinBag.GetSlot(pos).Item();
    int slotIdx = static_cast<int>(incoming->GetPartsType());

    if (itemSlot.CurrentSkins[slotIdx].HasItem())
    {
        Skin* outgoing = &itemSlot.CurrentSkins[slotIdx].Item();
        itemSlot.CurrentSkins[slotIdx].Unequip();               // 렌더링 원복
        itemBag.skinBag.GetSlot(pos).Equip(outgoing);          // 가방 복귀
    }
    else
    {
        itemBag.skinBag.Drop(pos);
    }

    itemSlot.CurrentSkins[slotIdx].Equip(incoming);             // 렌더링 반영

    return {
        true,
        static_cast<uint8_t>(incoming->GetPartsType()),
        incoming->GetItemId()
    };
}

// ── UseConsumbale ─────────────────────────────────────────────────────────────
void ItemComponent::UseConsumbale(int pos)
{
    itemSlot.ConsumableQuickSlot[pos].Use();
}
