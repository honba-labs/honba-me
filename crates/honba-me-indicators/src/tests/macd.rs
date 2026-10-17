//! Unit tests for `crate::macd`.

use super::{assert_close, feed};
use crate::Macd;

#[test]
#[should_panic(expected = "MACD periods must be positive")]
fn zero_period_panics() {
    let _ = Macd::new(0, 3, 2);
}

#[test]
fn reports_its_periods() {
    let m = Macd::new(2, 4, 3);
    assert_eq!(
        (m.fast_period(), m.slow_period(), m.signal_period()),
        (2, 4, 3)
    );
}

#[test]
fn first_value_arrives_after_slow_plus_signal_minus_one_inputs() {
    let mut m = Macd::new(2, 3, 2);
    let out = feed(&mut m, &[1.0, 2.0, 3.0, 4.0, 5.0]);
    let first = out.iter().position(Option::is_some).unwrap();
    assert_eq!(first, 3 + 2 - 2);
}

#[test]
fn constant_input_converges_to_zero() {
    let mut m = Macd::new(2, 3, 2);
    let v = feed(&mut m, &[7.0; 6]).last().copied().flatten().unwrap();
    assert_close(v.macd, 0.0);
    assert_close(v.signal, 0.0);
    assert_close(v.histogram, 0.0);
}
