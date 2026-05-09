use sqlx::SqlitePool;

use crate::dbconn;
use crate::msg::{error_response, CheckAccountReq, DbOpcode, PacketReader, PacketWriter};

/// ReqCheckAccount 처리
/// 계정 확인 또는 신규 생성 후 account_id 반환
pub async fn on_check_account(r: &mut PacketReader<'_>, pool: &SqlitePool) -> Vec<u8> {
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
