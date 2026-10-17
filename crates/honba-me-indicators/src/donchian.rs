//! Donchian Channel.
//!
//! The channel over `period` bars is the highest high and lowest low in the
//! window; the middle is their midpoint.

use std::collections::VecDeque;

use honba_messages::Bar;

use crate::indicator::Indicator;

/// The value produced by [`Donchian`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DonchianValue {
    /// Highest high over the window.
    pub upper: f64,
    /// Lowest low over the window.
    pub lower: f64,
    /// Midpoint `(upper + lower) / 2`.
    pub middle: f64,
}

/// Donchian Channel over `period` bars.
///
/// Each update is O(1) amortised via monotonic deques for the rolling
/// maximum of highs and minimum of lows.
///
/// ```
/// use honba_indicators::{Indicator, Donchian};
/// use honba_messages::{Bar, BarAggregation, BarSpecification, BarType, Exchange, InstrumentId, PriceType, UnixNanos};
/// fn bar(h: f64, l: f64) -> Bar {
///     let bt = BarType::new(InstrumentId::new("X", Exchange::new("NSE")), BarSpecification::new(1, BarAggregation::Minute, PriceType::Last));
///     Bar::new(bt, l, h, l, l, 1.0, UnixNanos::from_u64(1), UnixNanos::from_u64(1))
/// }
/// let mut d = Donchian::new(3);
/// assert_eq!(d.update(&bar(10.0, 8.0)), None);
/// assert_eq!(d.update(&bar(11.0, 9.0)), None);
/// let v = d.update(&bar(9.0, 7.0)).unwrap();
/// assert_eq!(v.upper, 11.0);
/// assert_eq!(v.lower, 7.0);
/// ```
#[derive(Clone, Debug)]
pub struct Donchian {
    period: usize,
    index: usize,
    highs: VecDeque<(usize, f64)>,
    lows: VecDeque<(usize, f64)>,
    last: Option<DonchianValue>,
}

impl Donchian {
    /// Creates a Donchian Channel over `period` bars.
    ///
    /// # Panics
    ///
    /// Panics if `period` is zero.
    pub fn new(period: usize) -> Self {
        assert!(period > 0, "Donchian period must be positive");
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

    fn push(&mut self, high: f64, low: f64) -> Option<DonchianValue> {
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
            let upper = self.highs.front().unwrap().1;
            let lower = self.lows.front().unwrap().1;
            let v = DonchianValue {
                upper,
                lower,
                middle: (upper + lower) * 0.5,
            };
            self.last = Some(v);
            Some(v)
        } else {
            None
        }
    }

    /// Feeds raw `high`/`low` without building a `Bar`.
    pub fn update_hl(&mut self, high: f64, low: f64) -> Option<DonchianValue> {
        self.push(high, low)
    }
}

impl Indicator for Donchian {
    type Input<'a> = &'a Bar;
    type Output = DonchianValue;

    fn update(&mut self, bar: &Bar) -> Option<DonchianValue> {
        self.push(bar.high(), bar.low())
    }

    fn value(&self) -> Option<DonchianValue> {
        self.last
    }

    fn reset(&mut self) {
        self.index = 0;
        self.highs.clear();
        self.lows.clear();
        self.last = None;
    }
}
