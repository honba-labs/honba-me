//! Unit tests for `crate::calendar` (trait defaults and `Session`).

use chrono::NaiveTime;

use super::{date, Holidays};
use crate::{MarketCalendar, Session};

fn hm(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, 0).unwrap()
}

#[test]
fn session_is_half_open_at_the_close() {
    let s = Session::regular();
    assert_eq!((s.open(), s.close()), (hm(9, 15), hm(15, 30)));
    assert!(s.contains(hm(9, 15)));
    assert!(s.contains(hm(15, 29)));
    assert!(!s.contains(hm(15, 30)));
    assert!(!s.contains(hm(9, 14)));
}

#[test]
fn session_displays_as_open_dash_close() {
    assert_eq!(Session::regular().to_string(), "09:15:00-15:30:00");
    assert_eq!(
        Session::new(hm(10, 0), hm(11, 0)).to_string(),
        "10:00:00-11:00:00"
    );
}

#[test]
fn continuous_session_covers_the_whole_day_but_the_last_second() {
    let s = Session::continuous_24h();
    assert!(s.contains(hm(0, 0)));
    assert!(s.contains(hm(23, 59)));
    assert!(!s.contains(NaiveTime::from_hms_opt(23, 59, 59).unwrap()));
}

#[test]
fn weekends_and_holidays_are_not_trading_days() {
    // 2025-01-06 is a Monday.
    let cal = Holidays(vec![date(2025, 1, 7)]);
    assert!(cal.is_trading_day(date(2025, 1, 6)));
    assert!(!cal.is_trading_day(date(2025, 1, 7)));
    assert!(!cal.is_trading_day(date(2025, 1, 11)));
    assert!(!cal.is_trading_day(date(2025, 1, 12)));
    assert!(!cal.is_settlement_day(date(2025, 1, 7)));
}

#[test]
fn next_and_prev_skip_holidays_and_weekends() {
    let cal = Holidays(vec![date(2025, 1, 13)]);
    // Fri 10th -> skip Sat, Sun, holiday Mon 13th -> Tue 14th.
    assert_eq!(cal.next_trading_day(date(2025, 1, 10)), date(2025, 1, 14));
    assert_eq!(cal.prev_trading_day(date(2025, 1, 14)), date(2025, 1, 10));
}

#[test]
fn trading_days_between_is_inclusive_and_empty_when_reversed() {
    let cal = Holidays(vec![date(2025, 1, 8)]);
    // Mon 6th..Fri 10th minus Wed 8th.
    assert_eq!(
        cal.trading_days_between(date(2025, 1, 6), date(2025, 1, 10)),
        4
    );
    assert_eq!(
        cal.trading_days_between(date(2025, 1, 6), date(2025, 1, 6)),
        1
    );
    assert_eq!(
        cal.trading_days_between(date(2025, 1, 10), date(2025, 1, 6)),
        0
    );
}
