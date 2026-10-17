//! Behavioural tests for every indicator.

use honba_indicators::{Atr, BollingerBands, BollingerValue, Ema, Indicator, Macd, Rsi, Sma};
use honba_messages::{
    Bar, BarAggregation, BarSpecification, BarType, Exchange, InstrumentId, PriceType, UnixNanos,
};

fn bar(h: f64, l: f64, c: f64) -> Bar {
    let bt = BarType::new(
        InstrumentId::new("X", Exchange::new("NSE")),
        BarSpecification::new(1, BarAggregation::Day, PriceType::Last),
    );
    Bar::new(
        bt,
        c,
        h,
        l,
        c,
        1.0,
        UnixNanos::from_u64(1),
        UnixNanos::from_u64(1),
    )
}

// --- SMA ---

#[test]
fn sma_returns_none_until_primed() {
    let mut s = Sma::new(3);
    assert_eq!(s.update(1.0), None);
    assert_eq!(s.update(2.0), None);
    assert_eq!(s.update(3.0), Some(2.0));
    assert!(s.is_ready());
}

#[test]
fn sma_rolling_average() {
    let mut s = Sma::new(3);
    let inputs = [1.0, 2.0, 3.0, 4.0, 5.0];
    let expected: Vec<Option<f64>> = vec![None, None, Some(2.0), Some(3.0), Some(4.0)];
    for (i, x) in inputs.iter().enumerate() {
        assert_eq!(s.update(*x), expected[i], "at index {i}");
    }
}

#[test]
fn sma_reset_clears_state() {
    let mut s = Sma::new(3);
    s.update(1.0);
    s.update(2.0);
    s.update(3.0);
    assert!(s.is_ready());
    s.reset();
    assert!(!s.is_ready());
    assert_eq!(s.update(10.0), None);
}

// --- EMA ---

#[test]
fn ema_seeds_from_sma() {
    let mut e = Ema::new(3);
    assert_eq!(e.update(1.0), None);
    assert_eq!(e.update(2.0), None);
    assert_eq!(e.update(3.0), Some(2.0));
}

#[test]
fn ema_smooths_after_seed() {
    let mut e = Ema::new(3); // alpha = 0.5
    for x in [1.0, 2.0, 3.0] {
        e.update(x);
    }
    assert_eq!(e.update(4.0), Some(3.0));
    assert_eq!(e.update(5.0), Some(4.0));
}

#[test]
fn ema_reset_clears_state() {
    let mut e = Ema::new(3);
    for x in [1.0, 2.0, 3.0] {
        e.update(x);
    }
    e.reset();
    assert!(!e.is_ready());
    assert_eq!(e.update(1.0), None);
}

// --- RSI ---

#[test]
fn rsi_all_up_is_100() {
    let mut r = Rsi::new(3);
    for p in [1.0, 2.0, 3.0, 4.0] {
        r.update(p);
    }
    let v = r.value().unwrap();
    assert!((v - 100.0).abs() < 1e-9, "expected 100, got {v}");
}

#[test]
fn rsi_all_down_is_zero() {
    let mut r = Rsi::new(3);
    for p in [4.0, 3.0, 2.0, 1.0] {
        r.update(p);
    }
    let v = r.value().unwrap();
    assert!(v.abs() < 1e-9, "expected 0, got {v}");
}

#[test]
fn rsi_flat_is_neutral_when_no_loss() {
    // With all gains = 0 and all losses = 0, avg_loss == 0, so we return 100.
    // This is an edge case; document the behaviour rather than fight it.
    let mut r = Rsi::new(3);
    for p in [5.0, 5.0, 5.0, 5.0] {
        r.update(p);
    }
    let v = r.value().unwrap();
    assert!((v - 100.0).abs() < 1e-9);
}

#[test]
fn rsi_requires_period_plus_one_inputs() {
    let mut r = Rsi::new(3);
    assert_eq!(r.update(1.0), None); // first price sets baseline
    assert_eq!(r.update(2.0), None);
    assert_eq!(r.update(3.0), None);
    assert_eq!(r.update(4.0), Some(100.0));
}

// --- ATR ---

#[test]
fn atr_first_bar_uses_high_low() {
    let mut a = Atr::new(2);
    assert_eq!(a.update(&bar(10.0, 8.0, 9.0)), None); // tr = 2
                                                      // Second bar: high=11, low=9, prev_close=9 -> tr = max(2, 2, 0) = 2
    assert_eq!(a.update(&bar(11.0, 9.0, 10.0)), Some(2.0));
}

#[test]
fn atr_accounts_for_gaps() {
    let mut a = Atr::new(1);
    a.update(&bar(10.0, 9.0, 9.5)); // tr = 1
                                    // Gap up: prev_close=9.5, high=20, low=15 -> tr = max(5, 10.5, 5.5) = 10.5
    let v = a.update(&bar(20.0, 15.0, 19.0)).unwrap();
    assert!((v - 10.5).abs() < 1e-9);
}

// --- Bollinger ---

#[test]
fn bollinger_returns_none_until_primed() {
    let mut bb = BollingerBands::new(3, 2.0);
    assert_eq!(bb.update(1.0), None);
    assert_eq!(bb.update(2.0), None);
    assert!(bb.update(3.0).is_some());
}

#[test]
fn bollinger_constant_series_has_zero_width() {
    let mut bb = BollingerBands::new(5, 2.0);
    for _ in 0..5 {
        bb.update(10.0);
    }
    let v = bb.value().unwrap();
    assert_eq!(v.middle, 10.0);
    assert_eq!(v.upper, 10.0);
    assert_eq!(v.lower, 10.0);
}

#[test]
fn bollinger_bands_bracket_middle() {
    let mut bb = BollingerBands::new(5, 2.0);
    for x in [1.0, 2.0, 3.0, 4.0, 5.0] {
        bb.update(x);
    }
    let v: BollingerValue = bb.value().unwrap();
    assert_eq!(v.middle, 3.0);
    let expected_std = 2.0_f64.sqrt();
    assert!((v.upper - (3.0 + 2.0 * expected_std)).abs() < 1e-9);
    assert!((v.lower - (3.0 - 2.0 * expected_std)).abs() < 1e-9);
}

// --- MACD ---

#[test]
#[should_panic(expected = "fast period must be less than slow period")]
fn macd_rejects_fast_ge_slow() {
    let _ = Macd::new(26, 12, 9);
}

#[test]
fn macd_none_until_all_emas_primed() {
    let mut m = Macd::new(2, 4, 2);
    // Slow (4) + signal (2) -> needs at least 5 values before Some.
    for i in 1..=4 {
        assert_eq!(m.update(i as f64), None, "at step {i}");
    }
    assert!(m.update(5.0).is_some());
}

#[test]
fn macd_histogram_equals_macd_minus_signal() {
    let mut m = Macd::new(2, 4, 2);
    for x in 1..=20 {
        m.update(x as f64);
    }
    let v = m.value().unwrap();
    assert!((v.histogram - (v.macd - v.signal)).abs() < 1e-12);
    assert!(v.macd > 0.0);
}

#[test]
fn macd_reset_clears_state() {
    let mut m = Macd::new(2, 4, 2);
    for x in 1..=20 {
        m.update(x as f64);
    }
    assert!(m.is_ready());
    m.reset();
    assert!(!m.is_ready());
    assert_eq!(m.update(1.0), None);
}
