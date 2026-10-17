//! Relative Strength Index (Wilder).

use crate::indicator::Indicator;

/// Wilder's Relative Strength Index.
///
/// Produces values in `[0, 100]`. The first value appears after `period + 1`
/// inputs (one extra to establish the first price change). Subsequent values
/// use Wilder smoothing:
///
/// `avg = (avg * (n - 1) + x) / n`
///
/// ```
/// use honba_indicators::{Indicator, Rsi};
///
/// let mut rsi = Rsi::new(3);
/// for p in [1.0, 2.0, 3.0, 4.0] {
///     rsi.update(p);
/// }
/// // All-up moves drive RSI to 100.
/// assert_eq!(rsi.value(), Some(100.0));
/// ```
#[derive(Clone, Debug)]
pub struct Rsi {
    period: usize,
    prev_price: Option<f64>,
    seed_gains: Vec<f64>,
    seed_losses: Vec<f64>,
    avg_gain: Option<f64>,
    avg_loss: Option<f64>,
    last: Option<f64>,
}

impl Rsi {
    /// Creates an RSI over `period` price changes.
    ///
    /// # Panics
    ///
    /// Panics if `period` is zero.
    pub fn new(period: usize) -> Self {
        assert!(period > 0, "RSI period must be positive");
        Self {
            period,
            prev_price: None,
            seed_gains: Vec::with_capacity(period),
            seed_losses: Vec::with_capacity(period),
            avg_gain: None,
            avg_loss: None,
            last: None,
        }
    }

    /// Returns the configured period.
    pub fn period(&self) -> usize {
        self.period
    }

    fn compute(avg_gain: f64, avg_loss: f64) -> f64 {
        if avg_loss == 0.0 {
            100.0
        } else {
            let rs = avg_gain / avg_loss;
            100.0 - 100.0 / (1.0 + rs)
        }
    }
}

impl Indicator for Rsi {
    type Input<'a> = f64;
    type Output = f64;

    fn update(&mut self, price: f64) -> Option<f64> {
        let prev = match self.prev_price {
            None => {
                self.prev_price = Some(price);
                return None;
            }
            Some(p) => p,
        };
        self.prev_price = Some(price);

        let change = price - prev;
        let gain = if change > 0.0 { change } else { 0.0 };
        let loss = if change < 0.0 { -change } else { 0.0 };

        if self.avg_gain.is_none() {
            self.seed_gains.push(gain);
            self.seed_losses.push(loss);
            if self.seed_gains.len() == self.period {
                let n = self.period as f64;
                let g: f64 = self.seed_gains.iter().sum::<f64>() / n;
                let l: f64 = self.seed_losses.iter().sum::<f64>() / n;
                self.avg_gain = Some(g);
                self.avg_loss = Some(l);
                let v = Self::compute(g, l);
                self.last = Some(v);
                return Some(v);
            }
            return None;
        }

        let n = self.period as f64;
        let g = (self.avg_gain.unwrap() * (n - 1.0) + gain) / n;
        let l = (self.avg_loss.unwrap() * (n - 1.0) + loss) / n;
        self.avg_gain = Some(g);
        self.avg_loss = Some(l);
        let v = Self::compute(g, l);
        self.last = Some(v);
        Some(v)
    }

    fn value(&self) -> Option<f64> {
        self.last
    }

    fn reset(&mut self) {
        self.prev_price = None;
        self.seed_gains.clear();
        self.seed_losses.clear();
        self.avg_gain = None;
        self.avg_loss = None;
        self.last = None;
    }
}
