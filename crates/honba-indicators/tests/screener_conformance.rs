//! Runs `schema/conformance/screener_scan.json` against the pure screener evaluator.
//!
//! The expected values were generated once from the Python evaluator
//! (`scripts/gen_screener_scan_vectors.py`); `python/tests/integration/
//! test_screener_scan_conformance.py` runs the same file against Python. Booleans must match
//! exactly; metric values within `tolerance.relative` (the SMA window sum is updated in a
//! different operation order in Rust, so the last bit may differ).
//!
//! `unsupported` cases are metrics Python cannot compute from bars (it answers `false`); Rust must
//! reject them. `divergences` pin a case where the two intentionally differ.

use honba_entities::{ScreenerFilterGroup, ScreenerFilterPredicate};
use honba_indicators::screener::{evaluate_group, evaluate_predicate, latest_metrics};
use honba_messages::{
    Bar, BarAggregation, BarSpecification, BarType, Exchange, InstrumentId, PriceType, UnixNanos,
};
use serde_json::Value;

const FIXTURE: &str = include_str!("../../../schema/conformance/screener_scan.json");

fn floats(v: &Value) -> Vec<f64> {
    v.as_array()
        .expect("array")
        .iter()
        .map(|x| x.as_f64().expect("number"))
        .collect()
}

fn bars(input: &Value) -> Vec<Bar> {
    let (o, h, l, c, v) = (
        floats(&input["open"]),
        floats(&input["high"]),
        floats(&input["low"]),
        floats(&input["close"]),
        floats(&input["volume"]),
    );
    let bar_type = BarType::new(
        InstrumentId::new("X", Exchange::new("NSE")),
        BarSpecification::new(1, BarAggregation::Day, PriceType::Last),
    );
    (0..c.len())
        .map(|i| {
            let ts = UnixNanos::from_u64(i as u64 * 86_400_000_000_000);
            Bar::new(bar_type.clone(), o[i], h[i], l[i], c[i], v[i], ts, ts)
        })
        .collect()
}

fn close_enough(actual: f64, expected: f64, rel: f64) -> bool {
    if expected.fract() == 0.0 {
        return actual == expected;
    }
    (actual - expected).abs() <= rel * expected.abs().max(1.0)
}

fn predicate(case: &Value) -> ScreenerFilterPredicate {
    serde_json::from_value(case["predicate"].clone()).expect("predicate parses")
}

#[test]
fn every_predicate_case_matches_python() {
    let fx: Value = serde_json::from_str(FIXTURE).unwrap();
    assert_eq!(fx["type"], "ScreenerScan");
    let rel = fx["tolerance"]["relative"].as_f64().unwrap();
    let cases = fx["cases"].as_array().unwrap();
    assert!(cases.len() >= 60);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let data = bars(&fx["inputs"][case["input"].as_str().unwrap()]);
        let pred = predicate(case);
        let got = evaluate_predicate(&pred, &data);
        let expected = &case["expected"];
        if let Some(py_error) = expected.get("error") {
            // Python raised (TypeError/ValueError): Rust must refuse, not answer.
            assert!(
                got.is_err(),
                "{name}: python raised {py_error}, rust gave {got:?}"
            );
            continue;
        }
        assert_eq!(
            got,
            Ok(expected["result"].as_bool().unwrap()),
            "{name}: predicate result"
        );
        let mut keys = vec![pred.key.clone()];
        if let Some(r) = pred.metric_ref() {
            keys.push(r.key);
        }
        let metrics = latest_metrics(&keys, &data).unwrap();
        for (key, want) in expected["metrics"].as_object().unwrap() {
            let have = metrics[key.as_str()];
            match want.as_f64() {
                None => assert_eq!(have, None, "{name}: metric {key}"),
                Some(w) => assert!(
                    have.is_some_and(|h| close_enough(h, w, rel)),
                    "{name}: metric {key}: got {have:?}, want {w}"
                ),
            }
        }
    }
}

#[test]
fn every_group_case_matches_python() {
    let fx: Value = serde_json::from_str(FIXTURE).unwrap();
    for case in fx["groups"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let data = bars(&fx["inputs"][case["input"].as_str().unwrap()]);
        let group: ScreenerFilterGroup = serde_json::from_value(case["group"].clone()).unwrap();
        assert_eq!(
            evaluate_group(&group, &data),
            Ok(case["expected"]["result"].as_bool().unwrap()),
            "{name}"
        );
    }
}

#[test]
fn metrics_python_cannot_compute_from_bars_are_rejected_not_false() {
    let fx: Value = serde_json::from_str(FIXTURE).unwrap();
    for case in fx["unsupported"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let data = bars(&fx["inputs"][case["input"].as_str().unwrap()]);
        let err = evaluate_predicate(&predicate(case), &data)
            .expect_err(&format!("{name}: must not be answered"));
        assert_eq!(err.reason(), case["rust"].as_str().unwrap(), "{name}");
    }
}

#[test]
fn pinned_divergences_hold_for_rust() {
    let fx: Value = serde_json::from_str(FIXTURE).unwrap();
    let divergences = fx["divergences"].as_array().unwrap();
    assert!(!divergences.is_empty());
    for case in divergences {
        let name = case["name"].as_str().unwrap();
        let data = bars(&fx["inputs"][case["input"].as_str().unwrap()]);
        assert_eq!(
            evaluate_predicate(&predicate(case), &data),
            Ok(case["rust"]["result"].as_bool().unwrap()),
            "{name}"
        );
        assert_ne!(case["python"]["result"], case["rust"]["result"], "{name}");
    }
}
