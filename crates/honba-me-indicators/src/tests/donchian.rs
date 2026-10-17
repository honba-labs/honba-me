//! Unit tests for `crate::donchian`.

use super::{assert_close, hlc};
use crate::{Donchian, Indicator};

#[test]
#[should_panic(expected = "Donchian period must be positive")]
fn zero_period_panics() {
    let _ = Donchian::new(0);
}

#[test]
fn reports_its_period_and_readiness() {
    let mut d = Donchian::new(3);
    assert_eq!(d.period(), 3);
    assert!(!d.is_ready());
    d.update(&hlc(10.0, 8.0, 9.0));
    assert!(!d.is_ready());
    d.update(&hlc(11.0, 9.0, 10.0));
    assert!(!d.is_ready());
    let _ = d.update(&hlc(9.0, 7.0, 8.0));
    assert!(d.is_ready());
}

#[test]
fn middle_is_midpoint_of_upper_and_lower() {
    let mut d = Donchian::new(3);
    for (h, l) in [(10.0, 8.0), (11.0, 9.0), (9.0, 7.0)] {
        d.update(&hlc(h, l, 9.0));
    }
    let v = d.value().unwrap();
    assert_eq!(v.upper, 11.0);
    assert_eq!(v.lower, 7.0);
    assert_close(v.middle, 9.0);
}

#[test]
fn window_slides_over_last_period_bars() {
    let mut d = Donchian::new(2);
    let v = d.update(&hlc(10.0, 8.0, 9.0));
    assert_eq!(v, None);
    let v = d.update(&hlc(11.0, 9.0, 10.0)).unwrap();
    assert_eq!(v.upper, 11.0);
    assert_eq!(v.lower, 8.0);
    let v = d.update(&hlc(9.0, 7.0, 8.0)).unwrap();
    assert_eq!(v.upper, 11.0);
    assert_eq!(v.lower, 7.0);
    let v = d.update(&hlc(5.0, 2.0, 3.0)).unwrap();
    assert_eq!(v.upper, 9.0);
    assert_eq!(v.lower, 2.0);
}

#[test]
fn period_one_echoes_input_bar() {
    let mut d = Donchian::new(1);
    let v = d.update(&hlc(10.0, 8.0, 9.0)).unwrap();
    assert_eq!(v.upper, 10.0);
    assert_eq!(v.lower, 8.0);
    assert_close(v.middle, 9.0);
}

#[test]
fn reset_clears_state() {
    let mut d = Donchian::new(2);
    d.update(&hlc(10.0, 8.0, 9.0));
    d.update(&hlc(11.0, 9.0, 10.0));
    assert!(d.is_ready());
    d.reset();
    assert!(!d.is_ready());
    assert_eq!(d.update(&hlc(5.0, 3.0, 4.0)), None);
}

fn reference(bars: &[(f64, f64)], period: usize) -> Vec<Option<(f64, f64, f64)>> {
    let mut out = Vec::with_capacity(bars.len());
    for i in 0..bars.len() {
        if i + 1 < period {
            out.push(None);
            continue;
        }
        let window = &bars[i + 1 - period..=i];
        let upper = window
            .iter()
            .map(|(h, _)| *h)
            .fold(f64::NEG_INFINITY, f64::max);
        let lower = window.iter().map(|(_, l)| *l).fold(f64::INFINITY, f64::min);
        out.push(Some((upper, lower, (upper + lower) * 0.5)));
    }
    out
}

#[test]
fn oracle_against_o_n_reference() {
    let seq: Vec<(f64, f64)> = (0..500)
        .map(|i: usize| {
            let a = ((i.wrapping_mul(1664525).wrapping_add(1013904223)) % 10000) as f64 / 100.0;
            let b = ((i.wrapping_mul(22695477).wrapping_add(1)) % 5000) as f64 / 100.0;
            let lo = a.min(b);
            let hi = a.max(b) + 0.01;
            (hi, lo)
        })
        .collect();
    for period in [1, 2, 3, 5, 20] {
        let mut d = Donchian::new(period);
        let expected = reference(&seq, period);
        for (i, (h, l)) in seq.iter().enumerate() {
            let got = d.update(&hlc(*h, *l, *l));
            match expected[i] {
                None => assert_eq!(got, None, "period {period} index {i}"),
                Some((eu, el, em)) => {
                    let gv = got.unwrap();
                    assert_close(gv.upper, eu);
                    assert_close(gv.lower, el);
                    assert_close(gv.middle, em);
                }
            }
        }
    }
}
