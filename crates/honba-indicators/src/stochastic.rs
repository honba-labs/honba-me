//! Stochastic oscillator (%K and %D).
//!
//! Raw `%K = 100 * (close - LL) / (HH - LL)` over `period` bars. `HH` is the
//! highest high and `LL` the lowest low in the window. `HH == LL` yields
//! `50.0` by convention (neutral). `%D` is the SMA of the last `d_period`
//! `%K` values, `None` until that SMA is primed.

use std::collections::VecDeque;

use honba_messages::Bar;

use crate::indicator::Indicator;

/// The value produced by [`Stochastic`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StochasticValue {
    /// Raw `%K`, `0..100` (flat window gives `50`).
    pub k: f64,
    /// Smoothed `%D`, an SMA of `%K` over `d_period`.
    pub d: Option<f64>,
}

/// Stochastic oscillator (%K and %D) over `period` bars and a `d_period` SMA.
///
///
/// Monotonic deques give O(1) amortised updates; the keepers of `k` values
/// and their running sum give O(1) for `d`.
///
/// ```
/// use honba_indicators::{Indicator, Stochastic};
/// use honba_messages::{Bar, BarAggregation, BarSpecification, BarType, Exchange, InstrumentId, PriceType, UnixNanos};
/// fn bar(h: f64, l: f64, c: f64) -> Bar {
///     let bt = BarType::new(InstrumentId::new("X", Exchange::new("NSE")), BarSpecification::new(1, BarAggregation::Minute, PriceType::Last));
///     Bar::new(bt, c, h, l, c, 1.0, UnixNanos::from_u64(1), UnixNanos::from_u64(1))
/// }
/// let mut s = Stochastic::new(3, 2);
/// s.update(&bar(10.0, 5.0, 7.0));
/// s.update(&bar(11.0, 6.0, 9.0));
/// let v = s.update(&bar(9.0, 4.0, 6.0)).unwrap();
/// // HH=11, LL=4, close=6 -> k = 100*(2/7) ~ 28.571
/// assert!((v.k - 28.571428).abs() < 1e-4);
/// assert!(v.d.is_none());
/// let v2 = s.update(&bar(12.0, 7.0, 10.0)).unwrap();
/// // Next k 75, so d = (28.571+75)/2
/// assert!((v2.d.unwrap() - 51.7857).abs() < 1e-3);
/// ```
#[derive(Clone, Debug)]
pub struct Stochastic {
    period: usize,
    d_period: usize,
    index: usize,
    highs: VecDeque<(usize, f64)>,
    lows: VecDeque<(usize, f64)>,
    ks: VecDeque<f64>,
    k_sum: f64,
    last: Option<StochasticValue>,
}

impl Stochastic {
    /// Creates a stochastic oscillator over `period` bars with a `d_period` SMA.
    ///
    /// # Panics
    ///
    /// Panics if either period is zero.
    pub fn new(period: usize, d_period: usize) -> Self {
        assert!(period > 0, "Stochastic period must be positive");
        assert!(d_period > 0, "Stochastic d_period must be positive");
        Self {
            period,
            d_period,
            index: 0,
            highs: VecDeque::new(),
            lows: VecDeque::new(),
            ks: VecDeque::new(),
            k_sum: 0.0,
            last: None,
        }
    }

    /// Returns the primary `%K` lookback.
    pub fn period(&self) -> usize {
        self.period
    }

    /// Returns the smoothing lookback for `%D`.
    pub fn d_period(&self) -> usize {
        self.d_period
    }

    fn push(&mut self, high: f64, low: f64, close: f64) -> Option<StochasticValue> {
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

        if idx + 1 < self.period {
            return None;
        }

        let hh = self.highs.front().unwrap().1;
        let ll = self.lows.front().unwrap().1;
        let denom = hh - ll;
        let k = if denom.abs() < f64::EPSILON {
            50.0
        } else {
            100.0 * (close - ll) / denom
        };

        self.ks.push_back(k);
        self.k_sum += k;
        if self.ks.len() > self.d_period {
            if let Some(old) = self.ks.pop_front() {
                self.k_sum -= old;
            }
        }

        let d = if self.ks.len() == self.d_period {
            Some(self.k_sum / self.d_period as f64)
        } else {
            None
        };

        let v = StochasticValue { k, d };
        self.last = Some(v);
        Some(v)
    }

    /// Feeds raw `high`/`low`/`close` without building a `Bar`.
    pub fn update_hlc(&mut self, high: f64, low: f64, close: f64) -> Option<StochasticValue> {
        self.push(high, low, close)
    }
}

impl Indicator for Stochastic {
    type Input<'a> = &'a Bar;
    type Output = StochasticValue;

    fn update(&mut self, bar: &Bar) -> Option<StochasticValue> {
        self.push(bar.high(), bar.low(), bar.close())
    }

    fn value(&self) -> Option<StochasticValue> {
        self.last
    }

    fn reset(&mut self) {
        self.index = 0;
        self.highs.clear();
        self.lows.clear();
        self.ks.clear();
        self.k_sum = 0.0;
        self.last = None;
    }
}
