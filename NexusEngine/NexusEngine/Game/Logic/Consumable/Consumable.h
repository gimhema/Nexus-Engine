#pragma once

#include "../Item.h"

class Consumable : public ItemBase
{
    public:
        Consumable() {}
        ~Consumable() {}

    protected:
        void Create(ItemBasicInfo iInfo) override;
        void Use() override;

    public:
        int ItemNum;

};