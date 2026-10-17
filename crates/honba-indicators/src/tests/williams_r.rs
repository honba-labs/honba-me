//! Unit tests for `crate::williams_r`.

use super::{assert_close, hlc};
use crate::{Indicator, WilliamsR};

#[test]
#[should_panic(expected = "WilliamsR period must be positive")]
fn zero_period_panics() {
    let _ = WilliamsR::new(0);
}

#[test]
fn reports_its_period_and_readiness() {
    let mut w = WilliamsR::new(3);
    assert_eq!(w.period(), 3);
    assert!(!w.is_ready());
    w.update(&hlc(10.0, 5.0, 9.0));
    w.update(&hlc(12.0, 6.0, 11.0));
    assert!(!w.is_ready());
    let _ = w.update(&hlc(11.0, 4.0, 6.0));
    assert!(w.is_ready());
}

#[test]
fn flat_window_gives_minus_fifty() {
    let mut w = WilliamsR::new(2);
    w.update(&hlc(10.0, 10.0, 10.0));
    let v = w.update(&hlc(10.0, 10.0, 10.0)).unwrap();
    assert_close(v, -50.0);
}

#[test]
fn period_one_yields_known_value() {
    let mut w = WilliamsR::new(1);
    let v = w.update(&hlc(10.0, 5.0, 7.0)).unwrap();
    assert_close(v, -60.0);
}

#[test]
fn known_triple() {
    let mut w = WilliamsR::new(3);
    w.update(&hlc(10.0, 5.0, 9.0));
    w.update(&hlc(12.0, 6.0, 11.0));
    let r = w.update(&hlc(11.0, 4.0, 6.0)).unwrap();
    assert_close(r, -75.0);
}

#[test]
fn reset_clears_state() {
    let mut w = WilliamsR::new(2);
    w.update(&hlc(10.0, 5.0, 9.0));
    w.update(&hlc(11.0, 6.0, 10.0));
    assert!(w.is_ready());
    w.reset();
    assert!(!w.is_ready());
    assert_eq!(w.update(&hlc(12.0, 7.0, 11.0)), None);
}

fn reference(bars: &[(f64, f64, f64)], period: usize) -> Vec<Option<f64>> {
    let mut out = Vec::with_capacity(bars.len());
    for i in 0..bars.len() {
        if i + 1 < period {
            out.push(None);
            continue;
        }
        let window = &bars[i + 1 - period..=i];
        let hh = window
            .iter()
            .map(|(h, _, _)| *h)
            .fold(f64::NEG_INFINITY, f64::max);
        let ll = window
            .iter()
            .map(|(_, l, _)| *l)
            .fold(f64::INFINITY, f64::min);
        let close = bars[i].2;
        let denom = hh - ll;
        let v = if denom.abs() < f64::EPSILON {
            -50.0
        } else {
            -100.0 * (hh - close) / denom
        };
        out.push(Some(v));
    }
    out
}

#[test]
fn oracle_against_o_n_reference() {
    let seq: Vec<(f64, f64, f64)> = (0..500)
        .map(|i: usize| {
            let a = ((i.wrapping_mul(1664525).wrapping_add(1013904223)) % 10000) as f64 / 100.0;
            let b = ((i.wrapping_mul(22695477).wrapping_add(1)) % 5000) as f64 / 100.0;
            let lo = a.min(b).min(0.0);
            let hi = (a.max(b) + 0.01).max(0.01);
            let cl = lo + (hi - lo) * 0.3;
            (hi, lo, cl)
        })
        .collect();
    for period in [1, 2, 3, 5, 20] {
        let mut w = WilliamsR::new(period);
        let expected = reference(&seq, period);
        for (i, (h, l, c)) in seq.iter().enumerate() {
            let got = w.update(&hlc(*h, *l, *c));
            match expected[i] {
                None => assert_eq!(got, None, "period {period} index {i}"),
                Some(e) => assert_close(got.unwrap(), e),
            }
        }
    }
}
