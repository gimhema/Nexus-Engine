#pragma once

#include "Item.h"

class Skin : public ItemBase
{
    public:
        Skin() {}
        ~Skin() {}

    protected:
        void Create(ItemBasicInfo iInfo) override;
};






