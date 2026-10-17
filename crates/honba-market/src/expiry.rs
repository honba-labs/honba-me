//! Generic derivative contract expiry rules.

use chrono::{Datelike, Duration, NaiveDate, Weekday};
use honba_entities::InstrumentKind;

use crate::calendar::MarketCalendar;

/// Contract expiration rules (e.g. weekly/monthly derivative expirations).
pub trait ExpiryRules: Send + Sync {
    /// Determines whether the given date is an expiry date for contracts of kind `kind`.
    fn is_expiry_date(&self, date: NaiveDate, kind: InstrumentKind) -> bool;

    /// Calculates the expiry date for a given year and month.
    fn expiry_for_month(&self, year: i32, month: u32, kind: InstrumentKind) -> Option<NaiveDate>;

    /// Adjusts an expiry date if it falls on a market holiday (usually moves to previous trading day).
    fn adjust_for_holiday(&self, expiry: NaiveDate, calendar: &dyn MarketCalendar) -> NaiveDate {
        let mut d = expiry;
        while !calendar.is_trading_day(d) {
            d -= Duration::days(1);
        }
        d
    }
}

/// Standard monthly expiry rule: last Thursday of the month.
#[derive(Clone, Copy, Debug, Default)]
pub struct LastThursdayExpiry;

impl ExpiryRules for LastThursdayExpiry {
    fn is_expiry_date(&self, date: NaiveDate, kind: InstrumentKind) -> bool {
        match kind {
            InstrumentKind::Future | InstrumentKind::Option => {
                if date.weekday() != Weekday::Thu {
                    return false;
                }
                // Next week must be in next month
                let next_week = date + Duration::days(7);
                next_week.month() != date.month()
            }
            _ => false,
        }
    }

    fn expiry_for_month(&self, year: i32, month: u32, kind: InstrumentKind) -> Option<NaiveDate> {
        match kind {
            InstrumentKind::Future | InstrumentKind::Option => {
                // Start from the end of the month
                let next_month = if month == 12 { 1 } else { month + 1 };
                let next_year = if month == 12 { year + 1 } else { year };
                let first_of_next = NaiveDate::from_ymd_opt(next_year, next_month, 1)?;
                let mut d = first_of_next - Duration::days(1);
                while d.weekday() != Weekday::Thu {
                    d -= Duration::days(1);
                }
                Some(d)
            }
            _ => None,
        }
    }
}
