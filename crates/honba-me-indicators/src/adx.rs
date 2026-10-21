//! ADX (Average Directional Index).
//!
//! ADX measures trend strength regardless of direction. It is derived from
//! +DI (Positive Directional Index) and -DI (Negative Directional Index).
//!
//! # Formula
//!
//! ```text
//! +DM = High - High[1] if High - High[1] > Low[1] - Low and High - High[1] > 0, else 0
//! -DM = Low[1] - Low if Low[1] - Low > High - High[1] and Low[1] - Low > 0, else 0
//! TR = max(High - Low, |High - Close[1]|, |Low - Close[1]|)
//! +DI = 100 * Smoothed(+DM) / Smoothed(TR)
//! -DI = 100 * Smoothed(-DM) / Smoothed(TR)
//! DX = 100 * |+DI - -DI| / (+DI + -DI)
//! ADX = Smoothed(DX)
//! ```

use honba_core::Bar;
use crate::indicator::Indicator;
use rust_decimal::prelude::ToPrimitive;

/// ADX (Average Directional Index) with +DI and -DI.
///
/// Returns the ADX value as output. Use `plus_di()`, `minus_di()`, and `dx()`
/// to access intermediate values.
#[derive(Clone, Debug)]
pub struct Adx {
    period: usize,
    highs: Vec<f64>,
    lows: Vec<f64>,
    closes: Vec<f64>,
    plus_di: Vec<f64>,
    minus_di: Vec<f64>,
    dx: Vec<f64>,
    adx: Vec<f64>,
    atr_sum: f64,
    plus_dm_sum: f64,
    minus_dm_sum: f64,
    dx_sum: f64,
    initialized: bool,
}

impl Adx {
    /// Creates a new ADX with the given period (typically 14).
    pub fn new(period: usize) -> Self {
        Self {
            period,
            highs: Vec::with_capacity(period + 1),
            lows: Vec::with_capacity(period + 1),
            closes: Vec::with_capacity(period + 1),
            plus_di: Vec::with_capacity(period),
            minus_di: Vec::with_capacity(period),
            dx: Vec::with_capacity(period),
            adx: Vec::with_capacity(period),
            atr_sum: 0.0,
            plus_dm_sum: 0.0,
            minus_dm_sum: 0.0,
            dx_sum: 0.0,
            initialized: false,
        }
    }

    /// Returns the configured period.
    pub fn period(&self) -> usize {
        self.period
    }

    /// Returns the current +DI value.
    pub fn plus_di(&self) -> Option<f64> {
        self.plus_di.last().copied()
    }

    /// Returns the current -DI value.
    pub fn minus_di(&self) -> Option<f64> {
        self.minus_di.last().copied()
    }

    /// Returns the current DX value.
    pub fn dx(&self) -> Option<f64> {
        self.dx.last().copied()
    }
}

impl Indicator for Adx {
    type Input<'a> = &'a Bar;
    type Output = f64;

    fn update(&mut self, bar: &Bar) -> Option<f64> {
        let high = bar.high.0.to_f64().unwrap_or(0.0);
        let low = bar.low.0.to_f64().unwrap_or(0.0);
        let close = bar.close.0.to_f64().unwrap_or(0.0);

        self.highs.push(high);
        self.lows.push(low);
        self.closes.push(close);

        if self.highs.len() < 2 {
            return None;
        }

        let prev_high = self.highs[self.highs.len() - 2];
        let prev_low = self.lows[self.lows.len() - 2];
        let prev_close = self.closes[self.closes.len() - 2];

        // Calculate +DM and -DM
        let up_move = high - prev_high;
        let down_move = prev_low - low;

        let plus_dm = if up_move > down_move && up_move > 0.0 {
            up_move
        } else {
            0.0
        };
        let minus_dm = if down_move > up_move && down_move > 0.0 {
            down_move
        } else {
            0.0
        };

        // True Range
        let tr1 = high - low;
        let tr2 = (high - prev_close).abs();
        let tr3 = (low - prev_close).abs();
        let tr = tr1.max(tr2).max(tr3);

        // Initialize sums
        if self.highs.len() <= self.period + 1 {
            self.atr_sum += tr;
            self.plus_dm_sum += plus_dm;
            self.minus_dm_sum += minus_dm;

            if self.highs.len() == self.period + 1 {
                // Initial values
                let atr = self.atr_sum / self.period as f64;
                let plus_di_val = if atr > 0.0 {
                    100.0 * self.plus_dm_sum / atr
                } else {
                    0.0
                };
                let minus_di_val = if atr > 0.0 {
                    100.0 * self.minus_dm_sum / atr
                } else {
                    0.0
                };
                let dx = if plus_di_val + minus_di_val > 0.0 {
                    100.0 * (plus_di_val - minus_di_val).abs() / (plus_di_val + minus_di_val)
                } else {
                    0.0
                };

                self.plus_di.push(plus_di_val);
                self.minus_di.push(minus_di_val);
                self.dx.push(dx);
                self.dx_sum += dx;
                self.adx.push(dx);
                self.initialized = true;

                return Some(dx);
            }
            return None;
        }

        // Wilder smoothing for subsequent values
        let atr = (self.atr_sum * (self.period as f64 - 1.0) + tr) / self.period as f64;
        self.atr_sum = atr;

        let plus_di_val = if atr > 0.0 {
            100.0 * (self.plus_dm_sum * (self.period as f64 - 1.0) + plus_dm) / (atr * self.period as f64)
        } else {
            0.0
        };
        let minus_di_val = if atr > 0.0 {
            100.0 * (self.minus_dm_sum * (self.period as f64 - 1.0) + minus_dm) / (atr * self.period as f64)
        } else {
            0.0
        };

        let dx = if plus_di_val + minus_di_val > 0.0 {
            100.0 * (plus_di_val - minus_di_val).abs() / (plus_di_val + minus_di_val)
        } else {
            0.0
        };

        self.plus_di.push(plus_di_val);
        self.minus_di.push(minus_di_val);
        self.dx.push(dx);

        // ADX = smoothed DX
        let adx_value = if self.adx.len() < self.period {
            // Initial ADX = average of first `period` DX values
            self.dx_sum += dx;
            if self.dx.len() == self.period {
                self.dx_sum / self.period as f64
            } else {
                0.0
            }
        } else {
            // Wilder smoothing: (PrevADX * (period - 1) + CurrentDX) / period
            let prev_adx = *self.adx.last().unwrap_or(&0.0);
            (prev_adx * (self.period as f64 - 1.0) + dx) / self.period as f64
        };

        self.plus_di.push(plus_di_val);
        self.minus_di.push(minus_di_val);
        self.dx.push(dx);
        self.adx.push(adx_value);

        Some(adx_value)
    }

    fn value(&self) -> Option<f64> {
        self.adx.last().copied()
    }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
        self.closes.clear();
        self.plus_di.clear();
        self.minus_di.clear();
        self.dx.clear();
        self.adx.clear();
        self.atr_sum = 0.0;
        self.plus_dm_sum = 0.0;
        self.minus_dm_sum = 0.0;
        self.dx_sum = 0.0;
        self.initialized = false;
    }
}
