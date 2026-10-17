//! Bollinger Bands.

use std::collections::VecDeque;

use crate::indicator::Indicator;

/// The three bands produced by [`BollingerBands`].
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct BollingerValue {
    /// The simple moving average.
    pub middle: f64,
    /// `middle + k * stddev`.
    pub upper: f64,
    /// `middle - k * stddev`.
    pub lower: f64,
}

/// Bollinger Bands over `period` samples with `k` standard deviations.
///
/// Uses population standard deviation (`n` in the denominator). This matches
/// the convention used by most charting platforms.
///
/// ```
/// use honba_indicators::{BollingerBands, Indicator};
///
/// let mut bb = BollingerBands::new(5, 2.0);
/// for x in [1.0, 2.0, 3.0, 4.0, 5.0] {
///     bb.update(x);
/// }
/// let v = bb.value().unwrap();
/// assert_eq!(v.middle, 3.0);
/// // population stddev = sqrt(2) ≈ 1.41421356...
/// assert!((v.upper - (3.0 + 2.0 * 2.0_f64.sqrt())).abs() < 1e-9);
/// ```
#[derive(Clone, Debug)]
pub struct BollingerBands {
    period: usize,
    k: f64,
    buf: VecDeque<f64>,
    sum: f64,
    sum_sq: f64,
    last: Option<BollingerValue>,
}

impl BollingerBands {
    /// Creates Bollinger Bands.
    ///
    /// # Panics
    ///
    /// Panics if `period` is zero or `k` is negative.
    pub fn new(period: usize, k: f64) -> Self {
        assert!(period > 0, "Bollinger period must be positive");
        assert!(k >= 0.0, "Bollinger k must be non-negative");
        Self {
            period,
            k,
            buf: VecDeque::with_capacity(period),
            sum: 0.0,
            sum_sq: 0.0,
            last: None,
        }
    }

    /// Returns the configured period.
    pub fn period(&self) -> usize {
        self.period
    }

    /// Returns the configured standard-deviation multiplier.
    pub fn k(&self) -> f64 {
        self.k
    }
}

impl Indicator for BollingerBands {
    type Input<'a> = f64;
    type Output = BollingerValue;

    fn update(&mut self, input: f64) -> Option<BollingerValue> {
        self.buf.push_back(input);
        self.sum += input;
        self.sum_sq += input * input;
        if self.buf.len() > self.period {
            if let Some(old) = self.buf.pop_front() {
                self.sum -= old;
                self.sum_sq -= old * old;
            }
        }
        if self.buf.len() < self.period {
            return None;
        }

        let n = self.period as f64;
        let mean = self.sum / n;
        let variance = (self.sum_sq / n) - mean * mean;
        let variance = variance.max(0.0);
        let std = variance.sqrt();

        let v = BollingerValue {
            middle: mean,
            upper: mean + self.k * std,
            lower: mean - self.k * std,
        };
        self.last = Some(v);
        Some(v)
    }

    fn value(&self) -> Option<BollingerValue> {
        self.last
    }

    fn reset(&mut self) {
        self.buf.clear();
        self.sum = 0.0;
        self.sum_sq = 0.0;
        self.last = None;
    }
}
