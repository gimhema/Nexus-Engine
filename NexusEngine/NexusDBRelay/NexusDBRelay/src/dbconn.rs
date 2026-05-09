// ─────────────────────────────────────────────────────────────────────────────
// dbconn.rs — SQLite 연결 풀 + 스키마 + CRUD 함수
//
// 모든 DB 접근은 이 모듈에서만 수행. dbagent.rs 에서 직접 sqlx를 호출하지 않음.
// 파라미터 바인딩(.bind())을 통해 SQL 인젝션을 구조적으로 차단.
// ─────────────────────────────────────────────────────────────────────────────

use anyhow::Result;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    Row, SqlitePool,
};

// ─────────────────────────────────────────────────────────────────────────────
// 캐릭터 데이터 레코드 (DB → 메모리)
// ─────────────────────────────────────────────────────────────────────────────
#[derive(Debug)]
pub struct CharacterRecord {
    pub id:          u32,
    pub account_id:  u32,
    pub name:        String,
    pub level:       u32,
    pub experience:  u64,
    pub max_hp:      u32,
    pub hp:          u32,
    pub zone_id:     u32,
    pub pos_x:       f32,
    pub pos_y:       f32,
    pub pos_z:       f32,
    pub orientation: f32,
}

// ─────────────────────────────────────────────────────────────────────────────
// 연결 풀 초기화
// ─────────────────────────────────────────────────────────────────────────────
pub async fn init_pool(path: &str) -> Result<SqlitePool> {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        // WAL 모드: 동시 읽기·쓰기 성능 향상 (게임서버 다중 요청 대응)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(opts)
        .await?;

    init_schema(&pool).await?;
    log::info!("SQLite 연결 완료: {}", path);
    Ok(pool)
}

// ─────────────────────────────────────────────────────────────────────────────
// 스키마 초기화 (IF NOT EXISTS — 재시작 시 안전)
// ─────────────────────────────────────────────────────────────────────────────
async fn init_schema(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS accounts (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            account_name TEXT    UNIQUE NOT NULL,
            token        TEXT    NOT NULL,
            -- 프로덕션에서는 bcrypt/argon2 해시 저장 권장
            created_at   INTEGER NOT NULL DEFAULT (unixepoch())
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS characters (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            account_id  INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
            name        TEXT    UNIQUE NOT NULL,
            level       INTEGER NOT NULL DEFAULT 1,
            experience  INTEGER NOT NULL DEFAULT 0,
            max_hp      INTEGER NOT NULL DEFAULT 100,
            hp          INTEGER NOT NULL DEFAULT 100,
            zone_id     INTEGER NOT NULL DEFAULT 1,
            pos_x       REAL    NOT NULL DEFAULT 0.0,
            pos_y       REAL    NOT NULL DEFAULT 0.0,
            pos_z       REAL    NOT NULL DEFAULT 0.0,
            orientation REAL    NOT NULL DEFAULT 0.0,
            created_at  INTEGER NOT NULL DEFAULT (unixepoch())
        )",
    )
    .execute(pool)
    .await?;

    log::debug!("스키마 초기화 완료");
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// 계정 CRUD
// ─────────────────────────────────────────────────────────────────────────────

/// 계정이 없으면 생성, 있으면 토큰 검증 후 account_id 반환.
/// Phase 4: 프로덕션에서는 토큰 해시 비교로 교체 필요.
pub async fn check_or_create_account(
    pool: &SqlitePool,
    account_name: &str,
    token: &str,
) -> Result<u32> {
    // 기존 계정 조회
    let row = sqlx::query(
        "SELECT id, token FROM accounts WHERE account_name = ?",
    )
    .bind(account_name)
    .fetch_optional(pool)
    .await?;

    if let Some(row) = row {
        let stored_token: String = row.get("token");
        if stored_token != token {
            anyhow::bail!("토큰 불일치");
        }
        let id: i64 = row.get("id");
        return Ok(id as u32);
    }

    // 신규 계정 생성
    let result = sqlx::query(
        "INSERT INTO accounts (account_name, token) VALUES (?, ?)",
    )
    .bind(account_name)
    .bind(token)
    .execute(pool)
    .await?;

    log::info!("신규 계정 생성: {}", account_name);
    Ok(result.last_insert_rowid() as u32)
}

// ─────────────────────────────────────────────────────────────────────────────
// 캐릭터 CRUD
// ─────────────────────────────────────────────────────────────────────────────

/// 캐릭터 생성. 성공 시 새 character_id 반환.
pub async fn create_character(
    pool: &SqlitePool,
    account_id: u32,
    name: &str,
) -> Result<u32> {
    let result = sqlx::query(
        "INSERT INTO characters (account_id, name) VALUES (?, ?)",
    )
    .bind(account_id as i64)
    .bind(name)
    .execute(pool)
    .await?;

    log::info!("캐릭터 생성: account_id={} name={}", account_id, name);
    Ok(result.last_insert_rowid() as u32)
}

/// 캐릭터 전체 데이터 조회.
pub async fn load_character(
    pool: &SqlitePool,
    character_id: u32,
) -> Result<Option<CharacterRecord>> {
    let row = sqlx::query(
        "SELECT id, account_id, name, level, experience,
                max_hp, hp, zone_id, pos_x, pos_y, pos_z, orientation
         FROM characters WHERE id = ?",
    )
    .bind(character_id as i64)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| CharacterRecord {
        id:          r.get::<i64, _>("id")          as u32,
        account_id:  r.get::<i64, _>("account_id")  as u32,
        name:        r.get("name"),
        level:       r.get::<i64, _>("level")       as u32,
        experience:  r.get::<i64, _>("experience")  as u64,
        max_hp:      r.get::<i64, _>("max_hp")      as u32,
        hp:          r.get::<i64, _>("hp")          as u32,
        zone_id:     r.get::<i64, _>("zone_id")     as u32,
        pos_x:       r.get("pos_x"),
        pos_y:       r.get("pos_y"),
        pos_z:       r.get("pos_z"),
        orientation: r.get("orientation"),
    }))
}

/// 캐릭터 가변 상태 저장 (로그아웃·자동저장).
pub async fn save_character(
    pool: &SqlitePool,
    character_id: u32,
    level:       u32,
    experience:  u64,
    hp:          u32,
    zone_id:     u32,
    pos_x:       f32,
    pos_y:       f32,
    pos_z:       f32,
    orientation: f32,
) -> Result<()> {
    // max_hp는 레벨에서 파생. 기본 100 + (level-1)*10
    let max_hp = 100u32 + (level.saturating_sub(1)) * 10;

    sqlx::query(
        "UPDATE characters
         SET level=?, experience=?, max_hp=?, hp=?,
             zone_id=?, pos_x=?, pos_y=?, pos_z=?, orientation=?
         WHERE id=?",
    )
    .bind(level as i64)
    .bind(experience as i64)
    .bind(max_hp as i64)
    .bind(hp as i64)
    .bind(zone_id as i64)
    .bind(pos_x)
    .bind(pos_y)
    .bind(pos_z)
    .bind(orientation)
    .bind(character_id as i64)
    .execute(pool)
    .await?;

    log::debug!("캐릭터 저장 완료: id={}", character_id);
    Ok(())
}
