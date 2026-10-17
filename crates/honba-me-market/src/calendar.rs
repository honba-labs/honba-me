//! Generic trading session, market calendar, and holiday contracts.

use chrono::{Datelike, Duration, NaiveDate, NaiveTime, Weekday};
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::Result;

/// A trading session window on a given day.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    open: NaiveTime,
    close: NaiveTime,
}

impl Session {
    /// Creates a session between the given times.
    pub fn new(open: NaiveTime, close: NaiveTime) -> Self {
        debug_assert!(close > open, "session close must be after open");
        Self { open, close }
    }

    /// The default 24-hour continuous session (e.g. for crypto or null pack).
    pub fn continuous_24h() -> Self {
        Self {
            open: NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
            close: NaiveTime::from_hms_opt(23, 59, 59).unwrap(),
        }
    }

    /// The regular equity session (09:15 to 15:30).
    pub fn regular() -> Self {
        Self {
            open: NaiveTime::from_hms_opt(9, 15, 0).unwrap(),
            close: NaiveTime::from_hms_opt(15, 30, 0).unwrap(),
        }
    }

    /// The opening time.
    pub fn open(&self) -> NaiveTime {
        self.open
    }

    /// The closing time.
    pub fn close(&self) -> NaiveTime {
        self.close
    }

    /// Returns `true` if the time falls within `[open, close)`.
    pub fn contains(&self, t: NaiveTime) -> bool {
        t >= self.open && t < self.close
    }
}

impl fmt::Display for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.open, self.close)
    }
}

/// A source of declared trading-holiday dates.
pub trait HolidaySource {
    /// Returns every declared trading holiday known to the source.
    fn holidays(&self) -> Result<Vec<NaiveDate>>;
}

/// A trading calendar: sessions, holidays, and trading-day date arithmetic.
pub trait MarketCalendar: Send + Sync {
    /// Returns the primary session window used on regular trading days.
    fn session(&self) -> Session;

    /// Returns `true` if the given date is a declared market holiday.
    fn is_holiday(&self, date: NaiveDate) -> bool;

    /// Returns `true` if the given date is a trading day.
    fn is_trading_day(&self, date: NaiveDate) -> bool {
        !matches!(date.weekday(), Weekday::Sat | Weekday::Sun) && !self.is_holiday(date)
    }

    /// Returns `true` if the given date is a settlement day.
    /// By default, settlement days match trading days.
    fn is_settlement_day(&self, date: NaiveDate) -> bool {
        self.is_trading_day(date)
    }

    /// Returns the next trading day strictly after `date`.
    fn next_trading_day(&self, date: NaiveDate) -> NaiveDate {
        let mut d = date + Duration::days(1);
        while !self.is_trading_day(d) {
            d += Duration::days(1);
        }
        d
    }

    /// Returns the previous trading day strictly before `date`.
    fn prev_trading_day(&self, date: NaiveDate) -> NaiveDate {
        let mut d = date - Duration::days(1);
        while !self.is_trading_day(d) {
            d -= Duration::days(1);
        }
        d
    }

    /// Returns the number of trading days in `[start, end]`.
    fn trading_days_between(&self, start: NaiveDate, end: NaiveDate) -> usize {
        let mut count = 0;
        let mut d = start;
        while d <= end {
            if self.is_trading_day(d) {
                count += 1;
            }
            d += Duration::days(1);
        }
        count
    }
}

/// Backward compatibility alias for [`MarketCalendar`].
pub use MarketCalendar as TradingCalendar;

impl<T: MarketCalendar + ?Sized> MarketCalendar for std::sync::Arc<T> {
    fn session(&self) -> Session {
        (**self).session()
    }
    fn is_holiday(&self, date: NaiveDate) -> bool {
        (**self).is_holiday(date)
    }
    fn is_trading_day(&self, date: NaiveDate) -> bool {
        (**self).is_trading_day(date)
    }
}

impl<T: MarketCalendar + ?Sized> MarketCalendar for Box<T> {
    fn session(&self) -> Session {
        (**self).session()
    }
    fn is_holiday(&self, date: NaiveDate) -> bool {
        (**self).is_holiday(date)
    }
    fn is_trading_day(&self, date: NaiveDate) -> bool {
        (**self).is_trading_day(date)
    }
}
