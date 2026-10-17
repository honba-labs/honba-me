//! Williams %R.
//!
//! `%R = -100 * (HH - close) / (HH - LL)` over `period` bars. `HH` is the
//! highest high and `LL` the lowest low in the window. A flat window
//! (`HH == LL`) yields `-50.0` by convention.

use std::collections::VecDeque;

use honba_messages::Bar;

use crate::indicator::Indicator;

/// Williams %R over `period` bars, in `[-100, 0]`.
///
/// Monotonic deques give O(1) amortised updates.
///
/// ```
/// use honba_indicators::{Indicator, WilliamsR};
/// use honba_messages::{Bar, BarAggregation, BarSpecification, BarType, Exchange, InstrumentId, PriceType, UnixNanos};
/// fn bar(h: f64, l: f64, c: f64) -> Bar {
///     let bt = BarType::new(InstrumentId::new("X", Exchange::new("NSE")), BarSpecification::new(1, BarAggregation::Minute, PriceType::Last));
///     Bar::new(bt, c, h, l, c, 1.0, UnixNanos::from_u64(1), UnixNanos::from_u64(1))
/// }
/// let mut w = WilliamsR::new(3);
/// w.update(&bar(10.0, 5.0, 9.0));
/// w.update(&bar(12.0, 6.0, 11.0));
/// let r = w.update(&bar(11.0, 4.0, 6.0)).unwrap();
/// // HH=12, LL=4, close=6 -> -100*(6/8) = -75
/// assert!((r - (-75.0)).abs() < 1e-9);
/// ```
#[derive(Clone, Debug)]
pub struct WilliamsR {
    period: usize,
    index: usize,
    highs: VecDeque<(usize, f64)>,
    lows: VecDeque<(usize, f64)>,
    last: Option<f64>,
}

impl WilliamsR {
    /// Creates a Williams %R over `period` bars.
    ///
    /// # Panics
    ///
    /// Panics if `period` is zero.
    pub fn new(period: usize) -> Self {
        assert!(period > 0, "WilliamsR period must be positive");
        Self {
            period,
            index: 0,
            highs: VecDeque::new(),
            lows: VecDeque::new(),
            last: None,
        }
    }

    /// Returns the configured period.
    pub fn period(&self) -> usize {
        self.period
    }

    fn push(&mut self, high: f64, low: f64, close: f64) -> Option<f64> {
        let idx = self.index;
        self.index += 1;

        while self.highs.back().is_some_and(|(_, v)| *v <= high) {
            self.highs.pop_back();
        }
        self.highs.push_back((idx, high));

        while self.lows.back().is_some_and(|(_, v)| *v >= low) {
            self.lows.pop_back();
        }
        self.lows.push_back((idx, low));

        let cutoff = idx.saturating_sub(self.period - 1);
        while self.highs.front().is_some_and(|(i, _)| *i < cutoff) {
            self.highs.pop_front();
        }
        while self.lows.front().is_some_and(|(i, _)| *i < cutoff) {
            self.lows.pop_front();
        }

        if idx + 1 >= self.period {
            let hh = self.highs.front().unwrap().1;
            let ll = self.lows.front().unwrap().1;
            let denom = hh - ll;
            let v = if denom.abs() < f64::EPSILON {
                -50.0
            } else {
                -100.0 * (hh - close) / denom
            };
            self.last = Some(v);
            Some(v)
        } else {
            None
        }
    }

    /// Feeds raw `high`/`low`/`close` without building a `Bar`.
    pub fn update_hlc(&mut self, high: f64, low: f64, close: f64) -> Option<f64> {
        self.push(high, low, close)
    }
}

impl Indicator for WilliamsR {
    type Input<'a> = &'a Bar;
    type Output = f64;

    fn update(&mut self, bar: &Bar) -> Option<f64> {
        self.push(bar.high(), bar.low(), bar.close())
    }

    fn value(&self) -> Option<f64> {
        self.last
    }

    fn reset(&mut self) {
        self.index = 0;
        self.highs.clear();
        self.lows.clear();
        self.last = None;
    }
}
