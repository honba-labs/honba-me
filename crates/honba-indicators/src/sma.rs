//! Simple Moving Average.

use std::collections::VecDeque;

use crate::indicator::Indicator;

/// Most samples reserved up front by [`Sma::new`].
const MAX_PREALLOC: usize = 4096;

/// A simple moving average over the last `period` inputs.
///
/// Uses a ring buffer and a running sum, so each update is O(1).
///
/// ```
/// use honba_indicators::{Indicator, Sma};
///
/// let mut sma = Sma::new(3);
/// assert_eq!(sma.update(1.0), None);
/// assert_eq!(sma.update(2.0), None);
/// assert_eq!(sma.update(3.0), Some(2.0));
/// assert_eq!(sma.update(4.0), Some(3.0));
/// ```
#[derive(Clone, Debug)]
pub struct Sma {
    period: usize,
    buf: VecDeque<f64>,
    sum: f64,
    last: Option<f64>,
}

impl Sma {
    /// Creates an SMA over `period` samples.
    ///
    /// # Panics
    ///
    /// Panics if `period` is zero.
    pub fn new(period: usize) -> Self {
        assert!(period > 0, "SMA period must be positive");
        Self {
            period,
            // Capped so an absurd period cannot abort on allocation; the
            // buffer grows on demand beyond this.
            buf: VecDeque::with_capacity(period.min(MAX_PREALLOC)),
            sum: 0.0,
            last: None,
        }
    }

    /// Returns the configured period.
    pub fn period(&self) -> usize {
        self.period
    }
}

impl Indicator for Sma {
    type Input<'a> = f64;
    type Output = f64;

    fn update(&mut self, input: f64) -> Option<f64> {
        self.buf.push_back(input);
        self.sum += input;
        if self.buf.len() > self.period {
            if let Some(old) = self.buf.pop_front() {
                self.sum -= old;
            }
        }
        if self.buf.len() == self.period {
            let v = self.sum / self.period as f64;
            self.last = Some(v);
            Some(v)
        } else {
            None
        }
    }

    fn value(&self) -> Option<f64> {
        self.last
    }

    fn reset(&mut self) {
        self.buf.clear();
        self.sum = 0.0;
        self.last = None;
    }
}
