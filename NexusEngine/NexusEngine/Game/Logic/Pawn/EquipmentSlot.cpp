#include "EquipmentSlot.h"

void EquipmentSlot::Equip(Equipment* equipment)
{
    // TODO: 플레이어 스탯에 장비 수치 적용
    m_item = equipment;
}

void EquipmentSlot::Unequip()
{
    // TODO: 플레이어 스탯에서 장비 수치 제거
    m_item = nullptr;
}
