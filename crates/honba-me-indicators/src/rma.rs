//! RMA (Running Moving Average / Wilder's Smoothing).
//!
//! RMA is Wilder's smoothing method used in RSI, ATR, and ADX calculations.
//! It gives more weight to recent values.
//!
//! # Formula
//!
//! ```text
//! RMA = (PrevRMA * (Period - 1) + Current) / Period
//! ```

use crate::indicator::Indicator;

/// Running Moving Average (Wilder's Smoothing).
#[derive(Clone, Debug)]
pub struct Rma {
    period: usize,
    values: Vec<f64>,
    sum: f64,
    rma: Option<f64>,
}

impl Rma {
    /// Creates a new RMA with the given period.
    pub fn new(period: usize) -> Self {
        Self {
            period,
            values: Vec::with_capacity(period),
            sum: 0.0,
            rma: None,
        }
    }

    /// Returns the configured period.
    pub fn period(&self) -> usize {
        self.period
    }

    /// Returns the current RMA value.
    pub fn value(&self) -> Option<f64> {
        self.rma
    }
}

impl Default for Rma {
    fn default() -> Self {
        Self::new(14)
    }
}

impl Indicator for Rma {
    type Input<'a> = f64;
    type Output = f64;

    fn update(&mut self, input: f64) -> Option<f64> {
        self.values.push(input);
        self.sum += input;

        if self.values.len() < self.period {
            return None;
        }

        if self.values.len() == self.period {
            // First RMA = SMA of first `period` values
            self.rma = Some(self.sum / self.period as f64);
        } else {
            // Wilder smoothing: (PrevRMA * (Period - 1) + Current) / Period
            let prev = self.rma.unwrap_or(0.0);
            self.rma = Some((prev * (self.period as f64 - 1.0) + input) / self.period as f64);
        }

        self.rma
    }

    fn value(&self) -> Option<f64> {
        self.rma
    }

    fn reset(&mut self) {
        self.values.clear();
        self.sum = 0.0;
        self.rma = None;
    }
}
