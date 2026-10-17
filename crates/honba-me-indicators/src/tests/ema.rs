//! Unit tests for `crate::ema`.

use super::feed;
use crate::{Ema, Indicator};

#[test]
#[should_panic(expected = "EMA period must be positive")]
fn zero_period_panics() {
    let _ = Ema::new(0);
}

#[test]
fn seeds_with_the_sma_then_applies_alpha() {
    // Period 3 -> alpha = 2 / (3 + 1) = 0.5; seed = mean(1, 2, 3) = 2.
    let mut e = Ema::new(3);
    assert_eq!(e.period(), 3);
    assert_eq!(
        feed(&mut e, &[1.0, 2.0, 3.0, 4.0, 0.0]),
        [None, None, Some(2.0), Some(3.0), Some(1.5)]
    );
    assert_eq!(e.value(), Some(1.5));
}

#[test]
fn reset_requires_a_fresh_seed() {
    let mut e = Ema::new(2);
    feed(&mut e, &[1.0, 3.0, 5.0]);
    e.reset();
    assert_eq!(e.value(), None);
    assert_eq!(feed(&mut e, &[10.0, 20.0]), [None, Some(15.0)]);
}
