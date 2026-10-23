//! Historical gap detection and broker backfill.
//!
//! Compares what the local [`Lakehouse`] holds against what the NSE session *should* contain,
//! then asks a [`HistoricalBarSource`] (Kite / Upstox) for just the missing bars and writes
//! them back. The detection functions are pure; only [`reconcile_daily`] and
//! [`reconcile_intraday`] touch I/O.
//!
//! Backfill never overwrites: only bars for slots that were *missing* are written, so
//! re-running a reconcile is idempotent.

use std::collections::BTreeSet;

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, TimeZone, Utc, Weekday};
use honba_core::{Bar, InstrumentId, Timeframe};
use honba_feed::Lakehouse;
use honba_feed::lakehouse::ist_date;

use crate::candles::HistoricalBarSource;
use crate::{LoaderError, LoaderResult, ist};

/// A contiguous run of missing intraday bars, `start` inclusive to `end` inclusive (bar-open
/// instants, UTC).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GapRange {
    /// Open of the first missing bar.
    pub start: DateTime<Utc>,
    /// Open of the last missing bar.
    pub end: DateTime<Utc>,
    /// Number of missing bars in the run.
    pub bars: usize,
}

/// What a reconcile pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconcileReport {
    /// Slots (days or bars) found missing locally.
    pub missing: usize,
    /// Bars the broker returned (before filtering to missing slots).
    pub fetched: usize,
    /// Bars written to the lakehouse.
    pub written: usize,
    /// Daily gaps the broker could not fill (e.g. exchange holiday not in the calendar).
    pub unfilled_days: Vec<NaiveDate>,
}

/// Weekdays in `from..=to` that are neither in `have` nor in `holidays`.
pub fn missing_trading_days(
    have: &BTreeSet<NaiveDate>,
    from: NaiveDate,
    to: NaiveDate,
    holidays: &BTreeSet<NaiveDate>,
) -> Vec<NaiveDate> {
    let mut out = Vec::new();
    let mut d = from;
    while d <= to {
        let weekend = matches!(d.weekday(), Weekday::Sat | Weekday::Sun);
        if !weekend && !have.contains(&d) && !holidays.contains(&d) {
            out.push(d);
        }
        match d.succ_opt() {
            Some(n) => d = n,
            None => break,
        }
    }
    out
}

fn ist_instant(date: NaiveDate, h: u32, m: u32) -> DateTime<Utc> {
    let t = NaiveTime::from_hms_opt(h, m, 0).expect("valid time");
    ist()
        .from_local_datetime(&date.and_time(t))
        .single()
        .expect("fixed offset is unambiguous")
        .with_timezone(&Utc)
}

/// Bar-open instants an NSE session on `date` should contain: 09:15 IST stepping by
/// `interval_minutes` while the bar opens before the 15:30 close (a trailing partial bar, e.g.
/// the 15:15 hourly bar, counts).
pub fn expected_session_bars(date: NaiveDate, interval_minutes: u32) -> Vec<DateTime<Utc>> {
    let (open, close) = (ist_instant(date, 9, 15), ist_instant(date, 15, 30));
    let step = Duration::minutes(i64::from(interval_minutes.max(1)));
    let mut out = Vec::new();
    let mut t = open;
    while t < close {
        out.push(t);
        t += step;
    }
    out
}

/// Missing intraday bars of `date`, merged into contiguous runs.
pub fn missing_intraday_bars(
    have: &[DateTime<Utc>],
    date: NaiveDate,
    interval_minutes: u32,
) -> Vec<GapRange> {
    let have: BTreeSet<_> = have.iter().copied().collect();
    let step = Duration::minutes(i64::from(interval_minutes.max(1)));
    let mut ranges: Vec<GapRange> = Vec::new();
    for t in expected_session_bars(date, interval_minutes) {
        if have.contains(&t) {
            continue;
        }
        match ranges.last_mut() {
            Some(r) if r.end + step == t => {
                r.end = t;
                r.bars += 1;
            }
            _ => ranges.push(GapRange {
                start: t,
                end: t,
                bars: 1,
            }),
        }
    }
    ranges
}

fn lake_err(e: impl std::fmt::Display) -> LoaderError {
    LoaderError::Lakehouse(e.to_string())
}

/// Finds missing daily bars of `instrument` in `from..=to` and backfills them from `source`.
pub async fn reconcile_daily<S: HistoricalBarSource>(
    lake: &Lakehouse,
    source: &S,
    instrument: &InstrumentId,
    from: NaiveDate,
    to: NaiveDate,
    holidays: &BTreeSet<NaiveDate>,
) -> LoaderResult<ReconcileReport> {
    let lo = ist_instant(from, 0, 0);
    let hi = ist_instant(to, 23, 59);
    let have: BTreeSet<NaiveDate> = lake
        .read_bars_range(instrument, Timeframe::D1, lo, hi)
        .map_err(lake_err)?
        .iter()
        .map(|b| ist_date(b.timestamp))
        .collect();
    let missing = missing_trading_days(&have, from, to, holidays);
    let mut report = ReconcileReport {
        missing: missing.len(),
        ..Default::default()
    };
    let (Some(first), Some(last)) = (missing.first().copied(), missing.last().copied()) else {
        return Ok(report);
    };
    let fetched = source.daily_bars(instrument, first, last).await?;
    report.fetched = fetched.len();
    let want: BTreeSet<_> = missing.iter().copied().collect();
    let fill: Vec<Bar> = fetched
        .into_iter()
        .filter(|b| want.contains(&ist_date(b.timestamp)))
        .collect();
    let got: BTreeSet<_> = fill.iter().map(|b| ist_date(b.timestamp)).collect();
    report.unfilled_days = missing.into_iter().filter(|d| !got.contains(d)).collect();
    if !fill.is_empty() {
        lake.write_bars(Timeframe::D1, &fill).map_err(lake_err)?;
    }
    report.written = fill.len();
    Ok(report)
}

/// Minutes in an intraday [`Timeframe`], or `None` for daily/weekly.
pub fn timeframe_minutes(tf: Timeframe) -> Option<u32> {
    match tf {
        Timeframe::M1 => Some(1),
        Timeframe::M5 => Some(5),
        Timeframe::M15 => Some(15),
        Timeframe::M30 => Some(30),
        Timeframe::H1 => Some(60),
        Timeframe::D1 | Timeframe::W1 => None,
    }
}

/// Finds missing intraday bars of one session and backfills them from `source`.
pub async fn reconcile_intraday<S: HistoricalBarSource>(
    lake: &Lakehouse,
    source: &S,
    instrument: &InstrumentId,
    timeframe: Timeframe,
    date: NaiveDate,
) -> LoaderResult<ReconcileReport> {
    let minutes = timeframe_minutes(timeframe).ok_or_else(|| {
        LoaderError::Unsupported(format!("{timeframe:?} is not an intraday timeframe"))
    })?;
    let (lo, hi) = (ist_instant(date, 9, 0), ist_instant(date, 15, 30));
    let have: Vec<_> = lake
        .read_bars_range(instrument, timeframe, lo, hi)
        .map_err(lake_err)?
        .iter()
        .map(|b| b.timestamp)
        .collect();
    let gaps = missing_intraday_bars(&have, date, minutes);
    let missing: usize = gaps.iter().map(|g| g.bars).sum();
    let mut report = ReconcileReport {
        missing,
        ..Default::default()
    };
    if gaps.is_empty() {
        return Ok(report);
    }
    let fetched = source.intraday_bars(instrument, date, minutes).await?;
    report.fetched = fetched.len();
    let missing_slots: BTreeSet<DateTime<Utc>> = expected_session_bars(date, minutes)
        .into_iter()
        .filter(|t| !have.contains(t))
        .collect();
    let fill: Vec<Bar> = fetched
        .into_iter()
        .filter(|b| missing_slots.contains(&b.timestamp))
        .collect();
    if !fill.is_empty() {
        lake.write_bars(timeframe, &fill).map_err(lake_err)?;
    }
    report.written = fill.len();
    Ok(report)
}
