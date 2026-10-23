//! Supertrend indicator.
//!
//! Supertrend is an ATR-based trend-following indicator that provides dynamic
//! support/resistance bands. It flips direction when price crosses the band,
//! making it ideal for trending markets.
//!
//! # Formula
//!
//! ```text
//! HL2 = (High + Low) / 2
//! Basic Upper Band = HL2 + (Multiplier * ATR)
//! Basic Lower Band = HL2 - (Multiplier * ATR)
//! Final Upper Band = min(Basic Upper Band, Previous Final Upper Band) if Close[1] > Previous Final Upper Band else Basic Upper Band
//! Final Lower Band = max(Basic Lower Band, Previous Final Lower Band) if Close[1] < Previous Final Lower Band else Basic Lower Band
//! Supertrend = Final Upper Band if Close <= Final Upper Band else Final Lower Band
//! ```

use crate::indicator::Indicator;
use honba_core::Bar;
use rust_decimal::prelude::ToPrimitive;

/// Supertrend indicator.
///
/// Parameters:
/// - `period`: ATR period (default: 10)
/// - `multiplier`: ATR multiplier for band width (default: 3.0)
#[derive(Clone, Debug)]
pub struct Supertrend {
    period: usize,
    multiplier: f64,
    atr: crate::Atr,
    hl2_values: Vec<f64>,
    close_values: Vec<f64>,
    upper_band: Vec<f64>,
    lower_band: Vec<f64>,
    supertrend: Vec<f64>,
    in_uptrend: Vec<bool>,
}

impl Supertrend {
    /// Creates a new Supertrend with the given period and multiplier.
    pub fn new(period: usize, multiplier: f64) -> Self {
        Self {
            period,
            multiplier,
            atr: crate::Atr::new(period),
            hl2_values: Vec::with_capacity(period),
            close_values: Vec::with_capacity(period),
            upper_band: Vec::with_capacity(period),
            lower_band: Vec::with_capacity(period),
            supertrend: Vec::with_capacity(period),
            in_uptrend: Vec::with_capacity(period),
        }
    }

    /// Returns the configured period.
    pub fn period(&self) -> usize {
        self.period
    }

    /// Returns the configured multiplier.
    pub fn multiplier(&self) -> f64 {
        self.multiplier
    }

    /// Returns the current Supertrend value.
    pub fn value(&self) -> Option<f64> {
        self.supertrend.last().copied()
    }

    /// Returns true if currently in uptrend.
    pub fn is_uptrend(&self) -> bool {
        self.in_uptrend.last().copied().unwrap_or(true)
    }
}

impl Indicator for Supertrend {
    type Input<'a> = &'a Bar;
    type Output = f64;

    fn update(&mut self, bar: &Bar) -> Option<f64> {
        let hl2 = (bar.high.0.to_f64().unwrap_or(0.0) + bar.low.0.to_f64().unwrap_or(0.0)) / 2.0;
        let close = bar.close.0.to_f64().unwrap_or(0.0);

        // Update ATR
        let _atr_value = self.atr.update(bar);

        self.hl2_values.push(hl2);
        self.close_values.push(close);

        if self.atr.value().is_none() {
            self.upper_band.push(hl2);
            self.lower_band.push(hl2);
            self.supertrend.push(hl2);
            self.in_uptrend.push(true);
            return None;
        }

        let atr_val = self.atr.value().unwrap();
        let basic_upper = hl2 + self.multiplier * atr_val;
        let basic_lower = hl2 - self.multiplier * atr_val;

        let prev_upper = *self.upper_band.last().unwrap_or(&hl2);
        let prev_lower = *self.lower_band.last().unwrap_or(&hl2);
        let prev_close = *self.close_values.last().unwrap_or(&close);
        let prev_supertrend = *self.supertrend.last().unwrap_or(&hl2);
        let _prev_in_uptrend = *self.in_uptrend.last().unwrap_or(&true);

        let mut final_upper = basic_upper;
        let mut final_lower = basic_lower;

        if prev_close > prev_upper {
            final_upper = basic_upper.min(prev_upper);
        }

        if prev_close < prev_lower {
            final_lower = basic_lower.max(prev_lower);
        }

        let (st, is_uptrend) = if prev_supertrend == final_upper {
            if close <= final_upper {
                (final_upper, true)
            } else {
                (final_lower, false)
            }
        } else if close >= final_lower {
            (final_lower, false)
        } else {
            (final_upper, true)
        };

        self.upper_band.push(final_upper);
        self.lower_band.push(final_lower);
        self.supertrend.push(st);
        self.in_uptrend.push(is_uptrend);

        Some(st)
    }

    fn value(&self) -> Option<f64> {
        self.supertrend.last().copied()
    }

    fn reset(&mut self) {
        self.atr.reset();
        self.hl2_values.clear();
        self.close_values.clear();
        self.upper_band.clear();
        self.lower_band.clear();
        self.supertrend.clear();
        self.in_uptrend.clear();
    }
}
