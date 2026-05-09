use anyhow::{bail, Result};

/// 캐릭터명: 2~20자, 영숫자 + 언더스코어만 허용
pub fn validate_character_name(name: &str) -> Result<()> {
    if name.len() < 2 || name.len() > 20 {
        bail!("캐릭터명 길이 오류 (2~20자): {}", name.len());
    }
    if !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        bail!("캐릭터명 허용 문자: 영숫자, 언더스코어");
    }
    Ok(())
}

/// 계정명: 1~64자
pub fn validate_account_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 64 {
        bail!("계정명 길이 오류 (1~64자): {}", name.len());
    }
    Ok(())
}
