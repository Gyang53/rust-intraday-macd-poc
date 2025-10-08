// src/config.rs
use config::{Config, ConfigError, File, FileFormat};
use serde::Deserialize;
use std::env;

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub sqlite_path: String,
    pub redis_url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TradingConfig {
    pub default_symbol: String,
    pub macd_short: usize,
    pub macd_long: usize,
    pub macd_signal: usize,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub name: String,
    pub version: String,
    pub environment: String,
    pub database: DatabaseConfig,
    pub server: ServerConfig,
    pub trading: TradingConfig,
}

impl AppConfig {
    pub fn new() -> Result<Self, ConfigError> {
        let run_mode = env::var("RUN_MODE").unwrap_or_else(|_| "development".into());

        let config = Config::builder()
            .add_source(File::new("config/default.toml", FileFormat::Toml))
            .add_source(
                File::new(&format!("config/{}.toml", run_mode), FileFormat::Toml).required(false),
            )
            .add_source(config::Environment::with_prefix("APP"))
            .build()?;

        config.try_deserialize()
    }

    pub fn get_server_address(&self) -> String {
        format!("{}:{}", self.server.host, self.server.port)
    }
}
