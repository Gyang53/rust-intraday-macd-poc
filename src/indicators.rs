// src/indicators.rs
use serde::Serialize;

/// Simple EMA and MACD implementation used to build DIF/DEA/MACD series.
/// Deterministic, streaming-friendly.

#[derive(Debug)]
pub struct EMA {
    period: usize,
    mult: f64,
    current: Option<f64>,
}

impl EMA {
    pub fn new(period: usize) -> Self {
        let mult = 2.0 / (period as f64 + 1.0);
        EMA {
            period,
            mult,
            current: None,
        }
    }

    pub fn next(&mut self, value: f64) -> f64 {
        match self.current {
            None => {
                self.current = Some(value);
                value
            }
            Some(prev) => {
                let v = (value - prev) * self.mult + prev;
                self.current = Some(v);
                v
            }
        }
    }
}

#[derive(Debug)]
pub struct MACDCalc {
    ema_short: EMA,
    ema_long: EMA,
    dea_ema: EMA,
}

#[derive(Debug, Clone, Serialize)]
pub struct MACDPoint {
    pub ts: i64,
    pub price: f64,
    pub dif: f64,
    pub dea: f64,
    pub macd: f64,
}

impl MACDCalc {
    pub fn new(short: usize, long: usize, signal: usize) -> Self {
        MACDCalc {
            ema_short: EMA::new(short),
            ema_long: EMA::new(long),
            dea_ema: EMA::new(signal),
        }
    }

    /// feed a close price and get MACD values
    pub fn next(&mut self, close: f64) -> (f64, f64, f64) {
        let s = self.ema_short.next(close);
        let l = self.ema_long.next(close);
        let dif = s - l;
        let dea = self.dea_ema.next(dif);
        let macd = 2.0 * (dif - dea);
        (dif, dea, macd)
    }
}

/// Given a vector of (ts, price) returns vector of MACDPoint (with dif/dea/macd).
/// The input must be time-ordered ascending.
pub fn compute_macd_series(points: &[(i64, f64)]) -> Vec<MACDPoint> {
    let mut macd = MACDCalc::new(12, 26, 9);
    let mut out = Vec::with_capacity(points.len());
    for (ts, price) in points {
        let (dif, dea, macdv) = macd.next(*price);
        out.push(MACDPoint {
            ts: *ts,
            price: *price,
            dif,
            dea,
            macd: macdv,
        });
    }
    out
}
