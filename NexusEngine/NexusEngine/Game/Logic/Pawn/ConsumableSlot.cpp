#include "ConsumableSlot.h"

void ConsumableSlot::Equip(Consumable* consumable)
{
    // TODO: 퀵슬롯 UI 갱신 패킷 전송
    m_item = consumable;
}

void ConsumableSlot::Unequip()
{
    // TODO: 퀵슬롯 UI 갱신 패킷 전송
    m_item = nullptr;
}

void ConsumableSlot::Use()
{
    if (!HasItem()) return;

    // TODO: 소모품 효과 적용 후 수량 감소 / 소진 시 Unequip
    //       Consumable::Use() 접근 권한 설계 후 호출
}
