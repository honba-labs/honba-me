//! Average True Range (Wilder).

use honba_messages::Bar;

use crate::indicator::Indicator;

/// Wilder's Average True Range over `period` bars.
///
/// True range for a bar is:
///
/// `max(high - low, |high - prev_close|, |low - prev_close|)`
///
/// The first bar has no previous close, so its true range is just
/// `high - low`. The first ATR value is the SMA of the first `period` true
/// ranges; subsequent values use Wilder smoothing.
///
/// ```
/// use honba_indicators::{Atr, Indicator};
/// use honba_messages::{Bar, BarAggregation, BarSpecification, BarType, InstrumentId, PriceType, UnixNanos, Exchange};
///
/// fn bar(h: f64, l: f64, c: f64) -> Bar {
///     let bt = BarType::new(
///         InstrumentId::new("X", Exchange::new("NSE")),
///         BarSpecification::new(1, BarAggregation::Day, PriceType::Last),
///     );
///     Bar::new(bt, c, h, l, c, 1.0, UnixNanos::from_u64(1), UnixNanos::from_u64(1))
/// }
///
/// let mut atr = Atr::new(2);
/// assert_eq!(atr.update(&bar(10.0, 8.0, 9.0)), None);
/// assert_eq!(atr.update(&bar(11.0, 9.0, 10.0)), Some(2.0));
/// ```
#[derive(Clone, Debug)]
pub struct Atr {
    period: usize,
    prev_close: Option<f64>,
    seed_trs: Vec<f64>,
    atr: Option<f64>,
}

impl Atr {
    /// Creates an ATR over `period` bars.
    ///
    /// # Panics
    ///
    /// Panics if `period` is zero.
    pub fn new(period: usize) -> Self {
        assert!(period > 0, "ATR period must be positive");
        Self {
            period,
            prev_close: None,
            seed_trs: Vec::with_capacity(period),
            atr: None,
        }
    }

    /// Returns the configured period.
    pub fn period(&self) -> usize {
        self.period
    }

    /// Feeds one bar given as raw `high`, `low`, `close` and returns the ATR once warmed up.
    ///
    /// Identical to [`Indicator::update`] on a bar with those fields; it exists so callers that
    /// only hold price arrays (e.g. the WASM surface) need not build a `Bar`.
    pub fn update_hlc(&mut self, high: f64, low: f64, close: f64) -> Option<f64> {
        let tr = Self::true_range(high, low, self.prev_close);
        self.prev_close = Some(close);
        self.push_tr(tr)
    }

    fn push_tr(&mut self, tr: f64) -> Option<f64> {
        if self.atr.is_none() {
            self.seed_trs.push(tr);
            if self.seed_trs.len() == self.period {
                let n = self.period as f64;
                let seed = self.seed_trs.iter().sum::<f64>() / n;
                self.atr = Some(seed);
                return Some(seed);
            }
            return None;
        }

        let n = self.period as f64;
        let prev = self.atr.unwrap();
        let next = (prev * (n - 1.0) + tr) / n;
        self.atr = Some(next);
        Some(next)
    }

    fn true_range(high: f64, low: f64, prev_close: Option<f64>) -> f64 {
        let hl = high - low;
        match prev_close {
            None => hl,
            Some(pc) => {
                let hc = (high - pc).abs();
                let lc = (low - pc).abs();
                hl.max(hc).max(lc)
            }
        }
    }
}

impl Indicator for Atr {
    type Input<'a> = &'a Bar;
    type Output = f64;

    fn update(&mut self, bar: &Bar) -> Option<f64> {
        self.update_hlc(bar.high(), bar.low(), bar.close())
    }

    fn value(&self) -> Option<f64> {
        self.atr
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.seed_trs.clear();
        self.atr = None;
    }
}
