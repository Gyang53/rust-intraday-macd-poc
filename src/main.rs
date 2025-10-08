// src/main.rs
mod app;
mod config;
mod error;
mod indicators;
mod storage;
mod web;

use anyhow::Result;
use app::TradingApp;
use chrono::{NaiveTime, Utc};
use clap::Parser;
use config::AppConfig;
use rand::Rng;
use std::sync::Arc;
use storage::{Storage, Tick};
use tokio::time::{Duration, sleep};

#[derive(Parser, Debug)]
struct CliConfig {
    #[arg(long, help = "Override default symbol")]
    symbol: Option<String>,

    #[arg(long, help = "Override SQLite database path")]
    sqlite: Option<String>,

    #[arg(long, help = "Override Redis URL")]
    redis: Option<String>,

    #[arg(long, help = "Override server host")]
    host: Option<String>,

    #[arg(long, help = "Override server port")]
    port: Option<u16>,

    /// generate a simulated full trading day into sqlite for testing (yesterday)
    #[arg(long, default_value_t = false)]
    gen_sim: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Parse CLI arguments
    let cli_config = CliConfig::parse();

    // Load application configuration
    let mut app_config = AppConfig::new()?;

    // Override config with CLI values if provided
    if let Some(symbol) = cli_config.symbol {
        app_config.trading.default_symbol = symbol;
    }
    if let Some(sqlite) = cli_config.sqlite {
        app_config.database.sqlite_path = sqlite;
    }
    if let Some(redis) = cli_config.redis {
        app_config.database.redis_url = redis;
    }
    if let Some(host) = cli_config.host {
        app_config.server.host = host;
    }
    if let Some(port) = cli_config.port {
        app_config.server.port = port;
    }

    tracing::info!(
        "Starting {} v{} in {} mode",
        app_config.name,
        app_config.version,
        app_config.environment
    );

    let storage = Arc::new(Storage::new(
        &app_config.database.sqlite_path,
        &app_config.database.redis_url,
    )?);

    let trading_app = Arc::new(TradingApp::new(
        storage.clone(),
        Arc::new(app_config.clone()),
    ));

    // Optionally populate one full day of simulated minute data (useful on non-trading days)
    if cli_config.gen_sim {
        generate_and_store_mock_day(&storage, &app_config.trading.default_symbol).await?;
        tracing::info!(
            "Generated simulated day for {}",
            app_config.trading.default_symbol
        );
    }

    // Start web server
    web::start_web(trading_app, &app_config.server.host, app_config.server.port)
        .await
        .unwrap();

    // Keep main alive. In production your strategy loop would run here.
    let server_address = app_config.get_server_address();
    tracing::info!("Service running. Open http://{}/", server_address);

    loop {
        sleep(Duration::from_secs(60)).await;
    }
}

/// generate a mock full trading day minute-level data (09:30-11:30 and 13:00-15:00) for yesterday
async fn generate_and_store_mock_day(storage: &Arc<Storage>, symbol: &str) -> Result<()> {
    // pick date = yesterday
    let today = chrono::Local::now().date_naive();
    let date = today - chrono::Duration::days(1);

    // trading sessions:
    let morning_start = NaiveTime::from_hms_opt(9, 30, 0).unwrap();
    let morning_end = NaiveTime::from_hms_opt(11, 30, 0).unwrap();
    let afternoon_start = NaiveTime::from_hms_opt(13, 0, 0).unwrap();
    let afternoon_end = NaiveTime::from_hms_opt(15, 0, 0).unwrap();

    let mut rng = rand::thread_rng();
    // base price
    let mut price = 10.0 + rng.gen_range(-0.5..0.5);

    let mut push = |dt: chrono::NaiveDateTime| -> Tick {
        // random walk small moves
        price = f64::max(price + rng.gen_range(-0.2..0.2), 0.01);
        Tick {
            ts: chrono::DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc).timestamp_millis(),
            symbol: symbol.to_string(),
            price,
            vol: (rng.gen_range(100..2000)) as f64,
        }
    };

    // morning minutes
    let mut t = chrono::NaiveDateTime::new(date, morning_start);
    while t.time() <= morning_end {
        let tick = push(t);
        storage.save_tick(&tick).await?;
        t = t + chrono::Duration::minutes(1);
    }

    // afternoon minutes
    let mut t2 = chrono::NaiveDateTime::new(date, afternoon_start);
    while t2.time() <= afternoon_end {
        let tick = push(t2);
        storage.save_tick(&tick).await?;
        t2 = t2 + chrono::Duration::minutes(1);
    }

    Ok(())
}
