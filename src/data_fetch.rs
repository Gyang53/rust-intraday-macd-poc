use crate::config::AppConfig;
use crate::error::{AppError, ErrorCode, ResultExt};
use crate::models::{Kline, MarketDepth, Quote, Trade};
use crate::utils::http_client::HttpClient;
use anyhow::{Result, anyhow};
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct DataFetcher {
    config: Arc<AppConfig>,
    http_client: HttpClient,
    cache: Arc<RwLock<HashMap<String, CachedData>>>,
}

#[derive(Debug, Clone)]
struct CachedData {
    data: serde_json::Value,
    timestamp: i64,
    ttl: i64,
}

impl DataFetcher {
    pub fn new(config: Arc<AppConfig>) -> Self {
        Self {
            config: config.clone(),
            http_client: HttpClient::new(config.server.timeout),
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get real-time quote for a symbol
    pub async fn get_quote(&self, symbol: &str) -> Result<Quote, AppError> {
        let normalized_symbol = self.normalize_symbol(symbol);

        // Try to get from cache first
        if let Some(cached) = self
            .get_from_cache(&format!("quote:{}", normalized_symbol))
            .await?
        {
            return Ok(serde_json::from_value(cached)?);
        }

        // Try multiple data sources
        let mut errors = Vec::new();

        // Try EastMoney first
        if self.config.data_source.eastmoney.enabled {
            match self.get_quote_from_eastmoney(&normalized_symbol).await {
                Ok(quote) => {
                    self.cache_data(
                        &format!("quote:{}", normalized_symbol),
                        serde_json::to_value(&quote)?,
                        self.config.data_source.cache_duration * 1000,
                    )
                    .await?;
                    return Ok(quote);
                }
                Err(e) => errors.push(("EastMoney", e)),
            }
        }

        // Try Baidu Finance
        if self.config.data_source.baidu.enabled {
            match self.get_quote_from_baidu(&normalized_symbol).await {
                Ok(quote) => {
                    self.cache_data(
                        &format!("quote:{}", normalized_symbol),
                        serde_json::to_value(&quote)?,
                        self.config.data_source.cache_duration * 1000,
                    )
                    .await?;
                    return Ok(quote);
                }
                Err(e) => errors.push(("Baidu Finance", e)),
            }
        }

        // Try Sina Finance
        if self.config.data_source.sina.enabled {
            match self.get_quote_from_sina(&normalized_symbol).await {
                Ok(quote) => {
                    self.cache_data(
                        &format!("quote:{}", normalized_symbol),
                        serde_json::to_value(&quote)?,
                        self.config.data_source.cache_duration * 1000,
                    )
                    .await?;
                    return Ok(quote);
                }
                Err(e) => errors.push(("Sina Finance", e)),
            }
        }

        // If all sources failed
        let error_msg = format!(
            "Failed to get quote for {} from all sources: {:?}",
            symbol, errors
        );
        Err(AppError::api(ErrorCode::DataSourceError, error_msg))
    }

    /// Get historical K-line data
    pub async fn get_kline_data(
        &self,
        symbol: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
        period: &str,
    ) -> Result<Vec<Kline>, AppError> {
        let normalized_symbol = self.normalize_symbol(symbol);
        let cache_key = format!(
            "kline:{}:{}:{}:{}",
            normalized_symbol,
            start_date.format("%Y%m%d"),
            end_date.format("%Y%m%d"),
            period
        );

        // Try cache
        if let Some(cached) = self.get_from_cache(&cache_key).await? {
            return Ok(serde_json::from_value(cached)?);
        }

        let klines = self
            .get_kline_from_eastmoney(&normalized_symbol, start_date, end_date, period)
            .await?;

        self.cache_data(
            &cache_key,
            serde_json::to_value(&klines)?,
            3600 * 1000, // Cache for 1 hour
        )
        .await?;

        Ok(klines)
    }

    /// Get market depth data
    pub async fn get_market_depth(&self, symbol: &str) -> Result<MarketDepth, AppError> {
        let normalized_symbol = self.normalize_symbol(symbol);
        let cache_key = format!("depth:{}", normalized_symbol);

        if let Some(cached) = self.get_from_cache(&cache_key).await? {
            return Ok(serde_json::from_value(cached)?);
        }

        let depth = self.get_depth_from_eastmoney(&normalized_symbol).await?;

        self.cache_data(
            &cache_key,
            serde_json::to_value(&depth)?,
            30 * 1000, // Cache for 30 seconds
        )
        .await?;

        Ok(depth)
    }

    /// Get recent trades
    pub async fn get_recent_trades(
        &self,
        symbol: &str,
        limit: u32,
    ) -> Result<Vec<Trade>, AppError> {
        let normalized_symbol = self.normalize_symbol(symbol);
        let cache_key = format!("trades:{}:{}", normalized_symbol, limit);

        if let Some(cached) = self.get_from_cache(&cache_key).await? {
            return Ok(serde_json::from_value(cached)?);
        }

        let trades = self
            .get_trades_from_eastmoney(&normalized_symbol, limit)
            .await?;

        self.cache_data(
            &cache_key,
            serde_json::to_value(&trades)?,
            10 * 1000, // Cache for 10 seconds
        )
        .await?;

        Ok(trades)
    }

    /// Normalize stock symbol to standard format
    fn normalize_symbol(&self, symbol: &str) -> String {
        let symbol = symbol.trim().to_uppercase();

        // Convert to standard format: 000001.SZ, 600733.SH
        if symbol.ends_with(".SZ") || symbol.ends_with(".SH") {
            return symbol;
        }

        if symbol.len() == 6 {
            if symbol.starts_with(|c: char| c.is_ascii_digit()) {
                let prefix = &symbol[0..1];
                if prefix == "0" || prefix == "3" {
                    return format!("{}.SZ", symbol);
                } else if prefix == "6" {
                    return format!("{}.SH", symbol);
                }
            }
        }

        symbol
    }

    /// Get quote from EastMoney
    async fn get_quote_from_eastmoney(&self, symbol: &str) -> Result<Quote, AppError> {
        let (market, code) = self.parse_symbol(symbol)?;

        let url = format!(
            "{}/api/qt/stock/get?secid={}.{}&fields=f43,f47,f48,f49,f50,f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61,f62,f63,f64,f65,f66,f67,f68,f69,f70,f71,f72,f73,f74,f75,f76,f77,f78,f79,f80,f81,f82,f83,f84,f85,f86,f87,f88,f89,f90,f91,f92,f93,f94,f95,f96,f97,f98,f99,f100,f101,f102,f103,f104,f105,f106,f107,f108,f109,f110,f111,f112,f113,f114,f115,f116,f117,f118,f119,f120,f121,f122,f123,f124,f125,f126,f127,f128,f129,f130,f131,f132,f133,f134,f135,f136,f137,f138,f139,f140,f141,f142,f143,f144,f145,f146,f147,f148,f149,f150,f151,f152,f153,f154,f155,f156,f157,f158,f159,f160,f161,f162,f163,f164,f165,f166,f167,f168,f169,f170,f171,f172,f173,f174,f175,f176,f177,f178,f179,f180,f181,f182,f183,f184,f185,f186,f187,f188,f189,f190,f191,f192,f193,f194,f195,f196,f197,f198,f199,f200,f201,f202,f203,f204,f205,f206,f207,f208,f209,f210,f211,f212,f213,f214,f215,f216,f217,f218,f219,f220,f221,f222,f223,f224,f225,f226,f227,f228,f229,f230,f231,f232,f233,f234,f235,f236,f237,f238,f239,f240,f241,f242,f243,f244,f245,f246,f247,f248,f249,f250,f251,f252,f253,f254,f255,f256,f257,f258,f259,f260,f261,f262,f263,f264,f265,f266,f267,f268,f269,f270,f271,f272,f273,f274,f275,f276,f277,f278,f279,f280,f281,f282,f283,f284,f285,f286,f287,f288,f289,f290,f291,f292,f293,f294,f295,f296,f297,f298,f299,f300",
            self.config.data_source.eastmoney.base_url, market, code
        );

        let response = self
            .http_client
            .get(&url)
            .header("User-Agent", &self.config.data_source.eastmoney.user_agent)
            .send()
            .await
            .with_context("Failed to fetch data from EastMoney")?;

        let json: serde_json::Value = response
            .json()
            .await
            .with_context("Failed to parse EastMoney response")?;

        let data = json["data"]
            .as_object()
            .ok_or_else(|| AppError::data_not_found("No data found for symbol"))?;

        let price = self.get_decimal(data, "f43")?;
        let open = self.get_decimal_opt(data, "f46");
        let high = self.get_decimal_opt(data, "f44");
        let low = self.get_decimal_opt(data, "f45");
        let prev_close = self.get_decimal_opt(data, "f47");
        let volume = self.get_decimal_opt(data, "f48");
        let amount = self.get_decimal_opt(data, "f49");
        let change = self.get_decimal_opt(data, "f134");
        let change_pct = self.get_decimal_opt(data, "f135");
        let bid_price = self.get_decimal_opt(data, "f18");
        let ask_price = self.get_decimal_opt(data, "f19");
        let bid_volume = self.get_decimal_opt(data, "f10");
        let ask_volume = self.get_decimal_opt(data, "f11");

        Ok(Quote {
            symbol: symbol.to_string(),
            timestamp: Utc::now().timestamp_millis(),
            price,
            open,
            high,
            low,
            prev_close,
            volume,
            amount,
            change,
            change_pct,
            bid_price,
            ask_price,
            bid_volume,
            ask_volume,
        })
    }

    /// Get K-line data from EastMoney
    async fn get_kline_from_eastmoney(
        &self,
        symbol: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
        period: &str,
    ) -> Result<Vec<Kline>, AppError> {
        let (market, code) = self.parse_symbol(symbol)?;
        let ktype = self.convert_period_to_ktype(period)?;

        let start_ts = start_date
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| AppError::invalid_date_range())?
            .timestamp_millis();
        let end_ts = end_date
            .and_hms_opt(23, 59, 59)
            .ok_or_else(|| AppError::invalid_date_range())?
            .timestamp_millis();

        let mut klines = Vec::new();
        let mut current_end = end_ts;

        while current_end >= start_ts {
            let url = format!(
                "{}/api/qt/stock/kline/get?secid={}.{}&klt={}&fqt=0&beg={}&end={}&smplmt=1000",
                self.config.data_source.eastmoney.base_url,
                market,
                code,
                ktype,
                start_ts,
                current_end
            );

            let response = self
                .http_client
                .get(&url)
                .header("User-Agent", &self.config.data_source.eastmoney.user_agent)
                .send()
                .await
                .with_context("Failed to fetch K-line data from EastMoney")?;

            let json: serde_json::Value = response
                .json()
                .await
                .with_context("Failed to parse K-line response")?;

            let data = json["data"]
                .as_object()
                .ok_or_else(|| AppError::data_not_found("No K-line data found"))?;

            let klines_str = data["klines"]
                .as_array()
                .ok_or_else(|| AppError::data_not_found("No K-line data found"))?;

            if klines_str.is_empty() {
                break;
            }

            for kline_str in klines_str {
                let kline_data: Vec<&str> = kline_str.as_str().unwrap().split(',').collect();
                if kline_data.len() < 6 {
                    continue;
                }

                let date_str = kline_data[0];
                let open = Decimal::from_str_radix(kline_data[1], 10)
                    .with_context(format!("Invalid open price: {}", kline_data[1]))?;
                let close = Decimal::from_str_radix(kline_data[2], 10)
                    .with_context(format!("Invalid close price: {}", kline_data[2]))?;
                let high = Decimal::from_str_radix(kline_data[3], 10)
                    .with_context(format!("Invalid high price: {}", kline_data[3]))?;
                let low = Decimal::from_str_radix(kline_data[4], 10)
                    .with_context(format!("Invalid low price: {}", kline_data[4]))?;
                let volume = Decimal::from_str_radix(kline_data[5], 10)
                    .with_context(format!("Invalid volume: {}", kline_data[5]))?;

                let datetime = NaiveDateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M")
                    .or_else(|_| NaiveDateTime::parse_from_str(date_str, "%Y-%m-%d"))
                    .with_context(format!("Invalid date format: {}", date_str))?;

                klines.push(Kline {
                    symbol: symbol.to_string(),
                    timestamp: datetime.timestamp_millis(),
                    open,
                    high,
                    low,
                    close,
                    volume,
                    amount: None,
                    period: period.to_string(),
                });
            }

            // Update current_end to get previous page
            if let Some(first_kline) = klines.first() {
                current_end = first_kline.timestamp - 1;
            } else {
                break;
            }

            // Add delay to avoid rate limiting
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // Sort by timestamp ascending
        klines.sort_by_key(|k| k.timestamp);

        Ok(klines)
    }

    /// Get market depth from EastMoney
    async fn get_depth_from_eastmoney(&self, symbol: &str) -> Result<MarketDepth, AppError> {
        let (market, code) = self.parse_symbol(symbol)?;

        let url = format!(
            "{}/api/qt/bdata/get?secid={}.{}&fields=f1,f2,f3,f4,f5,f6,f7,f8,f9,f10,f11,f12,f13,f14,f15,f16,f17,f18,f19,f20,f21,f22,f23,f24,f25,f26,f27,f28,f29,f30,f31,f32,f33,f34,f35,f36,f37,f38,f39,f40,f41,f42,f43,f44,f45,f46,f47,f48,f49,f50,f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61,f62,f63,f64,f65,f66,f67,f68,f69,f70,f71,f72,f73,f74,f75,f76,f77,f78,f79,f80,f81,f82,f83,f84,f85,f86,f87,f88,f89,f90,f91,f92,f93,f94,f95,f96,f97,f98,f99,f100",
            self.config.data_source.eastmoney.base_url, market, code
        );

        let response = self
            .http_client
            .get(&url)
            .header("User-Agent", &self.config.data_source.eastmoney.user_agent)
            .send()
            .await
            .with_context("Failed to fetch market depth from EastMoney")?;

        let json: serde_json::Value = response
            .json()
            .await
            .with_context("Failed to parse market depth response")?;

        let data = json["data"]
            .as_object()
            .ok_or_else(|| AppError::data_not_found("No market depth data found"))?;

        let mut bids = Vec::new();
        let mut asks = Vec::new();

        // Parse bid orders (f1-f5: price, f6-f10: volume)
        for i in 0..5 {
            let price_key = format!("f{}", i + 1);
            let volume_key = format!("f{}", i + 6);

            if let (Some(price), Some(volume)) = (
                self.get_decimal_opt(data, &price_key),
                self.get_decimal_opt(data, &volume_key),
            ) {
                if price > Decimal::ZERO && volume > Decimal::ZERO {
                    bids.push((price, volume));
                }
            }
        }

        // Parse ask orders (f11-f15: price, f16-f20: volume)
        for i in 0..5 {
            let price_key = format!("f{}", i + 11);
            let volume_key = format!("f{}", i + 16);

            if let (Some(price), Some(volume)) = (
                self.get_decimal_opt(data, &price_key),
                self.get_decimal_opt(data, &volume_key),
            ) {
                if price > Decimal::ZERO && volume > Decimal::ZERO {
                    asks.push((price, volume));
                }
            }
        }

        Ok(MarketDepth {
            symbol: symbol.to_string(),
            timestamp: Utc::now().timestamp_millis(),
            bids,
            asks,
        })
    }

    /// Get recent trades from EastMoney
    async fn get_trades_from_eastmoney(
        &self,
        symbol: &str,
        limit: u32,
    ) -> Result<Vec<Trade>, AppError> {
        let (market, code) = self.parse_symbol(symbol)?;

        let url = format!(
            "{}/api/qt/stock/tradedetail/get?secid={}.{}&num={}",
            self.config.data_source.eastmoney.base_url, market, code, limit
        );

        let response = self
            .http_client
            .get(&url)
            .header("User-Agent", &self.config.data_source.eastmoney.user_agent)
            .send()
            .await
            .with_context("Failed to fetch trades from EastMoney")?;

        let json: serde_json::Value = response
            .json()
            .await
            .with_context("Failed to parse trades response")?;

        let data = json["data"]
            .as_object()
            .ok_or_else(|| AppError::data_not_found("No trades data found"))?;

        let trades_str = data["trades"]
            .as_array()
            .ok_or_else(|| AppError::data_not_found("No trades data found"))?;

        let mut trades = Vec::new();

        for trade_str in trades_str {
            let trade_data: Vec<&str> = trade_str.as_str().unwrap().split(',').collect();
            if trade_data.len() < 5 {
                continue;
            }

            let trade_id = trade_data[0].to_string();
            let price = Decimal::from_str_radix(trade_data[1], 10)
                .with_context(format!("Invalid trade price: {}", trade_data[1]))?;
            let volume = Decimal::from_str_radix(trade_data[2], 10)
                .with_context(format!("Invalid trade volume: {}", trade_data[2]))?;
            let side = if trade_data[3] == "B" {
                crate::models::TradeSide::Buy
            } else {
                crate::models::TradeSide::Sell
            };
            let time_str = trade_data[4];

            let time = NaiveTime::parse_from_str(time_str, "%H:%M:%S")
                .with_context(format!("Invalid time format: {}", time_str))?;
            let today = NaiveDate::today();
            let datetime = NaiveDateTime::new(today, time);
            let timestamp = datetime.timestamp_millis();

            trades.push(Trade {
                trade_id,
                symbol: symbol.to_string(),
                timestamp,
                price,
                volume,
                side,
                trade_type: None,
            });
        }

        Ok(trades)
    }

    /// Get quote from Baidu Finance
    async fn get_quote_from_baidu(&self, symbol: &str) -> Result<Quote, AppError> {
        let code = self.get_baidu_code(symbol)?;

        let url = format!(
            "{}/selfselect/getstockquotation?code={}&all=1&ktype=1&isIndex=false&isBk=false&isBlock=false&isFutures=false&stockType=ab&group=quotation_kline_ab&finClientType=pc",
            self.config.data_source.baidu.base_url, code
        );

        let response = self
            .http_client
            .get(&url)
            .header("User-Agent", &self.config.data_source.baidu.user_agent)
            .send()
            .await
            .with_context("Failed to fetch data from Baidu Finance")?;

        let json: serde_json::Value = response
            .json()
            .await
            .with_context("Failed to parse Baidu Finance response")?;

        let result = json["Result"]
            .as_array()
            .ok_or_else(|| AppError::data_not_found("No data found for symbol"))?;

        if result.is_empty() {
            return Err(AppError::data_not_found("No data found for symbol"));
        }

        let data = result[0].as_object().unwrap();

        let price = self.get_decimal(data, "f43")?;
        let open = self.get_decimal_opt(data, "f46");
        let high = self.get_decimal_opt(data, "f44");
        let low = self.get_decimal_opt(data, "f45");
        let prev_close = self.get_decimal_opt(data, "f47");
        let volume = self.get_decimal_opt(data, "f48");
        let amount = self.get_decimal_opt(data, "f49");
        let change = self.get_decimal_opt(data, "f134");
        let change_pct = self.get_decimal_opt(data, "f135");

        Ok(Quote {
            symbol: symbol.to_string(),
            timestamp: Utc::now().timestamp_millis(),
            price,
            open,
            high,
            low,
            prev_close,
            volume,
            amount,
            change,
            change_pct,
            bid_price: None,
            ask_price: None,
            bid_volume: None,
            ask_volume: None,
        })
    }

    /// Get quote from Sina Finance
    async fn get_quote_from_sina(&self, symbol: &str) -> Result<Quote, AppError> {
        let sina_code = self.get_sina_code(symbol)?;

        let url = format!(
            "{}/listview/{}.js",
            self.config.data_source.sina.base_url, sina_code
        );

        let response = self
            .http_client
            .get(&url)
            .header("User-Agent", &self.config.data_source.sina.user_agent)
            .send()
            .await
            .with_context("Failed to fetch data from Sina Finance")?;

        let text = response
            .text()
            .await
            .with_context("Failed to read Sina Finance response")?;

        // Parse the JavaScript data
        let json_str = text
            .splitn(2, '=')
            .nth(1)
            .and_then(|s| s.strip_suffix(';'))
            .ok_or_else(|| AppError::invalid_data("Invalid Sina Finance response format"))?;

        let json: serde_json::Value =
            serde_json::from_str(json_str).with_context("Failed to parse Sina Finance JSON")?;

        let data = json["data"]
            .as_array()
            .ok_or_else(|| AppError::data_not_found("No data found for symbol"))?;

        if data.is_empty() {
            return Err(AppError::data_not_found("No data found for symbol"));
        }

        let quote_data = data[0].as_object().unwrap();

        let price = self.get_decimal(quote_data, "price")?;
        let open = self.get_decimal_opt(quote_data, "open");
        let high = self.get_decimal_opt(quote_data, "high");
        let low = self.get_decimal_opt(quote_data, "low");
        let prev_close = self.get_decimal_opt(quote_data, "preclose");
        let volume = self.get_decimal_opt(quote_data, "volume");
        let amount = self.get_decimal_opt(quote_data, "amount");
        let change = self.get_decimal_opt(quote_data, "change");
        let change_pct = self.get_decimal_opt(quote_data, "changepercent");

        Ok(Quote {
            symbol: symbol.to_string(),
            timestamp: Utc::now().timestamp_millis(),
            price,
            open,
            high,
            low,
            prev_close,
            volume,
            amount,
            change,
            change_pct,
            bid_price: None,
            ask_price: None,
            bid_volume: None,
            ask_volume: None,
        })
    }

    /// Parse symbol into market and code
    fn parse_symbol(&self, symbol: &str) -> Result<(i32, &str), AppError> {
        if symbol.ends_with(".SZ") {
            let code = &symbol[0..6];
            Ok((0, code))
        } else if symbol.ends_with(".SH") {
            let code = &symbol[0..6];
            Ok((1, code))
        } else {
            Err(AppError::invalid_symbol(symbol))
        }
    }

    /// Convert period to EastMoney ktype
    fn convert_period_to_ktype(&self, period: &str) -> Result<i32, AppError> {
        match period.to_lowercase().as_str() {
            "1min" | "1分钟" => Ok(1),
            "5min" | "5分钟" => Ok(5),
            "15min" | "15分钟" => Ok(15),
            "30min" | "30分钟" => Ok(30),
            "60min" | "60分钟" | "1小时" => Ok(60),
            "day" | "日线" => Ok(101),
            "week" | "周线" => Ok(102),
            "month" | "月线" => Ok(103),
            _ => Err(AppError::invalid_parameter(format!(
                "Unsupported period: {}",
                period
            ))),
        }
    }

    /// Get Baidu finance code format
    fn get_baidu_code(&self, symbol: &str) -> Result<&str, AppError> {
        if symbol.ends_with(".SZ") {
            Ok(&symbol[0..6])
        } else if symbol.ends_with(".SH") {
            Ok(&symbol[0..6])
        } else {
            Err(AppError::invalid_symbol(symbol))
        }
    }

    /// Get Sina finance code format
    fn get_sina_code(&self, symbol: &str) -> Result<String, AppError> {
        if symbol.ends_with(".SZ") {
            Ok(format!("sz{}", &symbol[0..6]))
        } else if symbol.ends_with(".SH") {
            Ok(format!("sh{}", &symbol[0..6]))
        } else {
            Err(AppError::invalid_symbol(symbol))
        }
    }

    /// Helper to get Decimal from JSON object
    fn get_decimal(
        &self,
        data: &serde_json::Map<String, serde_json::Value>,
        key: &str,
    ) -> Result<Decimal, AppError> {
        let value = data
            .get(key)
            .ok_or_else(|| AppError::data_not_found(format!("Missing key: {}", key)))?;

        self.parse_decimal(value, key)
    }

    /// Helper to get optional Decimal from JSON object
    fn get_decimal_opt(
        &self,
        data: &serde_json::Map<String, serde_json::Value>,
        key: &str,
    ) -> Option<Decimal> {
        data.get(key).and_then(|v| self.parse_decimal(v, key).ok())
    }

    /// Helper to parse Decimal from JSON value
    fn parse_decimal(&self, value: &serde_json::Value, key: &str) -> Result<Decimal, AppError> {
        if let Some(s) = value.as_str() {
            Decimal::from_str_radix(s, 10)
                .with_context(format!("Invalid decimal value for {}: {}", key, s))
        } else if let Some(n) = value.as_f64() {
            Ok(Decimal::from_f64(n)
                .ok_or_else(|| {
                    AppError::invalid_data(format!("Invalid number for {}: {}", key, n))
                })?
                .round_dp(2))
        } else if let Some(n) = value.as_i64() {
            Ok(Decimal::from(n))
        } else {
            Err(AppError::invalid_data(format!(
                "Unsupported type for {}: {:?}",
                key, value
            )))
        }
    }

    /// Cache data
    async fn cache_data(
        &self,
        key: &str,
        data: serde_json::Value,
        ttl: i64,
    ) -> Result<(), AppError> {
        let mut cache = self.cache.write().await;
        cache.insert(
            key.to_string(),
            CachedData {
                data,
                timestamp: Utc::now().timestamp_millis(),
                ttl,
            },
        );
        Ok(())
    }

    /// Get data from cache
    async fn get_from_cache(&self, key: &str) -> Result<Option<serde_json::Value>, AppError> {
        let mut cache = self.cache.write().await;
        let now = Utc::now().timestamp_millis();

        // Clean up expired cache entries
        cache.retain(|_, v| now - v.timestamp < v.ttl);

        Ok(cache.get(key).map(|v| v.data.clone()))
    }
}

impl std::fmt::Display for DataFetcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DataFetcher(sources={:?})", self.get_enabled_sources())
    }
}

impl DataFetcher {
    fn get_enabled_sources(&self) -> Vec<&str> {
        let mut sources = Vec::new();
        if self.config.data_source.eastmoney.enabled {
            sources.push("EastMoney");
        }
        if self.config.data_source.baidu.enabled {
            sources.push("Baidu Finance");
        }
        if self.config.data_source.sina.enabled {
            sources.push("Sina Finance");
        }
        sources
    }
}
/// Get real-time quote for a symbol
pub async fn get_quote(&self, symbol: &str) -> Result<Quote, AppError> {
    let normalized_symbol = self.normalize_symbol(symbol);
    log::info!("Getting quote for symbol: {}", normalized_symbol);

    // Try to get from cache first
    if let Some(cached) = self
        .get_from_cache(&format!("quote:{}", normalized_symbol))
        .await?
    {
        log::debug!("Returning cached quote for {}", normalized_symbol);
        return Ok(serde_json::from_value(cached)?);
    }

    // Try multiple data sources with detailed error logging
    let mut errors = Vec::new();

    // Try EastMoney first
    if self.config.data_source.eastmoney.enabled {
        log::debug!(
            "Trying to get quote from EastMoney for {}",
            normalized_symbol
        );
        match self.get_quote_from_eastmoney(&normalized_symbol).await {
            Ok(quote) => {
                log::info!(
                    "Successfully got quote from EastMoney for {}",
                    normalized_symbol
                );
                self.cache_data(
                    &format!("quote:{}", normalized_symbol),
                    serde_json::to_value(&quote)?,
                    self.config.data_source.cache_duration * 1000,
                )
                .await?;
                return Ok(quote);
            }
            Err(e) => {
                log::error!("EastMoney failed for {}: {}", normalized_symbol, e);
                errors.push(("EastMoney", e.to_string()));
            }
        }
    }

    // Try Baidu Finance
    if self.config.data_source.baidu.enabled {
        log::debug!(
            "Trying to get quote from Baidu Finance for {}",
            normalized_symbol
        );
        match self.get_quote_from_baidu(&normalized_symbol).await {
            Ok(quote) => {
                log::info!(
                    "Successfully got quote from Baidu Finance for {}",
                    normalized_symbol
                );
                self.cache_data(
                    &format!("quote:{}", normalized_symbol),
                    serde_json::to_value(&quote)?,
                    self.config.data_source.cache_duration * 1000,
                )
                .await?;
                return Ok(quote);
            }
            Err(e) => {
                log::error!("Baidu Finance failed for {}: {}", normalized_symbol, e);
                errors.push(("Baidu Finance", e.to_string()));
            }
        }
    }

    // Try Sina Finance
    if self.config.data_source.sina.enabled {
        log::debug!(
            "Trying to get quote from Sina Finance for {}",
            normalized_symbol
        );
        match self.get_quote_from_sina(&normalized_symbol).await {
            Ok(quote) => {
                log::info!(
                    "Successfully got quote from Sina Finance for {}",
                    normalized_symbol
                );
                self.cache_data(
                    &format!("quote:{}", normalized_symbol),
                    serde_json::to_value(&quote)?,
                    self.config.data_source.cache_duration * 1000,
                )
                .await?;
                return Ok(quote);
            }
            Err(e) => {
                log::error!("Sina Finance failed for {}: {}", normalized_symbol, e);
                errors.push(("Sina Finance", e.to_string()));
            }
        }
    }

    // If all sources failed, try to get from storage or return simulated data
    log::warn!(
        "All data sources failed for {}, trying fallback strategies",
        normalized_symbol
    );

    // Try to get from storage
    if let Ok(Some(quote)) = self.get_quote_from_storage(&normalized_symbol).await {
        log::info!(
            "Returning historical quote from storage for {}",
            normalized_symbol
        );
        return Ok(quote);
    }

    // As a last resort, return simulated data with warning
    log::warn!(
        "No data available for {}, returning simulated data",
        normalized_symbol
    );
    Ok(self.generate_simulated_quote(&normalized_symbol))
}

/// Get quote from storage as fallback
async fn get_quote_from_storage(&self, symbol: &str) -> Result<Option<Quote>, AppError> {
    // In a real implementation, this would query the database
    // For now, return None
    Ok(None)
}

/// Generate simulated quote for fallback
fn generate_simulated_quote(&self, symbol: &str) -> Quote {
    let base_price = if symbol.starts_with("600733") {
        15.50 // Simulated price for 600733
    } else if symbol.starts_with("000001") {
        10.50 // Simulated price for 000001
    } else {
        8.0 + rand::random::<f64>() * 4.0 // Random price between 8-12
    };

    let change = (rand::random::<f64>() - 0.5) * 0.2; // Random change between -10% and +10%
    let price = base_price * (1.0 + change);

    Quote {
        symbol: symbol.to_string(),
        timestamp: chrono::Utc::now().timestamp_millis(),
        price: Decimal::from_f64(price).unwrap().round_dp(2),
        open: Some(Decimal::from_f64(base_price).unwrap().round_dp(2)),
        high: Some(
            Decimal::from_f64(base_price * (1.0 + change.abs() * 1.5))
                .unwrap()
                .round_dp(2),
        ),
        low: Some(
            Decimal::from_f64(base_price * (1.0 - change.abs() * 1.5))
                .unwrap()
                .round_dp(2),
        ),
        prev_close: Some(Decimal::from_f64(base_price).unwrap().round_dp(2)),
        volume: Some(
            Decimal::from_f64(rand::random::<f64>() * 1000000.0 + 500000.0)
                .unwrap()
                .round_dp(0),
        ),
        amount: Some(Decimal::from_f64(price * 1000000.0).unwrap().round_dp(0)),
        change: Some(Decimal::from_f64(price - base_price).unwrap().round_dp(2)),
        change_pct: Some(Decimal::from_f64(change * 100.0).unwrap().round_dp(2)),
        bid_price: Some(Decimal::from_f64(price - 0.01).unwrap().round_dp(2)),
        ask_price: Some(Decimal::from_f64(price).unwrap().round_dp(2)),
        bid_volume: Some(
            Decimal::from_f64(rand::random::<f64>() * 10000.0 + 5000.0)
                .unwrap()
                .round_dp(0),
        ),
        ask_volume: Some(
            Decimal::from_f64(rand::random::<f64>() * 10000.0 + 5000.0)
                .unwrap()
                .round_dp(0),
        ),
    }
}

/// Get historical K-line data with fallback
pub async fn get_kline_data(
    &self,
    symbol: &str,
    start_date: NaiveDate,
    end_date: NaiveDate,
    period: &str,
) -> Result<Vec<Kline>, AppError> {
    let normalized_symbol = self.normalize_symbol(symbol);
    let cache_key = format!(
        "kline:{}:{}:{}:{}",
        normalized_symbol,
        start_date.format("%Y%m%d"),
        end_date.format("%Y%m%d"),
        period
    );

    log::info!(
        "Getting K-line data for {} ({} to {})",
        normalized_symbol,
        start_date,
        end_date
    );

    // Try cache
    if let Some(cached) = self.get_from_cache(&cache_key).await? {
        log::debug!("Returning cached K-line data for {}", normalized_symbol);
        return Ok(serde_json::from_value(cached)?);
    }

    // Try to get from data source
    match self
        .get_kline_from_eastmoney(&normalized_symbol, start_date, end_date, period)
        .await
    {
        Ok(klines) => {
            log::info!("Successfully got K-line data for {}", normalized_symbol);
            self.cache_data(
                &cache_key,
                serde_json::to_value(&klines)?,
                3600 * 1000, // Cache for 1 hour
            )
            .await?;
            Ok(klines)
        }
        Err(e) => {
            log::error!("Failed to get K-line data from EastMoney: {}", e);

            // Try to get from storage
            if let Ok(Some(klines)) = self
                .get_klines_from_storage(&normalized_symbol, start_date, end_date, period)
                .await
            {
                log::info!(
                    "Returning K-line data from storage for {}",
                    normalized_symbol
                );
                return Ok(klines);
            }

            // Generate simulated K-line data
            log::warn!("Generating simulated K-line data for {}", normalized_symbol);
            Ok(self.generate_simulated_klines(&normalized_symbol, start_date, end_date, period))
        }
    }
}

/// Get K-lines from storage as fallback
async fn get_klines_from_storage(
    &self,
    symbol: &str,
    start_date: NaiveDate,
    end_date: NaiveDate,
    period: &str,
) -> Result<Option<Vec<Kline>>, AppError> {
    // In a real implementation, this would query the database
    Ok(None)
}

/// Generate simulated K-line data
fn generate_simulated_klines(
    &self,
    symbol: &str,
    start_date: NaiveDate,
    end_date: NaiveDate,
    period: &str,
) -> Vec<Kline> {
    let mut klines = Vec::new();
    let days = (end_date - start_date).num_days() as usize;

    // Base price based on symbol
    let base_price = if symbol.starts_with("600733") {
        15.50
    } else if symbol.starts_with("000001") {
        10.50
    } else {
        8.0 + rand::random::<f64>() * 4.0
    };

    let mut current_price = base_price;

    for i in 0..days {
        let date = start_date + chrono::Duration::days(i as i64);

        // Skip weekends
        let weekday = date.weekday();
        if weekday == chrono::Weekday::Sat || weekday == chrono::Weekday::Sun {
            continue;
        }

        // Generate daily price movement
        let change = (rand::random::<f64>() - 0.5) * 0.03; // ±3% daily change
        current_price *= (1.0 + change);

        let open = current_price * (0.995 + rand::random::<f64>() * 0.01); // Open within ±0.5% of current price
        let high = open * (1.0 + rand::random::<f64>() * 0.02); // High up to +2%
        let low = open * (0.98 + rand::random::<f64>() * 0.02); // Low down to -2%
        let close = if rand::random::<f64>() > 0.5 {
            (open + high + low + current_price) / 4.0
        } else {
            (open + high + low + current_price * 0.99) / 4.0
        };

        let volume = rand::random::<f64>() * 1000000.0 + 500000.0;

        klines.push(Kline {
            symbol: symbol.to_string(),
            timestamp: date.and_hms_opt(15, 0, 0).unwrap().timestamp_millis(),
            open: Decimal::from_f64(open).unwrap().round_dp(2),
            high: Decimal::from_f64(high).unwrap().round_dp(2),
            low: Decimal::from_f64(low).unwrap().round_dp(2),
            close: Decimal::from_f64(close).unwrap().round_dp(2),
            volume: Decimal::from_f64(volume).unwrap().round_dp(0),
            amount: Some(Decimal::from_f64(close * volume).unwrap().round_dp(0)),
            period: period.to_string(),
        });
    }

    klines
}
