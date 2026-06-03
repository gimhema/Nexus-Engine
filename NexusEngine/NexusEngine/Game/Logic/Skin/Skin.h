#pragma once

#include "../Item.h"



class Skin : public ItemBase
{
    public:
        explicit Skin(SKIN_PARTS_TYPE parts);
        ~Skin() override = default;

    protected:
        void Create(ItemBasicInfo iInfo) override;
        void Use() override;

    public:
        void Attach(SKIN_PARTS_TYPE parts);
        void Detach(SKIN_PARTS_TYPE parts);

        SKIN_PARTS_TYPE GetPartsType() const { return m_partsType; }

    private:
        SKIN_PARTS_TYPE m_partsType;
};






