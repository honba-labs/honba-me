//! Central Pivot Range (CPR).
//!
//! CPR is a popular intraday indicator in Indian markets that identifies key
//! support and resistance levels based on the previous day's OHLC.
//!
//! # Formula
//!
//! ```text
//! Pivot = (High + Low + Close) / 3
//! BC (Bottom Central Pivot) = (High + Low) / 2
//! TC (Top Central Pivot) = 2 * Pivot - BC
//! CPR Width = TC - BC
//! ```

use honba_core::Bar;
use crate::indicator::Indicator;
use rust_decimal::prelude::ToPrimitive;

/// Central Pivot Range (CPR) indicator.
///
/// CPR provides three key levels:
/// - Pivot: The central pivot point
/// - BC (Bottom Central Pivot): Lower boundary
/// - TC (Top Central Pivot): Upper boundary
#[derive(Clone, Debug)]
pub struct Cpr {
    pivot: Option<f64>,
    bc: Option<f64>,
    tc: Option<f64>,
    width: Option<f64>,
}

impl Cpr {
    /// Creates a new CPR calculator.
    pub fn new() -> Self {
        Self {
            pivot: None,
            bc: None,
            tc: None,
            width: None,
        }
    }

    /// Returns the current pivot value.
    pub fn pivot(&self) -> Option<f64> {
        self.pivot
    }

    /// Returns the bottom central pivot (BC).
    pub fn bc(&self) -> Option<f64> {
        self.bc
    }

    /// Returns the top central pivot (TC).
    pub fn tc(&self) -> Option<f64> {
        self.tc
    }

    /// Returns the CPR width (TC - BC).
    pub fn width(&self) -> Option<f64> {
        self.width
    }
}

impl Default for Cpr {
    fn default() -> Self {
        Self::new()
    }
}

impl Indicator for Cpr {
    type Input<'a> = &'a Bar;
    type Output = f64; // Returns the pivot value

    fn update(&mut self, bar: &Bar) -> Option<f64> {
        let high = bar.high.0.to_f64().unwrap_or(0.0);
        let low = bar.low.0.to_f64().unwrap_or(0.0);
        let close = bar.close.0.to_f64().unwrap_or(0.0);

        let pivot = (high + low + close) / 3.0;
        let bc = (high + low) / 2.0;
        let tc = 2.0 * pivot - bc;
        let width = tc - bc;

        self.pivot = Some(pivot);
        self.bc = Some(bc);
        self.tc = Some(tc);
        self.width = Some(width);

        Some(pivot)
    }

    fn value(&self) -> Option<f64> {
        self.pivot
    }

    fn reset(&mut self) {
        self.pivot = None;
        self.bc = None;
        self.tc = None;
        self.width = None;
    }
}
