use anyhow::Result;

use super::reader::PacketReader;
use super::validate::validate_account_name;

/// ReqCheckAccount 페이로드
pub struct CheckAccountReq {
    pub session_id:   u64,
    pub account_name: String,
    pub token:        String,
}

impl CheckAccountReq {
    pub fn decode(r: &mut PacketReader) -> Result<Self> {
        Ok(Self {
            session_id:   r.read_u64()?,
            account_name: r.read_string()?,
            token:        r.read_string()?,
        })
    }

    pub fn validate(&self) -> Result<()> {
        validate_account_name(&self.account_name)
    }
}
