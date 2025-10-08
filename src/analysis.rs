use crate::eastmoney::StockData;
use crate::indicators::calculate_macd;

#[derive(Debug, Clone, serde::Serialize)]
pub struct TradeSignal {
    pub date: String,
    pub signal: String, // "BUY" or "SELL"
    pub confidence: f64,
    pub price: f64,
}

pub fn analyze_signals(data: &[StockData]) -> Vec<TradeSignal> {
    let closes: Vec<f64> = data.iter().map(|d| d.close).collect();
    let (_dif, _dea, macd) = calculate_macd(&closes);
    let mut signals = vec![];

    for i in 1..macd.len() {
        // 金叉
        if macd[i - 1] < 0.0 && macd[i] > 0.0 {
            signals.push(TradeSignal {
                date: data[i].date.to_string(),
                signal: "BUY".into(),
                confidence: (macd[i].abs() * 10.0).min(100.0),
                price: data[i].close,
            });
        }
        // 死叉
        if macd[i - 1] > 0.0 && macd[i] < 0.0 {
            signals.push(TradeSignal {
                date: data[i].date.to_string(),
                signal: "SELL".into(),
                confidence: (macd[i].abs() * 10.0).min(100.0),
                price: data[i].close,
            });
        }
    }
    signals
}
