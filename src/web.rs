// src/web.rs
use crate::indicators::{MACDPoint, compute_macd_series};
use crate::storage::{Storage, Tick, Trade};
use actix_web::{App, HttpResponse, HttpServer, Responder, get, post, web};
use anyhow::Result;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, PartialEq, Copy)]
pub enum RunMode {
    Sim,
    Real,
}

#[derive(Clone)]
pub struct AppState {
    pub mode: Arc<RwLock<RunMode>>,
    pub storage: Arc<Storage>,
}

#[derive(Serialize)]
struct HistoryResponse {
    points: Vec<MACDPoint>,
}

#[post("/api/set_mode/{mode}")]
async fn set_mode(state: web::Data<AppState>, path: web::Path<String>) -> impl Responder {
    let m = path.into_inner();
    let new_mode = match m.as_str() {
        "real" => RunMode::Real,
        _ => RunMode::Sim,
    };
    {
        let mut lock = state.mode.write().await;
        *lock = new_mode;
    }
    HttpResponse::Ok().body(format!("mode set to {:?}", new_mode))
}

#[get("/api/get_mode")]
async fn get_mode(state: web::Data<AppState>) -> impl Responder {
    let lock = state.mode.read().await;
    let s = match *lock {
        RunMode::Real => "real",
        RunMode::Sim => "sim",
    };
    HttpResponse::Ok().body(s)
}

/// /api/latest/{symbol}
#[get("/api/latest/{symbol}")]
async fn latest(state: web::Data<AppState>, path: web::Path<String>) -> impl Responder {
    let symbol = path.into_inner();
    match state.storage.get_latest_tick(&symbol).await {
        Ok(Some(t)) => HttpResponse::Ok().json(t),
        Ok(None) => HttpResponse::NotFound().body("no tick"),
        Err(e) => HttpResponse::InternalServerError().body(format!("err: {:?}", e)),
    }
}

/// /api/trades/{symbol}
#[get("/api/trades/{symbol}")]
async fn trades(state: web::Data<AppState>, path: web::Path<String>) -> impl Responder {
    let symbol = path.into_inner();
    match state.storage.get_latest_trade(&symbol).await {
        Ok(Some(t)) => HttpResponse::Ok().json(t),
        Ok(None) => HttpResponse::NotFound().body("no trade"),
        Err(e) => HttpResponse::InternalServerError().body(format!("err: {:?}", e)),
    }
}

/// /api/history/{symbol}?date=YYYY-MM-DD  (for sim mode: date optional -> else most recent day in DB)
/// If mode==real -> returns recent 30 days (daily aggregated), else returns full-day minute series for date
#[get("/api/history/{symbol}")]
async fn history(
    state: web::Data<AppState>,
    path: web::Path<String>,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> impl Responder {
    let symbol = path.into_inner();
    let mode = { state.mode.read().await.clone() };

    let points_res: Result<Vec<(i64, f64)>, anyhow::Error> = (|| async {
        match mode {
            RunMode::Real => {
                // read last 30 days of ticks from sqlite, aggregate by day (take close of day)
                let ticks = state.storage.get_ticks_recent_days(&symbol, 30).await?;
                // group by date -> close price at last ts
                use std::collections::BTreeMap;
                let mut by_date: BTreeMap<String, (i64, f64)> = BTreeMap::new();
                for t in ticks {
                    let dt = chrono::NaiveDateTime::from_timestamp_opt(t.ts / 1000, 0)
                        .unwrap_or_else(|| chrono::NaiveDateTime::from_timestamp(0, 0));
                    let date_str = dt.date().format("%Y-%m-%d").to_string();
                    // keep last (largest ts)
                    match by_date.get(&date_str) {
                        Some((prev_ts, _)) if *prev_ts > t.ts => {}
                        _ => {
                            by_date.insert(date_str, (t.ts, t.price));
                        }
                    }
                }
                let mut out = Vec::new();
                for (_d, (ts, price)) in by_date {
                    out.push((ts, price));
                }
                Ok(out)
            }
            RunMode::Sim => {
                // if query.date provided use it; else try to use most recent date in DB
                if let Some(date) = query.get("date") {
                    let ticks = state.storage.get_ticks_for_date(&symbol, date).await?;
                    let out = ticks.into_iter().map(|t| (t.ts, t.price)).collect();
                    Ok(out)
                } else {
                    // fallback: return last full day present in DB (we look for last day's date)
                    let recent = state.storage.get_ticks_recent_days(&symbol, 7).await?;
                    if recent.is_empty() {
                        return Ok(vec![]);
                    }
                    // find last date string
                    let last_ts = recent.last().unwrap().ts;
                    let naivedt = chrono::NaiveDateTime::from_timestamp_opt(last_ts / 1000, 0)
                        .unwrap_or_else(|| chrono::NaiveDateTime::from_timestamp(0, 0));
                    let date_str = naivedt.date().format("%Y-%m-%d").to_string();
                    let ticks = state.storage.get_ticks_for_date(&symbol, &date_str).await?;
                    Ok(ticks.into_iter().map(|t| (t.ts, t.price)).collect())
                }
            }
        }
    })()
    .await;

    match points_res {
        Ok(points) => {
            // compute macd
            let macd_points = compute_macd_series(&points);
            let resp = HistoryResponse {
                points: macd_points,
            };
            HttpResponse::Ok().json(resp)
        }
        Err(e) => HttpResponse::InternalServerError().body(format!("err: {:?}", e)),
    }
}

pub async fn start_web(storage: Arc<Storage>, host: &str, port: u16) -> std::io::Result<()> {
    let mode = Arc::new(RwLock::new(RunMode::Sim)); // default Sim
    let state = AppState { mode, storage };

    println!("Starting web server at {}:{}", host, port);
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(state.clone()))
            .service(set_mode)
            .service(get_mode)
            .service(latest)
            .service(trades)
            .service(history)
            .service(actix_files::Files::new("/", "./static").index_file("index.html"))
    })
    .bind((host, port))?
    .run()
    .await
}
