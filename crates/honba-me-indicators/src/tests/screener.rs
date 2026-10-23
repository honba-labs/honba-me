use honba_indicators::screener::{ScreenerFilterPredicate, FilterOp, MetricRef, ScreenerError, evaluate_predicate};
use honba_indicators::screener::MetricPeriod;
use honba_core::{Bar, Price, Quantity};

fn hlc(high: f64, low: f64, close: f64) -> Bar {
    Bar {
        instrument: honba_core::InstrumentId::new(honba_core::Exchange::Nse, "X"),
        open: Price::from(close),
        high: Price::from(high),
        low: Price::from(low),
        close: Price::from(close),
        volume: Quantity::from(1.0),
        ts_event: chrono::Utc::now(),
    }
}

#[test]
fn test_predicate_gt() {
    let bars = vec![hlc(10.0, 5.0, 8.0)];
    let pred = ScreenerFilterPredicate {
        metric: MetricRef { key: "close".to_string(), period: None },
        op: FilterOp::Gt,
        value: 7.0,
    };
    assert!(evaluate_predicate(&pred, &bars).unwrap());
}

#[test]
fn test_unsupported_metric() {
    let bars = vec![hlc(10.0, 5.0, 8.0)];
    let pred = ScreenerFilterPredicate {
        metric: MetricRef { key: "unknown".to_string(), period: None },
        op: FilterOp::Eq,
        value: 10.0,
    };
    let err = evaluate_predicate(&pred, &bars).unwrap_err();
    assert!(matches!(err, ScreenerError::UnsupportedMetric { .. }));
}
