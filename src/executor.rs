// src/executor.rs
use anyhow::Result;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone)]
pub struct SimExecutor {
    counter: Arc<AtomicUsize>,
}

impl SimExecutor {
    pub fn new() -> Self {
        Self {
            counter: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub async fn buy(&self, symbol: &str, price: f64, amount: f64) -> Result<String> {
        let id = self.counter.fetch_add(1, Ordering::SeqCst);
        println!(
            "[SIM BUY] {} @ {:.2} x {} -> id={}",
            symbol, price, amount, id
        );
        Ok(format!("sim-{}", id))
    }

    pub async fn sell(&self, symbol: &str, price: f64, amount: f64) -> Result<String> {
        let id = self.counter.fetch_add(1, Ordering::SeqCst);
        println!(
            "[SIM SELL] {} @ {:.2} x {} -> id={}",
            symbol, price, amount, id
        );
        Ok(format!("sim-{}", id))
    }
}

/// 国信证券 API 接入模板（伪代码）
/// 实盘需要参考券商的官方 SDK 或文档
pub struct GuosenExecutor {
    api_key: String,
    secret: String,
    base_url: String,
}

impl GuosenExecutor {
    pub fn new(api_key: String, secret: String) -> Self {
        Self {
            api_key,
            secret,
            base_url: "https://api.guosen.com.cn".to_string(), // 示例，需替换
        }
    }

    /// 查询账户信息
    pub async fn account_info(&self) -> Result<serde_json::Value> {
        let url = format!("{}/account/info", self.base_url);
        let resp = reqwest::Client::new()
            .get(&url)
            .header("X-API-KEY", &self.api_key)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
        Ok(resp)
    }

    /// 买入下单
    pub async fn buy(&self, symbol: &str, price: f64, amount: f64) -> Result<serde_json::Value> {
        let url = format!("{}/trade/buy", self.base_url);
        let body = serde_json::json!({
            "symbol": symbol,
            "price": price,
            "amount": amount,
            "api_key": self.api_key,
            "sign": self.sign(symbol, price, amount),
        });
        let resp = reqwest::Client::new()
            .post(&url)
            .json(&body)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
        Ok(resp)
    }

    /// 卖出下单
    pub async fn sell(&self, symbol: &str, price: f64, amount: f64) -> Result<serde_json::Value> {
        let url = format!("{}/trade/sell", self.base_url);
        let body = serde_json::json!({
            "symbol": symbol,
            "price": price,
            "amount": amount,
            "api_key": self.api_key,
            "sign": self.sign(symbol, price, amount),
        });
        let resp = reqwest::Client::new()
            .post(&url)
            .json(&body)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
        Ok(resp)
    }

    fn sign(&self, symbol: &str, price: f64, amount: f64) -> String {
        // TODO: 根据券商文档用 secret 做签名
        format!("mock-signature-{}-{}-{}", symbol, price, amount)
    }
}
