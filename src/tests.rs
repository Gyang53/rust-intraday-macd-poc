// src/tests.rs
#[cfg(test)]
mod tests {
    use crate::indicators::{EMA, MACDCalc, divergence_score};

    #[test]
    fn test_ema() {
        let mut ema = EMA::new(3);
        let data = vec![1.0, 2.0, 3.0, 4.0];
        let mut last = 0.0;
        for v in data {
            last = ema.next(v);
        }
        assert!(last > 2.0 && last < 4.0);
    }

    #[test]
    fn test_macd_sequence() {
        let mut macd = MACDCalc::new(12, 26, 9);
        let prices = (1..100).map(|i| i as f64).collect::<Vec<_>>();
        let mut values = Vec::new();
        for p in prices {
            values.push(macd.next(p));
        }
        // latest MACD dif should be > 0
        let last = values.last().unwrap();
        assert!(last.dif > 0.0);
    }

    #[test]
    fn test_divergence_score() {
        // create fake rising price but falling macd
        let price = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let macd = vec![1.0, 0.8, 0.6, 0.4, 0.2];
        let score = divergence_score(&price, &macd);
        assert!(score > 0.0); // bearish divergence -> sell
    }
}
