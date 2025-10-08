// src/storage.rs
use anyhow::{Context, Result};
use chrono::Utc;
use redis::AsyncCommands;
use rusqlite::{Connection, Row, params};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, instrument};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Tick {
    pub ts: i64,
    pub symbol: String,
    pub price: f64,
    pub vol: f64,
}

#[derive(Debug)]
pub struct Storage {
    conn: Arc<Mutex<Connection>>,
    redis: redis::Client,
}

impl Storage {
    pub fn new(sqlite_path: &str, redis_url: &str) -> Result<Self> {
        info!(
            "Initializing storage with SQLite: {}, Redis: {}",
            sqlite_path, redis_url
        );

        let conn = Connection::open(sqlite_path)
            .with_context(|| format!("Failed to open SQLite database at {}", sqlite_path))?;

        // Configure SQLite for better performance
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA cache_size = -64000;  -- 64MB cache
            PRAGMA temp_store = memory;
            PRAGMA mmap_size = 268435456;  -- 256MB memory mapping
            "#,
        )?;

        // Create tables and indexes
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS ticks (
                ts INTEGER NOT NULL,
                symbol TEXT NOT NULL,
                price REAL,
                vol REAL,
                PRIMARY KEY (symbol, ts)
            ) WITHOUT ROWID;



            "#,
        )?;

        let redis_client = redis::Client::open(redis_url)
            .with_context(|| format!("Failed to connect to Redis at {}", redis_url))?;

        // Test Redis connection
        let mut test_conn = redis_client.get_connection()?;
        let _: () = redis::cmd("PING").query(&mut test_conn)?;

        info!("Storage initialized successfully");

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            redis: redis_client,
        })
    }

    #[instrument(skip(self, tick))]
    pub async fn save_tick(&self, tick: &Tick) -> Result<()> {
        debug!("Saving tick for symbol: {}", tick.symbol);

        let t = tick.clone();
        let conn = self.conn.clone();

        // Save to SQLite
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = conn.blocking_lock();
            conn.execute(
                "INSERT OR REPLACE INTO ticks (ts, symbol, price, vol) VALUES (?1, ?2, ?3, ?4)",
                params![t.ts, t.symbol, t.price, t.vol],
            )
            .with_context(|| format!("Failed to insert tick for symbol {}", t.symbol))?;
            Ok(())
        })
        .await?
        .context("Failed to execute SQLite operation")?;

        // Save to Redis
        let mut con = self
            .redis
            .get_async_connection()
            .await
            .context("Failed to get Redis connection")?;

        let key = format!("tick:{}", tick.symbol);
        let v = serde_json::to_string(tick).context("Failed to serialize tick to JSON")?;

        let _: () = con
            .set_ex(&key, v, 3600)
            .await // 1 hour TTL
            .with_context(|| format!("Failed to set Redis key {}", key))?;

        debug!("Tick saved successfully for symbol: {}", tick.symbol);
        Ok(())
    }

    #[instrument(skip(self))]
    pub async fn get_latest_tick(&self, symbol: &str) -> Result<Option<Tick>> {
        let mut con = self
            .redis
            .get_async_connection()
            .await
            .context("Failed to get Redis connection")?;

        let key = format!("tick:{}", symbol);
        let v: Option<String> = con
            .get(&key)
            .await
            .with_context(|| format!("Failed to get Redis key {}", key))?;

        match v {
            Some(s) => {
                let tick: Tick = serde_json::from_str(&s).with_context(|| {
                    format!("Failed to deserialize tick from JSON for symbol {}", symbol)
                })?;
                Ok(Some(tick))
            }
            None => {
                debug!(
                    "No tick found in Redis for symbol: {}, falling back to SQLite",
                    symbol
                );
                self.get_latest_tick_from_sqlite(symbol).await
            }
        }
    }

    #[instrument(skip(self))]
    async fn get_latest_tick_from_sqlite(&self, symbol: &str) -> Result<Option<Tick>> {
        let symbol = symbol.to_string();
        let conn = self.conn.clone();

        tokio::task::spawn_blocking(move || -> Result<Option<Tick>> {
            let conn = conn.blocking_lock();
            let mut stmt = conn.prepare(
                "SELECT ts, symbol, price, vol FROM ticks WHERE symbol = ?1 ORDER BY ts DESC LIMIT 1"
            )?;

            let mut rows = stmt.query_map(params![symbol], |r: &Row| {
                Ok(Tick {
                    ts: r.get(0)?,
                    symbol: r.get(1)?,
                    price: r.get(2)?,
                    vol: r.get(3)?,
                })
            })?;

            match rows.next() {
                Some(row) => Ok(Some(row?)),
                None => Ok(None),
            }
        })
        .await?
        .context("Failed to execute SQLite query")
    }

    #[instrument(skip(self))]
    pub async fn get_ticks_range(
        &self,
        symbol: &str,
        start_ts: i64,
        end_ts: i64,
    ) -> Result<Vec<Tick>> {
        let symbol_str = symbol.to_string();
        let conn = self.conn.clone();

        debug!(
            "Fetching ticks for symbol: {} from {} to {}",
            symbol, start_ts, end_ts
        );

        let rows: Vec<Tick> = tokio::task::spawn_blocking(move || -> Result<Vec<Tick>> {
            let conn = conn.blocking_lock();
            let mut stmt = conn.prepare(
                "SELECT ts, symbol, price, vol FROM ticks WHERE symbol = ?1 AND ts >= ?2 AND ts < ?3 ORDER BY ts ASC"
            )?;

            let rows_iter = stmt.query_map(params![symbol_str, start_ts, end_ts], |r: &Row| {
                Ok(Tick {
                    ts: r.get(0)?,
                    symbol: r.get(1)?,
                    price: r.get(2)?,
                    vol: r.get(3)?,
                })
            })?;

            let mut out = Vec::new();
            for r in rows_iter {
                out.push(r?);
            }
            Ok(out)
        })
        .await?
        .context("Failed to execute SQLite query")?;

        debug!("Retrieved {} ticks for symbol: {}", rows.len(), symbol);
        Ok(rows)
    }

    #[instrument(skip(self))]
    pub async fn get_ticks_recent_days(&self, symbol: &str, days: i64) -> Result<Vec<Tick>> {
        let end = Utc::now();
        let start = end - chrono::Duration::days(days);
        self.get_ticks_range(symbol, start.timestamp_millis(), end.timestamp_millis())
            .await
    }

    #[instrument(skip(self))]
    pub async fn get_ticks_for_date(&self, symbol: &str, date: &str) -> Result<Vec<Tick>> {
        let start_naive = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .with_context(|| format!("Failed to parse date: {}", date))?
            .and_hms_opt(0, 0, 0)
            .unwrap();

        let end_naive = start_naive + chrono::Duration::days(1);

        let start_ts =
            chrono::DateTime::<Utc>::from_naive_utc_and_offset(start_naive, Utc).timestamp_millis();
        let end_ts =
            chrono::DateTime::<Utc>::from_naive_utc_and_offset(end_naive, Utc).timestamp_millis();

        self.get_ticks_range(symbol, start_ts, end_ts).await
    }

    #[instrument(skip(self))]
    pub async fn get_symbols(&self) -> Result<Vec<String>> {
        let conn = self.conn.clone();

        tokio::task::spawn_blocking(move || -> Result<Vec<String>> {
            let conn = conn.blocking_lock();
            let mut stmt = conn.prepare("SELECT DISTINCT symbol FROM ticks ORDER BY symbol")?;

            let rows_iter = stmt.query_map([], |r: &Row| Ok(r.get(0)?))?;

            let mut symbols = Vec::new();
            for row in rows_iter {
                symbols.push(row?);
            }
            Ok(symbols)
        })
        .await?
        .context("Failed to execute SQLite query")
    }
}
