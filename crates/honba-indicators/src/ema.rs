//! Exponential Moving Average.

use crate::indicator::Indicator;
use crate::sma::Sma;

/// An exponential moving average.
///
/// Seeded with the SMA of the first `period` inputs, then smoothed using
/// `alpha = 2 / (period + 1)`. This is the standard formulation used by most
/// charting platforms; it converges to the same values as the recursive
/// form once the seed effect decays.
///
/// ```
/// use honba_indicators::{Ema, Indicator};
///
/// let mut ema = Ema::new(3);
/// assert_eq!(ema.update(1.0), None);
/// assert_eq!(ema.update(2.0), None);
/// assert_eq!(ema.update(3.0), Some(2.0));   // SMA seed = (1+2+3)/3
/// assert_eq!(ema.update(4.0), Some(3.0));   // 0.5*4 + 0.5*2
/// assert_eq!(ema.update(5.0), Some(4.0));   // 0.5*5 + 0.5*3
/// ```
#[derive(Clone, Debug)]
pub struct Ema {
    period: usize,
    alpha: f64,
    seed: Sma,
    prev: Option<f64>,
}

impl Ema {
    /// Creates an EMA over `period` samples.
    ///
    /// # Panics
    ///
    /// Panics if `period` is zero.
    pub fn new(period: usize) -> Self {
        assert!(period > 0, "EMA period must be positive");
        Self {
            period,
            alpha: 2.0 / (period as f64 + 1.0),
            seed: Sma::new(period),
            prev: None,
        }
    }

    /// Returns the configured period.
    pub fn period(&self) -> usize {
        self.period
    }
}

impl Indicator for Ema {
    type Input<'a> = f64;
    type Output = f64;

    fn update(&mut self, input: f64) -> Option<f64> {
        if let Some(prev) = self.prev {
            let next = self.alpha * input + (1.0 - self.alpha) * prev;
            self.prev = Some(next);
            Some(next)
        } else if let Some(seed) = self.seed.update(input) {
            self.prev = Some(seed);
            Some(seed)
        } else {
            None
        }
    }

    fn value(&self) -> Option<f64> {
        self.prev
    }

    fn reset(&mut self) {
        self.seed.reset();
        self.prev = None;
    }
}
