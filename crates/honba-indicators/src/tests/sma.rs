//! Unit tests for `crate::sma`.

use super::feed;
use crate::{Indicator, Sma};

#[test]
#[should_panic(expected = "SMA period must be positive")]
fn zero_period_panics() {
    let _ = Sma::new(0);
}

#[test]
fn reports_its_period_and_readiness() {
    let mut s = Sma::new(2);
    assert_eq!(s.period(), 2);
    assert!(!s.is_ready());
    s.update(1.0);
    assert!(!s.is_ready());
    s.update(3.0);
    assert!(s.is_ready());
    assert_eq!(s.value(), Some(2.0));
}

#[test]
fn period_one_echoes_the_input() {
    let mut s = Sma::new(1);
    assert_eq!(
        feed(&mut s, &[4.0, 7.0, -1.0]),
        [Some(4.0), Some(7.0), Some(-1.0)]
    );
}

#[test]
fn window_slides_over_the_last_period_inputs() {
    let mut s = Sma::new(3);
    assert_eq!(
        feed(&mut s, &[1.0, 2.0, 3.0, 4.0, 5.0]),
        [None, None, Some(2.0), Some(3.0), Some(4.0)]
    );
}

#[test]
fn huge_period_does_not_preallocate() {
    // Used to abort the process: VecDeque::with_capacity(2**40).
    let mut s = Sma::new(1 << 40);
    assert_eq!(s.update(1.0), None);
    assert_eq!(s.period(), 1 << 40);
}
