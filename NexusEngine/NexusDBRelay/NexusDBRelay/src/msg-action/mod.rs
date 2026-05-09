mod auth;
mod character;

use sqlx::SqlitePool;

use crate::msg::{DbOpcode, PacketReader, error_response};

/// opcode → 핸들러 디스패치
pub async fn handle_packet(opcode: u16, payload: &[u8], pool: &SqlitePool) -> Vec<u8> {
    let mut r = PacketReader::new(payload);

    match DbOpcode::from_u16(opcode) {
        Some(DbOpcode::ReqCheckAccount)    => auth::on_check_account(&mut r, pool).await,
        Some(DbOpcode::ReqCreateCharacter) => character::on_create_character(&mut r, pool).await,
        Some(DbOpcode::ReqLoadCharacter)   => character::on_load_character(&mut r, pool).await,
        Some(DbOpcode::ReqSaveCharacter)   => character::on_save_character(&mut r, pool).await,
        _ => {
            log::warn!("알 수 없는 opcode: {:#06x}", opcode);
            error_response(opcode.wrapping_add(1), 0, "알 수 없는 요청")
        }
    }
}
