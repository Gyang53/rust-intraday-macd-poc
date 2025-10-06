// src/main.rs
mod indicators;
mod storage;
mod web;

use anyhow::Result;
use chrono::{NaiveTime, Utc};
use clap::Parser;
use rand::Rng;
use std::sync::Arc;
use storage::{Storage, Tick};
use tokio::time::{Duration, sleep};

#[derive(Parser, Debug)]
struct Config {
    #[arg(long, default_value = "600733.SH")]
    symbol: String,

    #[arg(long, default_value = "trading.db")]
    sqlite: String,

    #[arg(long, default_value = "redis://127.0.0.1/")]
    redis: String,

    #[arg(long, default_value = "0.0.0.0")]
    host: String,

    #[arg(long, default_value_t = 8080)]
    port: u16,

    /// generate a simulated full trading day into sqlite for testing (yesterday)
    #[arg(long, default_value_t = false)]
    gen_sim: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = Config::parse();

    let storage = Arc::new(Storage::new(&cfg.sqlite, &cfg.redis)?);

    // Optionally populate one full day of simulated minute data (useful on non-trading days)
    if cfg.gen_sim {
        generate_and_store_mock_day(&storage, &cfg.symbol).await?;
        println!("Generated simulated day for {}", cfg.symbol);
    }

    // start web server directly (not spawned due to Send trait issues with Actix-web)
    let web_storage = storage.clone();
    web::start_web(web_storage, &cfg.host, cfg.port)
        .await
        .unwrap();

    // Keep main alive. In production your strategy loop would run here.
    let host = cfg.host.clone();
    println!("Service running. Open http://{}:{}/", host, cfg.port);
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
