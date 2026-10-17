//! NSE trading calendar, built from injected holiday data.

use std::collections::HashSet;

use chrono::{NaiveDate, NaiveTime};

use super::super::error::Result;

use super::source::{HolidaySource, TradingCalendar};

pub use crate::calendar::Session;

/// The NSE trading calendar.
#[derive(Clone, Debug)]
pub struct NseCalendar {
    holidays: HashSet<NaiveDate>,
    session: Session,
}

impl NseCalendar {
    /// Builds a calendar from an explicit set of holiday dates.
    pub fn from_holidays(holidays: impl IntoIterator<Item = NaiveDate>) -> Self {
        Self {
            holidays: holidays.into_iter().collect(),
            session: Session::new(
                NaiveTime::from_hms_opt(9, 15, 0).unwrap(),
                NaiveTime::from_hms_opt(15, 30, 0).unwrap(),
            ),
        }
    }

    /// Builds a calendar by querying a [`HolidaySource`].
    pub fn from_source<S: HolidaySource + ?Sized>(source: &S) -> Result<Self> {
        Ok(Self::from_holidays(source.holidays()?))
    }

    /// Overrides the default session window.
    pub fn with_session(mut self, session: Session) -> Self {
        self.session = session;
        self
    }

    /// Iterates over the declared holidays.
    pub fn holidays(&self) -> impl Iterator<Item = &NaiveDate> {
        self.holidays.iter()
    }
}

impl TradingCalendar for NseCalendar {
    fn session(&self) -> Session {
        self.session
    }

    fn is_holiday(&self, date: NaiveDate) -> bool {
        self.holidays.contains(&date)
    }
}
