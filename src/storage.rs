// src/storage.rs
use anyhow::Result;
use chrono::{NaiveDateTime, Utc};
use redis::AsyncCommands;
use rusqlite::{Connection, Row, params};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Tick {
    pub ts: i64,
    pub symbol: String,
    pub price: f64,
    pub vol: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Trade {
    pub ts: i64,
    pub symbol: String,
    pub side: String,
    pub price: f64,
    pub amount: f64,
    pub order_id: String,
}

pub struct Storage {
    // We will run blocking sqlite ops in spawn_blocking; protect conn via Mutex for safety
    conn: Arc<Mutex<Connection>>,
    redis: redis::Client,
}

impl Storage {
    pub fn new(sqlite_path: &str, redis_url: &str) -> Result<Self> {
        let conn = Connection::open(sqlite_path)?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            CREATE TABLE IF NOT EXISTS ticks (
                ts INTEGER NOT NULL,
                symbol TEXT NOT NULL,
                price REAL,
                vol REAL
            );
            CREATE INDEX IF NOT EXISTS idx_ticks_symbol_ts ON ticks(symbol, ts);

            CREATE TABLE IF NOT EXISTS trades (
                ts INTEGER NOT NULL,
                symbol TEXT NOT NULL,
                side TEXT,
                price REAL,
                amount REAL,
                order_id TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_trades_symbol_ts ON trades(symbol, ts);
            "#,
        )?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            redis: redis::Client::open(redis_url)?,
        })
    }

    /// Save tick into sqlite (blocking inside spawn_blocking) and set latest tick in redis.
    pub async fn save_tick(&self, tick: &Tick) -> Result<()> {
        let t = tick.clone();
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = conn.blocking_lock();
            conn.execute(
                "INSERT INTO ticks (ts, symbol, price, vol) VALUES (?1, ?2, ?3, ?4)",
                params![t.ts, t.symbol, t.price, t.vol],
            )?;
            Ok(())
        })
        .await??;

        let mut con = self.redis.get_async_connection().await?;
        let key = format!("tick:{}", tick.symbol);
        let v = serde_json::to_string(tick)?;
        let _: () = con.set(key, v).await?;
        Ok(())
    }

    pub async fn save_trade(&self, trade: &Trade) -> Result<()> {
        let tr = trade.clone();
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = conn.blocking_lock();
            conn.execute(
                "INSERT INTO trades (ts, symbol, side, price, amount, order_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![tr.ts, tr.symbol, tr.side, tr.price, tr.amount, tr.order_id],
            )?;
            Ok(())
        }).await??;

        let mut con = self.redis.get_async_connection().await?;
        let key = format!("trade:{}", trade.symbol);
        let v = serde_json::to_string(trade)?;
        let _: () = con.set(key, v).await?;
        Ok(())
    }

    pub async fn get_latest_tick(&self, symbol: &str) -> Result<Option<Tick>> {
        let mut con = self.redis.get_async_connection().await?;
        let key = format!("tick:{}", symbol);
        let v: Option<String> = con.get(&key).await?;
        Ok(v.map(|s| serde_json::from_str(&s).unwrap()))
    }

    pub async fn get_latest_trade(&self, symbol: &str) -> Result<Option<Trade>> {
        let mut con = self.redis.get_async_connection().await?;
        let key = format!("trade:{}", symbol);
        let v: Option<String> = con.get(&key).await?;
        Ok(v.map(|s| serde_json::from_str(&s).unwrap()))
    }

    /// Get ticks in [start_ts, end_ts) ordered ascending
    pub async fn get_ticks_range(
        &self,
        symbol: &str,
        start_ts: i64,
        end_ts: i64,
    ) -> Result<Vec<Tick>> {
        let symbol = symbol.to_string();
        let conn = self.conn.clone();
        let rows: Vec<Tick> = tokio::task::spawn_blocking(move || -> Result<Vec<Tick>> {
            let conn = conn.blocking_lock();
            let mut stmt = conn.prepare("SELECT ts, symbol, price, vol FROM ticks WHERE symbol = ?1 AND ts >= ?2 AND ts < ?3 ORDER BY ts ASC")?;
            let rows_iter = stmt.query_map(params![symbol, start_ts, end_ts], |r: &Row| {
                Ok(Tick {
                    ts: r.get(0)?,
                    symbol: r.get(1)?,
                    price: r.get(2)?,
                    vol: r.get(3)?,
                })
            })?;
            let mut out = Vec::new();
            for r in rows_iter { out.push(r?); }
            Ok(out)
        }).await??;
        Ok(rows)
    }

    /// Get ticks for the most recent N days (based on ts)
    pub async fn get_ticks_recent_days(&self, symbol: &str, days: i64) -> Result<Vec<Tick>> {
        // compute start_ts from now - days
        let end = Utc::now();
        let start = end - chrono::Duration::days(days);
        self.get_ticks_range(symbol, start.timestamp_millis(), end.timestamp_millis())
            .await
    }

    /// Get ticks for a specific date (local date string "YYYY-MM-DD")
    pub async fn get_ticks_for_date(&self, symbol: &str, date: &str) -> Result<Vec<Tick>> {
        // date parse to start and end (local)
        let start_naive = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")?
            .and_hms_opt(0, 0, 0)
            .unwrap();
        let end_naive = start_naive + chrono::Duration::days(1);
        // treat as local -> convert to UTC timestamps (assume local is system local)
        let start_ts = chrono::DateTime::<Utc>::from_utc(start_naive, Utc).timestamp_millis();
        let end_ts = chrono::DateTime::<Utc>::from_utc(end_naive, Utc).timestamp_millis();
        self.get_ticks_range(symbol, start_ts, end_ts).await
    }
}
