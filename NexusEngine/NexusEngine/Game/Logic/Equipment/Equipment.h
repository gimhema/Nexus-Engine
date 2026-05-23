#pragma once

#include "../Item.h"


class Equipment : public ItemBase
{
    public:
        Equipment() {}
        ~Equipment() {}       
  
    protected:
        void Create(ItemBasicInfo iInfo) override;
        void Use() override;
    
    public:
        void Attach(EQUIPMENT_POS_TYPE parts);
        void Detach(EQUIPMENT_POS_TYPE parts);
        
};