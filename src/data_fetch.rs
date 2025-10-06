use anyhow::Result;
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct Quote {
    pub symbol: String,
    pub ts: u64,
    pub price: f64,
    pub vol: f64,
}

// mock
pub async fn fetch_quote_mock(symbol: &str) -> Result<Quote> {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let price = 10.0 + rng.gen_range(-0.5..0.5);
    let vol = rng.gen_range(100.0..10000.0);
    Ok(Quote {
        symbol: symbol.to_string(),
        ts: chrono::Utc::now().timestamp_millis() as u64,
        price,
        vol,
    })
}

/// 东方财富接口示例（仅开发参考，不可用于实盘）
/// symbol 例子: "000001.SZ"
pub async fn fetch_quote_eastmoney_example(symbol: &str) -> Result<Quote> {
    let url = format!(
        "https://push2.eastmoney.com/api/qt/stock/get?secid={}&fields=f43,f47,f48",
        if symbol.ends_with(".SZ") {
            format!("0.{}", &symbol[0..6])
        } else {
            format!("1.{}", &symbol[0..6])
        }
    );

    let resp = reqwest::Client::new()
        .get(&url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;

    let price = resp["data"]["f43"].as_f64().unwrap_or(0.0) / 100.0;
    let vol = resp["data"]["f47"].as_f64().unwrap_or(0.0);
    let ts = chrono::Utc::now().timestamp_millis() as u64;

    Ok(Quote {
        symbol: symbol.to_string(),
        ts,
        price,
        vol,
    })
}
