//! Unit tests for `crate::bollinger`.

use super::{assert_close, feed};
use crate::{BollingerBands, Indicator};

#[test]
#[should_panic(expected = "Bollinger period must be positive")]
fn zero_period_panics() {
    let _ = BollingerBands::new(0, 2.0);
}

#[test]
#[should_panic(expected = "Bollinger k must be non-negative")]
fn negative_k_panics() {
    let _ = BollingerBands::new(2, -0.1);
}

#[test]
fn bands_use_population_standard_deviation() {
    let mut b = BollingerBands::new(2, 2.0);
    assert_eq!((b.period(), b.k()), (2, 2.0));
    let v = feed(&mut b, &[1.0, 3.0])[1].unwrap();
    // mean 2, population std 1.
    assert_close(v.middle, 2.0);
    assert_close(v.upper, 4.0);
    assert_close(v.lower, 0.0);
}

#[test]
fn zero_k_collapses_the_bands_onto_the_middle() {
    let mut b = BollingerBands::new(3, 0.0);
    let v = feed(&mut b, &[1.0, 5.0, 9.0])[2].unwrap();
    assert_eq!((v.upper, v.lower), (v.middle, v.middle));
}

#[test]
fn reset_clears_the_window() {
    let mut b = BollingerBands::new(2, 1.0);
    feed(&mut b, &[1.0, 2.0]);
    b.reset();
    assert_eq!(b.value(), None);
    assert_eq!(b.update(10.0), None);
}
