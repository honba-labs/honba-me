//! Volume Weighted Average Price (VWAP).
//!
//! VWAP is the average price weighted by volume, typically reset at the start
//! of each trading session.
//!
//! # Formula
//!
//! ```text
//! Typical Price = (High + Low + Close) / 3
//! VWAP = Sum(TP * Volume) / Sum(Volume)
//! ```

use honba_core::Bar;
use crate::indicator::Indicator;
use rust_decimal::prelude::ToPrimitive;

/// Volume Weighted Average Price (VWAP).
///
/// VWAP is reset at the start of each trading day. It provides a volume-weighted
/// average price that serves as a benchmark for intraday trading.
#[derive(Clone, Debug)]
pub struct Vwap {
    typical_price_sum: f64,
    volume_sum: f64,
    last: Option<f64>,
}

impl Vwap {
    /// Creates a new VWAP calculator.
    pub fn new() -> Self {
        Self {
            typical_price_sum: 0.0,
            volume_sum: 0.0,
            last: None,
        }
    }

    /// Returns the configured period (always 1 for VWAP as it's cumulative).
    pub fn period(&self) -> usize {
        1
    }

    /// Returns the current VWAP value.
    pub fn value(&self) -> Option<f64> {
        self.last
    }
}

impl Default for Vwap {
    fn default() -> Self {
        Self::new()
    }
}

impl Indicator for Vwap {
    type Input<'a> = &'a Bar;
    type Output = f64;

    fn update(&mut self, bar: &Bar) -> Option<f64> {
        let high = bar.high.0.to_f64().unwrap_or(0.0);
        let low = bar.low.0.to_f64().unwrap_or(0.0);
        let close = bar.close.0.to_f64().unwrap_or(0.0);
        let volume = bar.volume.0.to_f64().unwrap_or(0.0);

        let typical_price = (high + low + close) / 3.0;
        self.typical_price_sum += typical_price * volume;
        self.volume_sum += volume;

        let vwap = if self.volume_sum > 0.0 {
            self.typical_price_sum / self.volume_sum
        } else {
            typical_price
        };

        self.last = Some(vwap);
        Some(vwap)
    }

    fn value(&self) -> Option<f64> {
        self.last
    }

    fn reset(&mut self) {
        self.typical_price_sum = 0.0;
        self.volume_sum = 0.0;
        self.last = None;
    }
}
