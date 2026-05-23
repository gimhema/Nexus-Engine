#pragma once

#include "../Item.h"



class Skin : public ItemBase
{
    public:
        Skin() {}
        ~Skin() {}

    protected:
        void Create(ItemBasicInfo iInfo) override;
        void Use() override;
    
    public:
        void Attach(SKIN_PARTS_TYPE parts);
        void Detach(SKIN_PARTS_TYPE parts);
};






