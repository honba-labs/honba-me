//! Unit tests for `crate::expiry`.

use honba_entities::InstrumentKind;

use super::{date, Holidays};
use crate::{ExpiryRules, LastThursdayExpiry};

#[test]
fn expiry_is_the_last_thursday_for_derivatives_only() {
    let r = LastThursdayExpiry;
    assert_eq!(
        r.expiry_for_month(2025, 1, InstrumentKind::Future),
        Some(date(2025, 1, 30))
    );
    // December rolls the year when finding the month end.
    assert_eq!(
        r.expiry_for_month(2024, 12, InstrumentKind::Option),
        Some(date(2024, 12, 26))
    );
    assert_eq!(r.expiry_for_month(2025, 1, InstrumentKind::Equity), None);
}

#[test]
fn is_expiry_date_matches_only_the_last_thursday() {
    let r = LastThursdayExpiry;
    assert!(r.is_expiry_date(date(2025, 1, 30), InstrumentKind::Future));
    assert!(!r.is_expiry_date(date(2025, 1, 23), InstrumentKind::Future));
    assert!(!r.is_expiry_date(date(2025, 1, 29), InstrumentKind::Option));
    assert!(!r.is_expiry_date(date(2025, 1, 30), InstrumentKind::Index));
}

#[test]
fn holiday_expiry_moves_to_the_previous_trading_day() {
    let cal = Holidays(vec![date(2025, 1, 30)]);
    let r = LastThursdayExpiry;
    assert_eq!(
        r.adjust_for_holiday(date(2025, 1, 30), &cal),
        date(2025, 1, 29)
    );
    assert_eq!(
        r.adjust_for_holiday(date(2025, 1, 23), &cal),
        date(2025, 1, 23)
    );
}
