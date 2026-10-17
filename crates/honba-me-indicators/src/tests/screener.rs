//! Unit tests for `crate::screener`: one test per operator and per branch of the reference
//! (Python) evaluator.

use honba_entities::{FilterOp, MetricRef, ScreenerFilterGroup, ScreenerFilterPredicate};
use honba_messages::Bar;
use serde_json::{json, Value};

use super::{any_instrument, hlc};
use crate::screener::{
    evaluate_group, evaluate_predicate, group_metric_keys, group_timeframes, latest_metrics,
    py_float_repr, validate_group, ScreenerError, MAX_GROUP_DEPTH,
};

fn closes(values: &[f64]) -> Vec<Bar> {
    values.iter().map(|&c| hlc(c + 0.5, c - 0.5, c)).collect()
}

fn pred(key: &str, op: FilterOp, value: Value) -> ScreenerFilterPredicate {
    ScreenerFilterPredicate::new(key, op, value).expect("valid predicate")
}

fn eval(key: &str, op: FilterOp, value: Value, bars: &[Bar]) -> bool {
    evaluate_predicate(&pred(key, op, value), bars).expect("evaluates")
}

fn group(raw: Value) -> ScreenerFilterGroup {
    serde_json::from_value(raw).expect("group")
}

#[test]
fn comparison_operators_follow_python() {
    let bars = closes(&[10.5, 11.25, 10.75]);
    assert!(eval("close", FilterOp::Gt, json!(10), &bars));
    assert!(!eval("close", FilterOp::Gt, json!(10.75), &bars));
    assert!(eval("close", FilterOp::Gte, json!(10.75), &bars));
    assert!(eval("close", FilterOp::Lt, json!(11), &bars));
    assert!(!eval("close", FilterOp::Lt, json!(10.75), &bars));
    assert!(eval("close", FilterOp::Lte, json!(10.75), &bars));
    assert!(eval("close", FilterOp::Eq, json!(10.75), &bars));
    assert!(!eval("close", FilterOp::Eq, json!("10.75"), &bars));
    assert!(eval("close", FilterOp::Neq, json!("10.75"), &bars));
    assert!(!eval("close", FilterOp::Neq, json!(10.75), &bars));
}

#[test]
fn a_null_operand_is_false_for_every_operator() {
    let bars = closes(&[10.0]);
    assert!(!eval("close", FilterOp::Eq, Value::Null, &bars));
    assert!(!eval("close", FilterOp::Neq, Value::Null, &bars));
    assert!(!eval("close", FilterOp::Like, Value::Null, &bars));
}

#[test]
fn booleans_compare_as_numbers_like_python() {
    let bars = closes(&[1.0]);
    assert!(eval("close", FilterOp::Eq, json!(true), &bars));
    assert!(!eval("close", FilterOp::Eq, json!(false), &bars));
}

#[test]
fn ordering_against_a_string_is_an_invalid_operand_not_false() {
    // Python raises TypeError here; a silent false would hide a malformed filter.
    let bars = closes(&[10.0]);
    let err = evaluate_predicate(&pred("close", FilterOp::Gt, json!("5")), &bars).unwrap_err();
    assert_eq!(err.reason(), "invalid_operand");
    // ... but only once the left side is known, as in Python.
    let warmup = closes(&[10.0]);
    assert_eq!(
        evaluate_predicate(&pred("sma5", FilterOp::Gt, json!("5")), &warmup),
        Ok(false)
    );
}

#[test]
fn between_is_inclusive_on_both_ends() {
    let bars = closes(&[10.75]);
    assert!(eval("close", FilterOp::Between, json!([10.75, 11]), &bars));
    assert!(eval("close", FilterOp::Between, json!([10, 10.75]), &bars));
    assert!(!eval("close", FilterOp::Between, json!([11, 12]), &bars));
}

#[test]
fn in_and_not_in_match_numerically_and_ignore_other_types() {
    let bars = closes(&[42.0]);
    assert!(eval("close", FilterOp::In, json!([42, 43]), &bars));
    assert!(eval("close", FilterOp::In, json!(["x", 42.0]), &bars));
    assert!(!eval("close", FilterOp::In, json!([1, "x"]), &bars));
    assert!(!eval("close", FilterOp::In, json!([]), &bars));
    assert!(eval("close", FilterOp::NotIn, json!([1, 2]), &bars));
    assert!(!eval("close", FilterOp::NotIn, json!([42]), &bars));
}

#[test]
fn like_and_has_are_case_insensitive_substring_matches_on_the_python_repr() {
    let bars = closes(&[10.75]);
    assert!(eval("close", FilterOp::Like, json!("0.7"), &bars));
    assert!(eval("close", FilterOp::Has, json!("10."), &bars));
    assert!(!eval("close", FilterOp::Like, json!("9"), &bars));
    let whole = closes(&[42.0]);
    // str(42.0) == "42.0" in Python
    assert!(eval("close", FilterOp::Like, json!("42.0"), &whole));
    // str(42) == "42"
    assert!(eval("close", FilterOp::Like, json!(42), &whole));
    // str(True) == "True", which is not a substring of "1.0" (a literal is not coerced first)
    assert!(!eval("close", FilterOp::Like, json!(true), &closes(&[1.0])));
    assert!(eval("close", FilterOp::Like, json!(""), &whole));
}

#[test]
fn like_with_a_list_operand_is_an_invalid_operand() {
    let bars = closes(&[1.0]);
    let err = evaluate_predicate(&pred("close", FilterOp::Like, json!([1])), &bars).unwrap_err();
    assert_eq!(err.reason(), "invalid_operand");
}

#[test]
fn python_float_repr_matches_python() {
    assert_eq!(py_float_repr(42.0), "42.0");
    assert_eq!(py_float_repr(10.75), "10.75");
    assert_eq!(py_float_repr(-0.5), "-0.5");
    assert_eq!(py_float_repr(0.0), "0.0");
    assert_eq!(py_float_repr(-0.0), "-0.0");
    assert_eq!(py_float_repr(0.0001), "0.0001");
    assert_eq!(py_float_repr(0.00001), "1e-05");
    assert_eq!(py_float_repr(1.5e-7), "1.5e-07");
    assert_eq!(py_float_repr(1e16), "1e+16");
    assert_eq!(py_float_repr(1234567890123456.0), "1234567890123456.0");
    assert_eq!(
        py_float_repr(123456789012345680.0),
        "1.2345678901234568e+17"
    );
}

#[test]
fn metric_comparisons_use_the_right_hand_series() {
    let bars = closes(&[10.0, 11.0, 12.0]);
    let p = ScreenerFilterPredicate::new(
        "low",
        FilterOp::Lt,
        serde_json::to_value(MetricRef::new("close")).unwrap(),
    )
    .unwrap();
    assert_eq!(evaluate_predicate(&p, &bars), Ok(true));
}

#[test]
fn crosses_above_needs_the_previous_bar_at_or_below_and_the_current_above() {
    let rising = closes(&[16.0, 18.0]);
    assert!(eval("close", FilterOp::CrossesAbove, json!(17), &rising));
    // previous == threshold counts as below (<=)
    assert!(eval("close", FilterOp::CrossesAbove, json!(16), &rising));
    // already above before
    assert!(!eval("close", FilterOp::CrossesAbove, json!(15), &rising));
    // needs two bars
    assert!(!eval(
        "close",
        FilterOp::CrossesAbove,
        json!(1),
        &closes(&[5.0])
    ));
}

#[test]
fn crosses_below_needs_the_previous_bar_at_or_above_and_the_current_below() {
    let falling = closes(&[18.0, 16.0]);
    assert!(eval("close", FilterOp::CrossesBelow, json!(17), &falling));
    assert!(eval("close", FilterOp::CrossesBelow, json!(18), &falling));
    assert!(!eval("close", FilterOp::CrossesBelow, json!(19), &falling));
}

#[test]
fn crossing_a_metric_uses_both_previous_values() {
    let mut series = vec![20.0, 19.5, 19.0, 18.5, 18.0, 17.5, 17.0, 16.5, 16.0];
    series.push(25.0);
    let bars = closes(&series);
    let sma5 = serde_json::to_value(MetricRef::new("SMA5")).unwrap();
    assert!(eval("SMA3", FilterOp::CrossesAbove, sma5.clone(), &bars));
    assert!(!eval("SMA3", FilterOp::CrossesBelow, sma5, &bars));
}

#[test]
fn warm_up_and_empty_series_are_false_not_errors() {
    assert!(!eval(
        "SMA5",
        FilterOp::Gt,
        json!(0),
        &closes(&[1.0, 2.0, 3.0])
    ));
    assert!(!eval(
        "RSI",
        FilterOp::Gt,
        json!(0),
        &closes(&[1.0, 2.0, 3.0])
    ));
    assert!(!eval("close", FilterOp::Gt, json!(0), &[]));
}

#[test]
fn rsi_of_a_series_without_losses_is_100() {
    let rising: Vec<f64> = (0..20).map(|i| 100.0 + i as f64).collect();
    assert!(eval("RSI", FilterOp::Gte, json!(100), &closes(&rising)));
}

#[test]
fn sma_is_the_trailing_mean_and_keys_are_case_insensitive() {
    let bars = closes(&[1.0, 2.0, 3.0, 4.0]);
    let m = latest_metrics(&["SMA3".into(), "sma2".into()], &bars).unwrap();
    assert_eq!(m["SMA3"], Some(3.0));
    assert_eq!(m["sma2"], Some(3.5));
}

#[test]
fn latest_metrics_reads_ohlcv_and_reports_none_when_warming_up() {
    let bars = vec![hlc(12.0, 9.0, 11.0)];
    let keys: Vec<String> = ["open", "HIGH", "low", "close", "volume", "SMA5"]
        .iter()
        .map(|k| (*k).to_owned())
        .collect();
    let m = latest_metrics(&keys, &bars).unwrap();
    assert_eq!(m["open"], Some(11.0));
    assert_eq!(m["HIGH"], Some(12.0));
    assert_eq!(m["low"], Some(9.0));
    assert_eq!(m["close"], Some(11.0));
    assert_eq!(m["volume"], Some(1.0));
    assert_eq!(m["SMA5"], None);
}

#[test]
fn the_52_week_metrics_need_252_bars_in_any_key_case() {
    let few: Vec<f64> = (0..20).map(f64::from).collect();
    let bars = closes(&few);
    assert!(!eval("price_52_week_high", FilterOp::Gt, json!(0), &bars));
    assert!(!eval("PRICE_52_WEEK_LOW", FilterOp::Gt, json!(0), &bars));
    let many: Vec<f64> = (0..260).map(|i| 100.0 + f64::from(i % 7)).collect();
    let bars = closes(&many);
    assert!(eval(
        "price_52_week_high",
        FilterOp::Gte,
        json!(106.5),
        &bars
    ));
    assert!(eval("price_52_week_low", FilterOp::Lte, json!(99.5), &bars));
    // the window is the last 252 bars: an early outlier falls out of it
    let mut with_outlier = many.clone();
    with_outlier[0] = 1000.0;
    assert!(!eval(
        "price_52_week_high",
        FilterOp::Gt,
        json!(500),
        &closes(&with_outlier)
    ));
}

#[test]
fn unsupported_metrics_are_errors_never_false() {
    let bars = closes(&[1.0, 2.0, 3.0]);
    for key in [
        "price_earnings_ttm",
        "market_cap",
        "ema20",
        "sma0",
        "smaX",
        "rsi14",
    ] {
        let err = evaluate_predicate(&pred(key, FilterOp::Gt, json!(1)), &bars).unwrap_err();
        assert_eq!(
            err,
            ScreenerError::UnsupportedMetric {
                key: key.to_owned()
            },
            "{key}"
        );
        assert_eq!(err.reason(), "unsupported_metric");
    }
    // an unsupported metric on the right-hand side is just as unsupported
    let rhs = serde_json::to_value(MetricRef::new("ema20")).unwrap();
    let err = evaluate_predicate(&pred("close", FilterOp::Gt, rhs), &bars).unwrap_err();
    assert_eq!(err.reason(), "unsupported_metric");
    // and so is an empty series
    assert!(evaluate_predicate(&pred("market_cap", FilterOp::Gt, json!(1)), &[]).is_err());
}

#[test]
fn a_period_dimension_is_unsupported_for_a_bar_dataset() {
    let bars = closes(&[1.0]);
    let mut p = pred("close", FilterOp::Gt, json!(0));
    p.period = Some(honba_entities::MetricPeriod::Ttm);
    assert_eq!(
        evaluate_predicate(&p, &bars).unwrap_err().reason(),
        "unsupported_metric"
    );
}

#[test]
fn groups_combine_with_and_or_and_nest() {
    let bars = closes(&[100.0, 101.0, 102.0, 103.0, 104.0]);
    let t = json!({"key": "close", "op": "gt", "value": 100});
    let f = json!({"key": "close", "op": "lt", "value": 100});
    assert_eq!(
        evaluate_group(&group(json!({"operator": "AND", "items": [t, t]})), &bars),
        Ok(true)
    );
    assert_eq!(
        evaluate_group(&group(json!({"operator": "AND", "items": [t, f]})), &bars),
        Ok(false)
    );
    assert_eq!(
        evaluate_group(&group(json!({"operator": "OR", "items": [f, t]})), &bars),
        Ok(true)
    );
    assert_eq!(
        evaluate_group(&group(json!({"operator": "OR", "items": [f, f]})), &bars),
        Ok(false)
    );
    let nested = json!({"operator": "AND", "items": [t, {"operator": "OR", "items": [f, t]}]});
    assert_eq!(evaluate_group(&group(nested), &bars), Ok(true));
}

#[test]
fn an_empty_group_is_true_even_without_bars() {
    for op in ["AND", "OR"] {
        let g = group(json!({"operator": op, "items": []}));
        assert_eq!(evaluate_group(&g, &[]), Ok(true));
    }
}

#[test]
fn a_group_does_not_short_circuit_past_an_unsupported_metric() {
    let bars = closes(&[1.0]);
    let g = group(json!({"operator": "OR", "items": [
        {"key": "close", "op": "gt", "value": 0},
        {"key": "market_cap", "op": "gt", "value": 0}
    ]}));
    assert_eq!(
        evaluate_group(&g, &bars).unwrap_err().reason(),
        "unsupported_metric"
    );
}

#[test]
fn a_malformed_group_is_an_invalid_filter() {
    let bad_op = group(json!({"operator": "XOR", "items": []}));
    assert_eq!(
        validate_group(&bad_op).unwrap_err().reason(),
        "invalid_filter"
    );
    let bad_item = group(json!({"operator": "AND", "items": [42]}));
    assert_eq!(
        validate_group(&bad_item).unwrap_err().reason(),
        "invalid_filter"
    );
    let bad_pred = group(json!({"operator": "AND", "items": [{"key": "close", "op": "gt"}]}));
    assert_eq!(
        validate_group(&bad_pred).unwrap_err().reason(),
        "invalid_filter"
    );
    assert!(evaluate_group(&bad_op, &closes(&[1.0])).is_err());
}

#[test]
fn nesting_is_bounded() {
    let mut raw = json!({"operator": "AND", "items": []});
    for _ in 0..=MAX_GROUP_DEPTH {
        raw = json!({"operator": "AND", "items": [raw]});
    }
    let err = validate_group(&group(raw)).unwrap_err();
    assert_eq!(err.reason(), "invalid_filter");
}

#[test]
fn the_number_of_predicates_is_bounded() {
    let items: Vec<Value> = (0..200)
        .map(|_| json!({"key": "close", "op": "gt", "value": 0}))
        .collect();
    let g = group(json!({"operator": "AND", "items": items}));
    assert_eq!(validate_group(&g).unwrap_err().reason(), "invalid_filter");
}

#[test]
fn group_metric_keys_lists_left_and_right_keys_once_in_order() {
    let g = group(json!({"operator": "AND", "items": [
        {"key": "SMA3", "op": "crosses_above", "value": {"key": "SMA5"}},
        {"operator": "OR", "items": [{"key": "close", "op": "gt", "value": 1},
                                     {"key": "SMA3", "op": "lt", "value": 9}]}
    ]}));
    assert_eq!(
        group_metric_keys(&g).unwrap(),
        vec!["SMA3", "SMA5", "close"]
    );
    let _ = any_instrument();
}

#[test]
fn group_timeframes_lists_every_timeframe_dimension_once() {
    use honba_entities::Timeframe;
    let g = group(json!({"operator": "AND", "items": [
        {"key": "SMA3", "op": "crosses_above", "value": {"key": "SMA5", "timeframe": "1W"},
         "timeframe": "1D"},
        {"key": "close", "op": "gt", "value": 1, "timeframe": "1D"},
        {"key": "close", "op": "gt", "value": 1}
    ]}));
    assert_eq!(
        group_timeframes(&g).unwrap(),
        vec![Timeframe::D1, Timeframe::W1]
    );
}
