use super::hlc;
use crate::screener::{
    FilterOp, MetricRef, ScreenerError, ScreenerFilterPredicate, evaluate_predicate,
};

#[test]
fn test_predicate_gt() {
    let bars = vec![hlc(10.0, 5.0, 8.0)];
    let pred = ScreenerFilterPredicate {
        metric: MetricRef {
            key: "close".to_string(),
            period: None,
        },
        op: FilterOp::Gt,
        value: 7.0,
    };
    assert!(evaluate_predicate(&pred, &bars).unwrap());
}

#[test]
fn test_unsupported_metric() {
    let bars = vec![hlc(10.0, 5.0, 8.0)];
    let pred = ScreenerFilterPredicate {
        metric: MetricRef {
            key: "unknown".to_string(),
            period: None,
        },
        op: FilterOp::Eq,
        value: 10.0,
    };
    let err = evaluate_predicate(&pred, &bars).unwrap_err();
    assert!(matches!(err, ScreenerError::UnsupportedMetric { .. }));
}
