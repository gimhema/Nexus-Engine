use sqlx::SqlitePool;

use crate::dbconn;
use crate::msg::{
    error_response, CreateCharacterReq, DbOpcode, LoadCharacterReq, PacketReader, PacketWriter,
    SaveCharacterReq,
};

/// ReqCreateCharacter 처리
pub async fn on_create_character(r: &mut PacketReader<'_>, pool: &SqlitePool) -> Vec<u8> {
    let res = DbOpcode::ResCreateCharacter as u16;

    let req = match CreateCharacterReq::decode(r) {
        Ok(v) => v,
        Err(e) => return error_response(res, 0, &e.to_string()),
    };
    if let Err(e) = req.validate() {
        return error_response(res, req.session_id, &e.to_string());
    }

    match dbconn::create_character(pool, req.account_id, &req.character_name).await {
        Ok(char_id) => {
            log::info!(
                "캐릭터 생성: name={} id={} account={}",
                req.character_name, char_id, req.account_id
            );
            PacketWriter::new(res)
                .write_u8(1)
                .write_u64(req.session_id)
                .write_u32(char_id)
                .write_string("OK")
                .finalize()
        }
        Err(e) => {
            let msg = if e.to_string().contains("UNIQUE") {
                "이미 사용 중인 캐릭터명입니다"
            } else {
                "캐릭터 생성 실패"
            };
            log::warn!("캐릭터 생성 실패: name={} err={}", req.character_name, e);
            error_response(res, req.session_id, msg)
        }
    }
}

/// ReqLoadCharacter 처리
pub async fn on_load_character(r: &mut PacketReader<'_>, pool: &SqlitePool) -> Vec<u8> {
    let res = DbOpcode::ResLoadCharacter as u16;

    let req = match LoadCharacterReq::decode(r) {
        Ok(v) => v,
        Err(e) => return error_response(res, 0, &e.to_string()),
    };

    match dbconn::load_character(pool, req.character_id).await {
        Ok(Some(c)) => {
            log::debug!("캐릭터 로드: id={} name={}", c.id, c.name);
            PacketWriter::new(res)
                .write_u8(1)
                .write_u64(req.session_id)
                .write_u32(c.id)
                .write_u32(c.account_id)
                .write_string(&c.name)
                .write_u32(c.level)
                .write_u64(c.experience)
                .write_u32(c.max_hp)
                .write_u32(c.hp)
                .write_u32(c.zone_id)
                .write_f32(c.pos_x)
                .write_f32(c.pos_y)
                .write_f32(c.pos_z)
                .write_f32(c.orientation)
                .finalize()
        }
        Ok(None) => error_response(res, req.session_id, "캐릭터 없음"),
        Err(e) => {
            log::error!("캐릭터 로드 실패: id={} err={}", req.character_id, e);
            error_response(res, req.session_id, "내부 오류")
        }
    }
}

/// ReqSaveCharacter 처리
pub async fn on_save_character(r: &mut PacketReader<'_>, pool: &SqlitePool) -> Vec<u8> {
    let res = DbOpcode::ResSaveCharacter as u16;

    let req = match SaveCharacterReq::decode(r) {
        Ok(v) => v,
        Err(e) => return error_response(res, 0, &e.to_string()),
    };
    if let Err(e) = req.validate() {
        return error_response(res, req.session_id, &e.to_string());
    }

    match dbconn::save_character(
        pool,
        req.character_id,
        req.level,
        req.experience,
        req.hp,
        req.zone_id,
        req.pos_x,
        req.pos_y,
        req.pos_z,
        req.orientation,
    )
    .await
    {
        Ok(()) => PacketWriter::new(res)
            .write_u8(1)
            .write_u64(req.session_id)
            .write_string("OK")
            .finalize(),
        Err(e) => {
            log::error!("캐릭터 저장 실패: id={} err={}", req.character_id, e);
            error_response(res, req.session_id, "저장 실패")
        }
    }
}
