mod config;
mod dbagent;
mod dbconn;
mod dbpackets;

use anyhow::Result;
use sqlx::SqlitePool;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[tokio::main]
async fn main() -> Result<()> {
    // RUST_LOG 미설정 시 기본 info 레벨 — Rust 2024에서 set_var는 unsafe
    if std::env::var("RUST_LOG").is_err() {
        unsafe { std::env::set_var("RUST_LOG", "info") };
    }
    env_logger::init();

    let cfg = config::Config::load()?;

    // DB 연결 풀 초기화 (스키마 자동 생성 포함)
    let pool = match cfg.database.backend.as_str() {
        "sqlite" => dbconn::init_pool(&cfg.database.sqlite_path).await?,
        other => {
            anyhow::bail!("지원하지 않는 DB 백엔드: '{}' (현재: sqlite만 지원)", other);
        }
    };

    let addr = format!("{}:{}", cfg.relay.host, cfg.relay.port);
    let listener = TcpListener::bind(&addr).await?;
    log::info!("NexusDBRelay 시작 — {}", addr);

    loop {
        let (stream, peer) = listener.accept().await?;
        log::info!("게임서버 연결: {}", peer);

        let pool = pool.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, pool).await {
                log::info!("연결 종료 ({}): {}", peer, e);
            }
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// TCP 연결 1개 처리 루프
//
// 패킷 포맷: [u16 total_size][u16 opcode][payload...]
// total_size는 헤더(4바이트) 포함.
// ─────────────────────────────────────────────────────────────────────────────
async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    pool: SqlitePool,
) -> Result<()> {
    let mut header = [0u8; 4];

    loop {
        stream.read_exact(&mut header).await?;

        let total_size = u16::from_le_bytes([header[0], header[1]]) as usize;
        let opcode     = u16::from_le_bytes([header[2], header[3]]);

        // total_size가 헤더보다 작으면 잘못된 패킷
        let payload_size = total_size.saturating_sub(4);
        if payload_size > 65535 {
            anyhow::bail!("비정상 패킷 크기: {}", total_size);
        }

        let mut payload = vec![0u8; payload_size];
        if payload_size > 0 {
            stream.read_exact(&mut payload).await?;
        }

        log::debug!("수신: opcode={:#06x} size={}", opcode, total_size);

        let response = dbagent::handle_packet(opcode, &payload, &pool).await;
        stream.write_all(&response).await?;
    }
}
