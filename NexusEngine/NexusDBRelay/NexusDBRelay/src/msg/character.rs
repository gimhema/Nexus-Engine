use anyhow::{bail, Result};

use super::reader::PacketReader;
use super::validate::validate_character_name;

/// ReqCreateCharacter 페이로드
pub struct CreateCharacterReq {
    pub session_id:     u64,
    pub account_id:     u32,
    pub character_name: String,
}

impl CreateCharacterReq {
    pub fn decode(r: &mut PacketReader) -> Result<Self> {
        Ok(Self {
            session_id:     r.read_u64()?,
            account_id:     r.read_u32()?,
            character_name: r.read_string()?,
        })
    }

    pub fn validate(&self) -> Result<()> {
        validate_character_name(&self.character_name)
    }
}

/// ReqLoadCharacter 페이로드
pub struct LoadCharacterReq {
    pub session_id:   u64,
    pub character_id: u32,
}

impl LoadCharacterReq {
    pub fn decode(r: &mut PacketReader) -> Result<Self> {
        Ok(Self {
            session_id:   r.read_u64()?,
            character_id: r.read_u32()?,
        })
    }
}

/// ReqSaveCharacter 페이로드 — 캐릭터 가변 상태 전체
pub struct SaveCharacterReq {
    pub session_id:   u64,
    pub character_id: u32,
    pub level:        u32,
    pub experience:   u64,
    pub hp:           u32,
    pub zone_id:      u32,
    pub pos_x:        f32,
    pub pos_y:        f32,
    pub pos_z:        f32,
    pub orientation:  f32,
}

impl SaveCharacterReq {
    pub fn decode(r: &mut PacketReader) -> Result<Self> {
        Ok(Self {
            session_id:   r.read_u64()?,
            character_id: r.read_u32()?,
            level:        r.read_u32()?,
            experience:   r.read_u64()?,
            hp:           r.read_u32()?,
            zone_id:      r.read_u32()?,
            pos_x:        r.read_f32()?,
            pos_y:        r.read_f32()?,
            pos_z:        r.read_f32()?,
            orientation:  r.read_f32()?,
        })
    }

    pub fn validate(&self) -> Result<()> {
        if self.level == 0 || self.level > 9999 {
            bail!("레벨 범위 오류: {}", self.level);
        }
        if self.hp == 0 {
            bail!("hp는 0 이상이어야 합니다");
        }
        Ok(())
    }
}
