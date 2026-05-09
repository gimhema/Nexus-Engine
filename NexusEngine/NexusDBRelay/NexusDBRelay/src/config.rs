use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    #[serde(default)]
    pub relay: RelayConfig,
    #[serde(default)]
    pub database: DatabaseConfig,
}

#[derive(Deserialize)]
pub struct RelayConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self { host: default_host(), port: default_port() }
    }
}

fn default_host() -> String { "127.0.0.1".to_string() }
fn default_port() -> u16    { 7073 }

#[derive(Deserialize)]
pub struct DatabaseConfig {
    #[serde(default = "default_backend")]
    pub backend: String,
    #[serde(default = "default_sqlite_path")]
    pub sqlite_path: String,
    pub postgresql_url: Option<String>,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            backend:        default_backend(),
            sqlite_path:    default_sqlite_path(),
            postgresql_url: None,
        }
    }
}

fn default_backend()     -> String { "sqlite".to_string() }
fn default_sqlite_path() -> String { "./nexus_data.db".to_string() }

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let path = "config.toml";
        if std::path::Path::new(path).exists() {
            let raw = std::fs::read_to_string(path)?;
            Ok(toml::from_str(&raw)?)
        } else {
            log::info!("config.toml 없음 — 기본값으로 실행");
            Ok(Self { relay: RelayConfig::default(), database: DatabaseConfig::default() })
        }
    }
}
