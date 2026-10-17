//! Unit tests for this crate, one file per area.

mod aggregator;

mod calendar;
mod costs;
mod expiry;
#[cfg(feature = "india")]
mod india_costs;
#[cfg(feature = "india")]
mod india_exchange;
mod rules;
mod settlement;
mod universes;

use chrono::NaiveDate;

use crate::{MarketCalendar, Session};

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

/// A weekday calendar with an explicit holiday list.
struct Holidays(Vec<NaiveDate>);

impl MarketCalendar for Holidays {
    fn session(&self) -> Session {
        Session::regular()
    }

    fn is_holiday(&self, date: NaiveDate) -> bool {
        self.0.contains(&date)
    }
}
