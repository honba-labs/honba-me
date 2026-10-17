//! Moving Average Convergence Divergence.

use crate::ema::Ema;
use crate::indicator::Indicator;

/// The three series produced by [`Macd`].
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct MacdValue {
    /// The MACD line: `fast_ema - slow_ema`.
    pub macd: f64,
    /// The signal line: EMA of the MACD line.
    pub signal: f64,
    /// `macd - signal`.
    pub histogram: f64,
}

/// MACD: fast EMA minus slow EMA, with an EMA of that difference as the
/// signal line.
///
/// The first output appears once the slow EMA and the signal EMA have both
/// primed. With typical settings (12, 26, 9) that's after `26 + 9 - 1 = 34`
/// inputs.
///
/// ```
/// use honba_indicators::{Indicator, Macd};
///
/// let mut macd = Macd::new(2, 4, 2);
/// for x in 1..=20 {
///     macd.update(x as f64);
/// }
/// let v = macd.value().unwrap();
/// assert!(v.macd > 0.0);
/// assert!((v.histogram - (v.macd - v.signal)).abs() < 1e-12);
/// ```
#[derive(Clone, Debug)]
pub struct Macd {
    fast: Ema,
    slow: Ema,
    signal: Ema,
    last: Option<MacdValue>,
}

impl Macd {
    /// Creates a MACD with the given fast, slow, and signal periods.
    ///
    /// # Panics
    ///
    /// Panics if `fast >= slow` or if any period is zero.
    pub fn new(fast: usize, slow: usize, signal: usize) -> Self {
        assert!(
            fast > 0 && slow > 0 && signal > 0,
            "MACD periods must be positive"
        );
        assert!(
            fast < slow,
            "MACD fast period must be less than slow period"
        );
        Self {
            fast: Ema::new(fast),
            slow: Ema::new(slow),
            signal: Ema::new(signal),
            last: None,
        }
    }

    /// Returns the fast EMA period.
    pub fn fast_period(&self) -> usize {
        self.fast.period()
    }

    /// Returns the slow EMA period.
    pub fn slow_period(&self) -> usize {
        self.slow.period()
    }

    /// Returns the signal EMA period.
    pub fn signal_period(&self) -> usize {
        self.signal.period()
    }
}

impl Indicator for Macd {
    type Input<'a> = f64;
    type Output = MacdValue;

    fn update(&mut self, input: f64) -> Option<MacdValue> {
        // Both EMAs must see every input, regardless of whether the other
        // has primed yet. Combining them with `?` inside one expression
        // would short-circuit and starve the slow EMA during fast's warmup.
        let fast = self.fast.update(input);
        let slow = self.slow.update(input);
        let macd = fast? - slow?;

        let signal = self.signal.update(macd)?;
        let v = MacdValue {
            macd,
            signal,
            histogram: macd - signal,
        };
        self.last = Some(v);
        Some(v)
    }

    fn value(&self) -> Option<MacdValue> {
        self.last
    }

    fn reset(&mut self) {
        self.fast.reset();
        self.slow.reset();
        self.signal.reset();
        self.last = None;
    }
}
