//! Tests for session-aware multi-timeframe aggregation.

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use honba_core::{Bar, Exchange, InstrumentId, Price, Quantity};
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;

use crate::aggregator::{BarAggregation, BarSpecification, SessionBarAggregator};
use crate::calendar::{MarketCalendar, Session};

struct MockCalendar;

impl MarketCalendar for MockCalendar {
    fn session(&self) -> Session {
        Session::new(
            NaiveTime::from_hms_opt(9, 15, 0).unwrap(),
            NaiveTime::from_hms_opt(15, 30, 0).unwrap(),
        )
    }

    fn is_holiday(&self, _date: NaiveDate) -> bool {
        false
    }
}

fn bar_at(
    symbol: &str,
    dt: NaiveDateTime,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    vol: f64,
) -> Bar {
    let instrument = InstrumentId::new(Exchange::Nse, symbol);
    Bar {
        instrument,
        open: Price::new(Decimal::from_f64(open).unwrap()),
        high: Price::new(Decimal::from_f64(high).unwrap()),
        low: Price::new(Decimal::from_f64(low).unwrap()),
        close: Price::new(Decimal::from_f64(close).unwrap()),
        volume: Quantity::new(Decimal::from_f64(vol).unwrap()),
        timestamp: dt.and_utc(),
    }
}

#[test]
fn htf_bar_only_when_complete() {
    let calendar = MockCalendar;
    let target_spec = BarSpecification::new(1, BarAggregation::Day);
    let mut aggregator = SessionBarAggregator::new(calendar, target_spec);

    let date = NaiveDate::from_ymd_opt(2026, 5, 10).unwrap();

    // 09:15 bar: session open
    let dt_open = NaiveDateTime::new(date, NaiveTime::from_hms_opt(9, 15, 0).unwrap());
    let bar1 = bar_at("TCS", dt_open, 100.0, 105.0, 99.0, 104.0, 1000.0);
    assert_eq!(aggregator.process(&bar1), None);

    // 12:00 bar: midday bar
    let dt_mid = NaiveDateTime::new(date, NaiveTime::from_hms_opt(12, 0, 0).unwrap());
    let bar2 = bar_at("TCS", dt_mid, 104.0, 110.0, 102.0, 108.0, 1500.0);
    assert_eq!(aggregator.process(&bar2), None);

    // 15:28 bar: near close, but not terminal yet
    let dt_pre = NaiveDateTime::new(date, NaiveTime::from_hms_opt(15, 28, 0).unwrap());
    let bar3 = bar_at("TCS", dt_pre, 108.0, 109.0, 107.0, 108.5, 800.0);
    assert_eq!(aggregator.process(&bar3), None);

    // 15:29 bar: duration is 1m, so end time is 15:30:00 (exact session close)
    let dt_close = NaiveDateTime::new(date, NaiveTime::from_hms_opt(15, 29, 0).unwrap());
    let bar_final = bar_at("TCS", dt_close, 108.5, 109.0, 106.0, 107.0, 1200.0);
    let htf_bar = aggregator.process(&bar_final);

    // MUST be emitted now and ONLY now!
    assert!(
        htf_bar.is_some(),
        "HTF bar should be emitted on session complete"
    );
    let htf = htf_bar.unwrap();
    assert_eq!(htf.open, Price::new(Decimal::from(100)));
    assert_eq!(htf.high, Price::new(Decimal::from(110)));
    assert_eq!(htf.low, Price::new(Decimal::from(99)));
    assert_eq!(htf.close, Price::new(Decimal::from(107)));
    assert_eq!(htf.volume, Quantity::new(Decimal::from(4500)));

    // Close timestamp of HTF bar must be the session close (15:30:00)
    let expected_close_dt =
        NaiveDateTime::new(date, NaiveTime::from_hms_opt(15, 30, 0).unwrap()).and_utc();
    assert_eq!(htf.timestamp, expected_close_dt);
}

#[test]
fn htf_bar_emitted_on_next_session_arrival() {
    let calendar = MockCalendar;
    let target_spec = BarSpecification::new(1, BarAggregation::Day);
    let mut aggregator = SessionBarAggregator::new(calendar, target_spec);

    let day1 = NaiveDate::from_ymd_opt(2026, 5, 10).unwrap();
    let dt1 = NaiveDateTime::new(day1, NaiveTime::from_hms_opt(9, 15, 0).unwrap());
    let bar1 = bar_at("INFY", dt1, 200.0, 205.0, 198.0, 202.0, 500.0);
    assert_eq!(aggregator.process(&bar1), None);

    // Next day arrives (e.g. data stream didn't have exact last second)
    let day2 = NaiveDate::from_ymd_opt(2026, 5, 11).unwrap();
    let dt2 = NaiveDateTime::new(day2, NaiveTime::from_hms_opt(9, 15, 0).unwrap());
    let bar2 = bar_at("INFY", dt2, 203.0, 206.0, 201.0, 205.0, 600.0);

    let completed = aggregator.process(&bar2);
    assert!(completed.is_some());
    let htf = completed.unwrap();
    assert_eq!(htf.open, Price::new(Decimal::from(200)));
    assert_eq!(htf.close, Price::new(Decimal::from(202)));
}
