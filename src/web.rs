// src/web.rs
use crate::app::TradingApp;
use crate::config::AppConfig;
use crate::indicators::{MACDPoint, compute_macd_series};
use actix_web::{App, HttpResponse, HttpServer, Responder, get, post, web};
use anyhow::{Context, Result};
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, instrument};

#[derive(Debug, Clone, PartialEq, Copy, Serialize)]
pub enum RunMode {
    Sim,
    Real,
}

impl std::fmt::Display for RunMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunMode::Sim => write!(f, "sim"),
            RunMode::Real => write!(f, "real"),
        }
    }
}

impl std::str::FromStr for RunMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "sim" => Ok(RunMode::Sim),
            "real" => Ok(RunMode::Real),
            _ => Err(format!("Invalid run mode: {}", s)),
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub mode: Arc<RwLock<RunMode>>,
    pub trading_app: Arc<TradingApp>,
    pub config: Arc<AppConfig>,
}

#[derive(Serialize)]
struct ApiResponse<T> {
    success: bool,
    data: Option<T>,
    error: Option<String>,
}

impl<T> ApiResponse<T> {
    fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    fn error(message: String) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message),
        }
    }
}

#[derive(Serialize)]
struct HistoryResponse {
    points: Vec<MACDPoint>,
    symbol: String,
    mode: String,
    count: usize,
}

#[derive(Serialize)]
struct ModeResponse {
    mode: String,
}

#[derive(Serialize)]
struct StatusResponse {
    status: String,
    version: String,
    mode: String,
    symbol_count: usize,
}

fn handle_error<E: std::fmt::Display>(err: E) -> HttpResponse {
    error!("API error: {}", err);
    HttpResponse::InternalServerError().json(ApiResponse::<()>::error(err.to_string()))
}

#[post("/api/set_mode/{mode}")]
#[instrument(skip(state))]
async fn set_mode(state: web::Data<AppState>, path: web::Path<String>) -> impl Responder {
    let mode_str = path.into_inner();

    match mode_str.parse::<RunMode>() {
        Ok(new_mode) => {
            {
                let mut lock = state.mode.write().await;
                *lock = new_mode;
            }

            info!("Run mode changed to: {}", new_mode);
            HttpResponse::Ok().json(ApiResponse::success(ModeResponse {
                mode: new_mode.to_string(),
            }))
        }
        Err(e) => {
            error!("Invalid mode requested: {}", mode_str);
            HttpResponse::BadRequest().json(ApiResponse::<()>::error(e))
        }
    }
}

#[get("/api/get_mode")]
#[instrument(skip(state))]
async fn get_mode(state: web::Data<AppState>) -> impl Responder {
    let mode = { state.mode.read().await.clone() };

    HttpResponse::Ok().json(ApiResponse::success(ModeResponse {
        mode: mode.to_string(),
    }))
}

#[get("/api/status")]
#[instrument(skip(state))]
async fn get_status(state: web::Data<AppState>) -> impl Responder {
    let mode = { state.mode.read().await.clone() };

    let symbol_count = match state.trading_app.get_storage().get_symbols().await {
        Ok(symbols) => symbols.len(),
        Err(e) => {
            error!("Failed to get symbols count: {}", e);
            return handle_error(e);
        }
    };

    HttpResponse::Ok().json(ApiResponse::success(StatusResponse {
        status: "running".to_string(),
        version: state.config.version.clone(),
        mode: mode.to_string(),
        symbol_count,
    }))
}

#[get("/api/latest/{symbol}")]
#[instrument(skip(state))]
async fn latest(state: web::Data<AppState>, path: web::Path<String>) -> impl Responder {
    let symbol = path.into_inner();

    match state.trading_app.get_symbol_info(&symbol).await {
        Ok(info) => {
            debug!("Retrieved symbol info for: {}", symbol);
            HttpResponse::Ok().json(ApiResponse::success(info))
        }
        Err(e) => handle_error(e),
    }
}

#[get("/api/symbols")]
#[instrument(skip(state))]
async fn get_symbols(state: web::Data<AppState>) -> impl Responder {
    match state.trading_app.get_all_symbols_info().await {
        Ok(symbols_info) => {
            debug!("Retrieved info for {} symbols", symbols_info.len());
            HttpResponse::Ok().json(ApiResponse::success(symbols_info))
        }
        Err(e) => handle_error(e),
    }
}

#[get("/api/history/{symbol}")]
#[instrument(skip(state, query))]
async fn history(
    state: web::Data<AppState>,
    path: web::Path<String>,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> impl Responder {
    let symbol = path.into_inner();
    let mode = { state.mode.read().await.clone() };

    let points_res: Result<Vec<(i64, f64)>> = async {
        match mode {
            RunMode::Real => {
                debug!("Fetching real mode history for symbol: {}", symbol);
                let analysis = state
                    .trading_app
                    .get_market_analysis(&symbol, Some(30))
                    .await
                    .context("Failed to fetch market analysis")?;

                let price_points: Vec<(i64, f64)> = analysis
                    .macd_points
                    .iter()
                    .map(|point| (point.ts, point.price))
                    .collect();
                Ok(price_points)
            }
            RunMode::Sim => {
                debug!("Fetching sim mode history for symbol: {}", symbol);
                if let Some(date) = query.get("date") {
                    let ticks = state
                        .trading_app
                        .get_storage()
                        .get_ticks_for_date(&symbol, date)
                        .await
                        .context("Failed to fetch ticks for date")?;

                    Ok(ticks.iter().map(|t| (t.ts, t.price)).collect())
                } else {
                    // Fallback: return last full day present in DB
                    let recent = state
                        .trading_app
                        .get_storage()
                        .get_ticks_recent_days(&symbol, 7)
                        .await
                        .context("Failed to fetch recent ticks")?;

                    if recent.is_empty() {
                        return Ok(vec![]);
                    }

                    // Find last date string
                    let last_ts = recent.last().unwrap().ts;
                    let naivedt = chrono::DateTime::from_timestamp(last_ts / 1000, 0)
                        .map(|dt| dt.naive_utc())
                        .unwrap_or_else(|| {
                            chrono::DateTime::from_timestamp(0, 0).unwrap().naive_utc()
                        });
                    let date_str = naivedt.date().format("%Y-%m-%d").to_string();

                    let ticks = state
                        .trading_app
                        .get_storage()
                        .get_ticks_for_date(&symbol, &date_str)
                        .await
                        .context("Failed to fetch ticks for date")?;

                    Ok(ticks.iter().map(|t| (t.ts, t.price)).collect())
                }
            }
        }
    }
    .await;

    match points_res {
        Ok(points) => {
            let computed_macd_points = compute_macd_series(&points);
            let count = computed_macd_points.len();

            debug!("Computed MACD for {} data points", count);

            let resp = HistoryResponse {
                points: computed_macd_points,
                symbol,
                mode: mode.to_string(),
                count,
            };

            HttpResponse::Ok().json(ApiResponse::success(resp))
        }
        Err(e) => handle_error(e),
    }
}

#[get("/api/health")]
#[instrument]
async fn health_check() -> impl Responder {
    HttpResponse::Ok().json(ApiResponse::success("healthy"))
}

pub async fn start_web(trading_app: Arc<TradingApp>, host: &str, port: u16) -> std::io::Result<()> {
    let config = trading_app.get_config().clone();
    let config = Arc::new(config);

    let mode = Arc::new(RwLock::new(RunMode::Sim)); // Default to Sim mode
    let state = AppState {
        mode,
        trading_app,
        config,
    };

    info!("Starting web server at {}:{}", host, port);

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(state.clone()))
            .service(set_mode)
            .service(get_mode)
            .service(get_status)
            .service(latest)
            .service(get_symbols)
            .service(history)
            .service(health_check)
            .service(actix_files::Files::new("/", "./static").index_file("index.html"))
    })
    .bind((host, port))?
    .run()
    .await
}
