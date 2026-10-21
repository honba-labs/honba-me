//! OBV (On Balance Volume).
//!
//! OBV is a momentum indicator that uses volume flow to predict changes in
//! stock price. It adds volume on up days and subtracts volume on down days.
//!
//! # Formula
//!
//! ```text
//! OBV = OBV[1] + Volume if Close > Close[1]
//! OBV = OBV[1] - Volume if Close < Close[1]
//! OBV = OBV[1] if Close == Close[1]
//! ```

use honba_core::Bar;
use crate::indicator::Indicator;
use rust_decimal::prelude::ToPrimitive;

/// On Balance Volume (OBV).
#[derive(Clone, Debug)]
pub struct Obv {
    value: f64,
    prev_close: Option<f64>,
}

impl Obv {
    /// Creates a new OBV calculator.
    pub fn new() -> Self {
        Self {
            value: 0.0,
            prev_close: None,
        }
    }

    /// Returns the current OBV value.
    pub fn value(&self) -> Option<f64> {
        Some(self.value)
    }
}

impl Default for Obv {
    fn default() -> Self {
        Self::new()
    }
}

impl Indicator for Obv {
    type Input<'a> = &'a Bar;
    type Output = f64;

    fn update(&mut self, bar: &Bar) -> Option<f64> {
        let close = bar.close.0.to_f64().unwrap_or(0.0);
        let volume = bar.volume.0.to_f64().unwrap_or(0.0);

        match self.prev_close {
            None => {
                // First bar - OBV starts at 0
                self.prev_close = Some(close);
            }
            Some(prev_close) => {
                if close > prev_close {
                    self.value += volume;
                } else if close < prev_close {
                    self.value -= volume;
                }
                // If close == prev_close, OBV unchanged
            }
        }

        Some(self.value)
    }

    fn value(&self) -> Option<f64> {
        Some(self.value)
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.prev_close = None;
    }
}
