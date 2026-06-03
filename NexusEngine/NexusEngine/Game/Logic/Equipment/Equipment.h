#pragma once

#include "../Item.h"


class Equipment : public ItemBase
{
    public:
        explicit Equipment(EQUIPMENT_POS_TYPE pos);
        ~Equipment() override = default;

    protected:
        void Create(ItemBasicInfo iInfo) override;
        void Use() override;

    public:
        void Attach(EQUIPMENT_POS_TYPE parts);
        void Detach(EQUIPMENT_POS_TYPE parts);

        EQUIPMENT_POS_TYPE GetPosType() const { return m_posType; }

    private:
        EQUIPMENT_POS_TYPE m_posType = EQUIPMENT_POS_TYPE::_DEFAULT;
};