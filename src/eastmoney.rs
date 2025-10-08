use chrono::NaiveDate;
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockData {
    pub date: NaiveDate,
    pub open: f64,
    pub close: f64,
    pub high: f64,
    pub low: f64,
    pub volume: f64,
}

pub async fn fetch_realtime_data(code: &str) -> anyhow::Result<Vec<StockData>> {
    let url = format!(
        "https://push2his.eastmoney.com/api/qt/stock/kline/get?secid=1.{}&fields1=f1,f2,f3,f4,f5&fields2=f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61&klt=101&fqt=1&end=20500101&lmt=60",
        code
    );
    let resp = Client::new()
        .get(&url)
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;
    let klines = resp["data"]["klines"]
        .as_array()
        .ok_or(anyhow::anyhow!("No data"))?;

    let mut data = vec![];
    for k in klines {
        if let Some(line) = k.as_str() {
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() >= 6 {
                data.push(StockData {
                    date: NaiveDate::parse_from_str(parts[0], "%Y-%m-%d").unwrap(),
                    open: parts[1].parse().unwrap_or(0.0),
                    close: parts[2].parse().unwrap_or(0.0),
                    high: parts[3].parse().unwrap_or(0.0),
                    low: parts[4].parse().unwrap_or(0.0),
                    volume: parts[5].parse().unwrap_or(0.0),
                });
            }
        }
    }
    Ok(data)
}
