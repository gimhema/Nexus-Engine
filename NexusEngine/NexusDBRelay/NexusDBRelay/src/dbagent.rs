// ─────────────────────────────────────────────────────────────────────────────
// dbagent.rs — RPC 핸들러 (게임서버의 프로시저 호출을 처리)
//
// 역할: 패킷 디코딩 → 입력 검증 → DB 작업 위임 → 응답 패킷 인코딩
// DB 접근은 dbconn 모듈에만 위임. 이 파일에 sqlx 코드 없음.
// ─────────────────────────────────────────────────────────────────────────────

use sqlx::SqlitePool;

use crate::dbconn;
use crate::dbpackets::{
    error_response, CheckAccountReq, CreateCharacterReq, DbOpcode, LoadCharacterReq,
    PacketReader, PacketWriter, SaveCharacterReq,
};

// ─────────────────────────────────────────────────────────────────────────────
// 메인 디스패처
// ─────────────────────────────────────────────────────────────────────────────
pub async fn handle_packet(opcode: u16, payload: &[u8], pool: &SqlitePool) -> Vec<u8> {
    let mut r = PacketReader::new(payload);

    match DbOpcode::from_u16(opcode) {
        Some(DbOpcode::ReqCheckAccount)    => on_check_account(&mut r, pool).await,
        Some(DbOpcode::ReqCreateCharacter) => on_create_character(&mut r, pool).await,
        Some(DbOpcode::ReqLoadCharacter)   => on_load_character(&mut r, pool).await,
        Some(DbOpcode::ReqSaveCharacter)   => on_save_character(&mut r, pool).await,
        _ => {
            log::warn!("알 수 없는 opcode: {:#06x}", opcode);
            // session_id 없이 받은 요청이므로 0으로 처리
            error_response(opcode.wrapping_add(1), 0, "알 수 없는 요청")
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ReqCheckAccount → ResCheckAccount
// 계정 확인 또는 신규 생성 후 account_id 반환
// ─────────────────────────────────────────────────────────────────────────────
async fn on_check_account(r: &mut PacketReader<'_>, pool: &SqlitePool) -> Vec<u8> {
    let res = DbOpcode::ResCheckAccount as u16;

    let req = match CheckAccountReq::decode(r) {
        Ok(v) => v,
        Err(e) => return error_response(res, 0, &e.to_string()),
    };
    if let Err(e) = req.validate() {
        return error_response(res, req.session_id, &e.to_string());
    }

    match dbconn::check_or_create_account(pool, &req.account_name, &req.token).await {
        Ok(account_id) => {
            log::info!("계정 확인: account={} id={}", req.account_name, account_id);
            PacketWriter::new(res)
                .write_u8(1)
                .write_u64(req.session_id)
                .write_u32(account_id)
                .write_string("OK")
                .finalize()
        }
        Err(e) => {
            log::warn!("계정 확인 실패: account={} err={}", req.account_name, e);
            error_response(res, req.session_id, &e.to_string())
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ReqCreateCharacter → ResCreateCharacter
// ─────────────────────────────────────────────────────────────────────────────
async fn on_create_character(r: &mut PacketReader<'_>, pool: &SqlitePool) -> Vec<u8> {
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
            // 중복 이름은 UNIQUE 제약 위반 → 클라이언트에 명확한 메시지 전달
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

// ─────────────────────────────────────────────────────────────────────────────
// ReqLoadCharacter → ResLoadCharacter
// ─────────────────────────────────────────────────────────────────────────────
async fn on_load_character(r: &mut PacketReader<'_>, pool: &SqlitePool) -> Vec<u8> {
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

// ─────────────────────────────────────────────────────────────────────────────
// ReqSaveCharacter → ResSaveCharacter
// ─────────────────────────────────────────────────────────────────────────────
async fn on_save_character(r: &mut PacketReader<'_>, pool: &SqlitePool) -> Vec<u8> {
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
