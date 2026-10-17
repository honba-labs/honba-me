//! Unit tests for `crate::rsi`.

use super::{assert_close, feed};
use crate::{Indicator, Rsi};

#[test]
#[should_panic(expected = "RSI period must be positive")]
fn zero_period_panics() {
    let _ = Rsi::new(0);
}

#[test]
fn seeds_from_simple_averages_then_uses_wilder_smoothing() {
    let mut r = Rsi::new(2);
    assert_eq!(r.period(), 2);
    let out = feed(&mut r, &[1.0, 2.0, 1.0, 3.0]);
    assert_eq!(out[..2], [None, None]);
    // Seed: avg gain 0.5, avg loss 0.5 -> RSI 50.
    assert_close(out[2].unwrap(), 50.0);
    // Wilder: gain (0.5 + 2) / 2 = 1.25, loss (0.5 + 0) / 2 = 0.25 -> RS 5.
    assert_close(out[3].unwrap(), 100.0 - 100.0 / 6.0);
    assert_eq!(r.value(), out[3]);
}

#[test]
fn reset_forgets_the_previous_price() {
    let mut r = Rsi::new(1);
    feed(&mut r, &[1.0, 2.0]);
    assert!(r.is_ready());
    r.reset();
    assert!(!r.is_ready());
    // First input after reset only records the price.
    assert_eq!(r.update(5.0), None);
    assert_eq!(r.update(4.0), Some(0.0));
}
