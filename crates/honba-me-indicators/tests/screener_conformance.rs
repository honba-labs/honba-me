//! Integration tests for the pure screener evaluator against canonical `honba-core` types.

use chrono::{TimeZone, Utc};
use honba_core::{Bar, Exchange, InstrumentId, Price, Quantity};
use honba_indicators::screener::{
    FilterOp, MetricPeriod, MetricRef, ScreenerError, ScreenerFilterGroup, ScreenerFilterPredicate,
    check_metric, evaluate_group, evaluate_predicate, latest_metrics,
};
use rust_decimal::Decimal;

fn make_bar(open: f64, high: f64, low: f64, close: f64, volume: u64, day_idx: i64) -> Bar {
    Bar {
        instrument: InstrumentId::new(Exchange::Nse, "X"),
        open: Price::new(Decimal::from_f64_retain(open).unwrap_or_default()),
        high: Price::new(Decimal::from_f64_retain(high).unwrap_or_default()),
        low: Price::new(Decimal::from_f64_retain(low).unwrap_or_default()),
        close: Price::new(Decimal::from_f64_retain(close).unwrap_or_default()),
        volume: Quantity::from_u64(volume),
        timestamp: Utc.timestamp_opt(day_idx * 86_400, 0).unwrap(),
    }
}

fn pred(key: &str, op: FilterOp, value: f64) -> ScreenerFilterPredicate {
    ScreenerFilterPredicate {
        metric: MetricRef {
            key: key.to_string(),
            period: None,
        },
        op,
        value,
    }
}

#[test]
fn predicate_comparisons_work_on_bars() {
    let data = vec![
        make_bar(10.0, 11.0, 9.5, 10.25, 100, 0),
        make_bar(10.25, 11.5, 10.0, 10.50, 150, 1),
        make_bar(10.50, 11.25, 10.25, 10.75, 200, 2),
    ];

    assert_eq!(
        evaluate_predicate(&pred("close", FilterOp::Gt, 10.0), &data),
        Ok(true)
    );
    assert_eq!(
        evaluate_predicate(&pred("close", FilterOp::Gt, 10.75), &data),
        Ok(false)
    );
    assert_eq!(
        evaluate_predicate(&pred("close", FilterOp::Gte, 10.75), &data),
        Ok(true)
    );
    assert_eq!(
        evaluate_predicate(&pred("close", FilterOp::Lt, 11.0), &data),
        Ok(true)
    );
    assert_eq!(
        evaluate_predicate(&pred("close", FilterOp::Lte, 10.75), &data),
        Ok(true)
    );
    assert_eq!(
        evaluate_predicate(&pred("close", FilterOp::Eq, 10.75), &data),
        Ok(true)
    );
    assert_eq!(
        evaluate_predicate(&pred("close", FilterOp::Ne, 10.0), &data),
        Ok(true)
    );
    assert_eq!(
        evaluate_predicate(&pred("close", FilterOp::CrossesAbove, 10.60), &data),
        Ok(true)
    );

    let metrics = latest_metrics(&["close".to_string(), "sma3".to_string()], &data).unwrap();
    assert_eq!(metrics["close"], Some(10.75));
    assert!((metrics["sma3"].unwrap() - 10.5).abs() < 1e-9);
}

#[test]
fn group_evaluation_and_or() {
    let data = vec![
        make_bar(10.0, 11.0, 9.5, 10.25, 100, 0),
        make_bar(10.25, 11.5, 10.0, 10.50, 150, 1),
        make_bar(10.50, 11.25, 10.25, 10.75, 200, 2),
    ];

    let group_and = ScreenerFilterGroup::And(vec![
        ScreenerFilterGroup::Predicate(pred("close", FilterOp::Gt, 10.0)),
        ScreenerFilterGroup::Predicate(pred("volume", FilterOp::Gte, 200.0)),
    ]);
    assert_eq!(evaluate_group(&group_and, &data), Ok(true));

    let group_or = ScreenerFilterGroup::Or(vec![
        ScreenerFilterGroup::Predicate(pred("close", FilterOp::Lt, 5.0)),
        ScreenerFilterGroup::Predicate(pred("volume", FilterOp::Eq, 200.0)),
    ]);
    assert_eq!(evaluate_group(&group_or, &data), Ok(true));
}

#[test]
fn unsupported_metrics_and_periods_are_rejected() {
    let data = vec![make_bar(10.0, 11.0, 9.5, 10.25, 100, 0)];
    assert!(check_metric("close").is_ok());
    assert!(matches!(
        check_metric("pe_ratio"),
        Err(ScreenerError::UnsupportedMetric { .. })
    ));

    let bad_period = ScreenerFilterPredicate {
        metric: MetricRef {
            key: "close".to_string(),
            period: Some(MetricPeriod::Ttm),
        },
        op: FilterOp::Gt,
        value: 5.0,
    };
    assert!(matches!(
        evaluate_predicate(&bad_period, &data),
        Err(ScreenerError::UnsupportedMetric { .. })
    ));
}
