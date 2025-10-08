// src/app.rs
use crate::config::AppConfig;
use crate::error::{AppError, Result};
use crate::indicators::{MACDPoint, compute_macd_series};
use crate::storage::{Storage, Tick};

use serde::Serialize;
use std::sync::Arc;
use tracing::{debug, instrument};

#[derive(Debug, Clone)]
pub struct TradingApp {
    storage: Arc<Storage>,
    config: Arc<AppConfig>,
}

#[derive(Debug, Serialize)]
pub struct SymbolInfo {
    pub symbol: String,
    pub latest_tick: Option<Tick>,
    pub data_points: usize,
}

#[derive(Debug, Serialize)]
pub struct MarketAnalysis {
    pub symbol: String,
    pub macd_points: Vec<MACDPoint>,
    pub signal_count: usize,
    pub bullish_signals: usize,
    pub bearish_signals: usize,
    pub analysis_period: String,
}

impl TradingApp {
    pub fn new(storage: Arc<Storage>, config: Arc<AppConfig>) -> Self {
        Self { storage, config }
    }

    #[instrument(skip(self))]
    pub async fn get_symbol_info(&self, symbol: &str) -> Result<SymbolInfo> {
        debug!("Getting symbol info for: {}", symbol);

        let latest_tick = self.storage.get_latest_tick(symbol).await?;

        // Get recent data points count
        let recent_ticks = self
            .storage
            .get_ticks_recent_days(symbol, 1)
            .await
            .unwrap_or_default();

        Ok(SymbolInfo {
            symbol: symbol.to_string(),
            latest_tick,
            data_points: recent_ticks.len(),
        })
    }

    #[instrument(skip(self))]
    pub async fn get_market_analysis(
        &self,
        symbol: &str,
        days: Option<i64>,
    ) -> Result<MarketAnalysis> {
        let analysis_days = days.unwrap_or(30);
        debug!(
            "Generating market analysis for {} over {} days",
            symbol, analysis_days
        );

        let ticks = self
            .storage
            .get_ticks_recent_days(symbol, analysis_days)
            .await?;

        if ticks.is_empty() {
            return Err(AppError::DataNotFound(format!(
                "No data found for symbol {} in the last {} days",
                symbol, analysis_days
            )));
        }

        let price_points: Vec<(i64, f64)> = ticks.iter().map(|t| (t.ts, t.price)).collect();
        let macd_points = compute_macd_series(&price_points);

        let (bullish_signals, bearish_signals) = Self::count_macd_signals(&macd_points);

        Ok(MarketAnalysis {
            symbol: symbol.to_string(),
            macd_points,
            signal_count: bullish_signals + bearish_signals,
            bullish_signals,
            bearish_signals,
            analysis_period: format!("{} days", analysis_days),
        })
    }

    #[instrument(skip(self))]
    pub async fn get_all_symbols_info(&self) -> Result<Vec<SymbolInfo>> {
        let symbols = self.storage.get_symbols().await?;
        let mut symbols_info = Vec::new();

        for symbol in symbols {
            match self.get_symbol_info(&symbol).await {
                Ok(info) => symbols_info.push(info),
                Err(e) => {
                    debug!("Failed to get info for symbol {}: {}", symbol, e);
                    // Continue with other symbols
                }
            }
        }

        Ok(symbols_info)
    }

    fn count_macd_signals(macd_points: &[MACDPoint]) -> (usize, usize) {
        let mut bullish_signals = 0;
        let mut bearish_signals = 0;

        for i in 1..macd_points.len() {
            let prev = &macd_points[i - 1];
            let current = &macd_points[i];

            // Bullish signal: MACD crosses above zero
            if prev.macd <= 0.0 && current.macd > 0.0 {
                bullish_signals += 1;
            }
            // Bearish signal: MACD crosses below zero
            else if prev.macd >= 0.0 && current.macd < 0.0 {
                bearish_signals += 1;
            }
        }

        (bullish_signals, bearish_signals)
    }

    pub fn get_config(&self) -> &AppConfig {
        &self.config
    }

    pub fn get_storage(&self) -> &Arc<Storage> {
        &self.storage
    }
}
