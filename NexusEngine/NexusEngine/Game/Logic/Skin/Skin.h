#pragma once

#include "Item.h"

enum class SKIN_PARTS_TYPE
{
    _DEFAULT = 0,
    _HEAD = 1,
    _BODY_UP = 2,
    _BODY_DOWN = 3,
    _HAND = 4,
    _SHOES = 5
};

class Skin : public ItemBase
{
    public:
        Skin() {}
        ~Skin() {}

    protected:
        void Create(ItemBasicInfo iInfo) override;
        void Use() override;
    
    public:
        void Attach();
        void Detach();
};






