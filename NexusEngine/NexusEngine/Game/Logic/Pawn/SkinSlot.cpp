#include "SkinSlot.h"

void SkinSlot::Equip(Skin* skin)
{
    // TODO: 클라이언트에 외형 변경 패킷 전송
    m_item = skin;
}

void SkinSlot::Unequip()
{
    // TODO: 클라이언트에 외형 원복 패킷 전송
    m_item = nullptr;
}
