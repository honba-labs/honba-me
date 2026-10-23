//! Loader tests: parsers, gap detection and idempotent backfill. No network.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use honba_core::rust_decimal::Decimal;
use honba_core::{Bar, Exchange, InstrumentId, Price, Quantity, Timeframe};
use honba_feed::Lakehouse;
use honba_loaders::bhavcopy::{bhavcopy_url, delivery_url, session_close_utc};
use honba_loaders::*;

fn d(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}
fn day(y: i32, m: u32, dd: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, dd).unwrap()
}
fn infy() -> InstrumentId {
    InstrumentId::new(Exchange::Nse, "INFY").with_token("408065")
}

struct TempDir(PathBuf);
impl TempDir {
    fn new(n: &str) -> Self {
        let p = std::env::temp_dir().join(format!("honba_loaders_{}_{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const BHAV: &str = "\
SYMBOL, SERIES, DATE1, PREV_CLOSE, OPEN_PRICE, HIGH_PRICE, LOW_PRICE, LAST_PRICE, CLOSE_PRICE, AVG_PRICE, TTL_TRD_QNTY, TURNOVER_LACS, NO_OF_TRADES, DELIV_QTY, DELIV_PER
RELIANCE, EQ, 20-Oct-2026, 2890.10, 2900.00, 2925.50, 2888.00, 2920.00, 2918.75, 2910.33, 5123456, 149112.50, 210345, 2400000, 46.85
SUSPENDED, BE, 20-Oct-2026, 10.00, -, -, -, -, -, -, 0, 0, 0, -, -
INFY, EQ, 20-Oct-2026, 1500.00, 1502.00, 1510.00, 1495.00, 1505.00, 1506.50, 1503.20, 3000000, 45096.00, 90000, -, -
";

#[test]
fn bhavcopy_parses_exact_decimals_and_skips_suspended_rows() {
    let recs = parse_bhavcopy_csv(BHAV, day(2026, 10, 19)).unwrap();
    assert_eq!(recs.len(), 2, "the `-` row is skipped");
    let r = &recs[0];
    assert_eq!(r.bar.instrument.symbol, "RELIANCE");
    assert_eq!(
        r.date,
        day(2026, 10, 20),
        "DATE1 wins over the fallback date"
    );
    assert_eq!(r.bar.close, Price(d("2918.75")));
    assert_eq!(r.bar.volume, Quantity(d("5123456")));
    assert_eq!(r.vwap, Some(d("2910.33")));
    assert_eq!(r.prev_close, Some(d("2890.10")));
    assert_eq!(r.trades, Some(210345));
    assert_eq!(r.deliverable_pct, Some(d("46.85")));
    assert_eq!(r.bar.timestamp, session_close_utc(day(2026, 10, 20)));
    assert_eq!(
        r.bar.timestamp,
        Utc.with_ymd_and_hms(2026, 10, 20, 10, 0, 0).unwrap(),
        "15:30 IST"
    );
    assert_eq!(recs[1].deliverable_qty, None);
}

#[test]
fn bhavcopy_missing_required_column_is_an_error() {
    let err = parse_bhavcopy_csv("SYMBOL,SERIES\nX,EQ\n", day(2026, 10, 20)).unwrap_err();
    assert!(err.to_string().contains("OPEN_PRICE"), "{err}");
}

#[test]
fn delivery_report_parses_and_merges_only_missing_fields() {
    let mto = "\
Security Wise Delivery Position - Compulsory Rolling Settlement
10,MTO,20102026,1234567
Trade Date <20-OCT-2026>,Settlement Type <N>
Record Type,Sr No,Name of Security,Type of Security,Quantity Traded,Deliverable Quantity(gross across client level),% of Deliverable Quantity to Traded Quantity
20,1,INFY,EQ,3000000,1800000,60.00
20,2,RELIANCE,EQ,5123456,9999999,99.99
20,3,JUNK,EQ,notnum,1,1
";
    let dels = parse_delivery_dat(mto);
    assert_eq!(dels.len(), 2, "header lines and unparsable rows ignored");
    let mut recs = parse_bhavcopy_csv(BHAV, day(2026, 10, 20)).unwrap();
    let filled = merge_delivery(&mut recs, &dels);
    assert_eq!(
        filled, 1,
        "RELIANCE already had delivery data; only INFY filled"
    );
    assert_eq!(
        recs[0].deliverable_qty,
        Some(d("2400000")),
        "existing value not overwritten"
    );
    assert_eq!(recs[1].deliverable_qty, Some(d("1800000")));
    assert_eq!(recs[1].deliverable_pct, Some(d("60.00")));
    assert_eq!(bars_of(&recs, true).len(), 2);
}

#[test]
fn nse_urls_are_dated_ddmmyyyy() {
    assert!(bhavcopy_url(day(2026, 10, 20)).ends_with("sec_bhavdata_full_20102026.csv"));
    assert!(delivery_url(day(2026, 10, 20)).ends_with("MTO_20102026.DAT"));
}

#[test]
fn kite_candles_decode_with_offset_timestamps() {
    let body = r#"{"status":"success","data":{"candles":[
        ["2026-10-20T09:15:00+0530",1500.5,1510,1499.25,1505.1,12345],
        ["2026-10-20T09:16:00+0530",1505.1,1506,1503,1504,999]]}}"#;
    let bars = parse_candles(body, &infy()).unwrap();
    assert_eq!(bars.len(), 2);
    assert_eq!(bars[0].open, Price(d("1500.5")));
    assert_eq!(bars[0].close, Price(d("1505.1")), "no f64 noise");
    assert_eq!(
        bars[0].timestamp,
        Utc.with_ymd_and_hms(2026, 10, 20, 3, 45, 0).unwrap()
    );
}

#[test]
fn upstox_candles_decode_rfc3339_sort_ascending_and_tolerate_oi() {
    let body = r#"{"status":"success","data":{"candles":[
        ["2026-10-21T00:00:00+05:30",101,102,100,101.5,500,0],
        ["2026-10-20T00:00:00+05:30",100,101,99,100.5,400,0]]}}"#;
    let bars = parse_candles(body, &infy()).unwrap();
    assert_eq!(
        bars[0].timestamp.date_naive(),
        day(2026, 10, 19),
        "newest-first input re-sorted"
    );
    assert_eq!(bars[1].volume, Quantity(d("500")));
}

#[test]
fn bad_candle_responses_fail_loudly() {
    for (body, needle) in [
        (
            r#"{"status":"error","message":"Invalid token"}"#,
            "Invalid token",
        ),
        (r#"{"status":"success","data":{}}"#, "data.candles"),
        (
            r#"{"data":{"candles":[["2026-10-20T09:15:00+0530",1,2]]}}"#,
            ">= 6 fields",
        ),
        (
            r#"{"data":{"candles":[["not a date",1,2,3,4,5]]}}"#,
            "timestamp",
        ),
        ("not json", "expected"),
    ] {
        let err = parse_candles(body, &infy()).unwrap_err().to_string();
        assert!(err.contains(needle), "{body} -> {err}");
    }
}

#[test]
fn upstox_url_orders_dates_to_then_from_and_encodes_the_key() {
    let l = UpstoxCandlesLoader::new("tok");
    let url = l.candles_url(
        "NSE_EQ|INE009A01021",
        "day",
        day(2026, 10, 1),
        day(2026, 10, 20),
    );
    assert_eq!(
        url,
        "https://api.upstox.com/v2/historical-candle/NSE_EQ%7CINE009A01021/day/2026-10-20/2026-10-01"
    );
    assert!(UpstoxCandlesLoader::interval_name(5).is_err());
    assert_eq!(KiteCandlesLoader::interval_name(5).unwrap(), "5minute");
    assert_eq!(KiteCandlesLoader::interval_name(1440).unwrap(), "day");
}

#[test]
fn missing_trading_days_skips_weekends_holidays_and_present_days() {
    // Mon 2026-10-19 .. Sun 2026-10-25
    let have: BTreeSet<_> = [day(2026, 10, 19), day(2026, 10, 22)].into();
    let holidays: BTreeSet<_> = [day(2026, 10, 21)].into();
    let miss = missing_trading_days(&have, day(2026, 10, 19), day(2026, 10, 25), &holidays);
    assert_eq!(miss, [day(2026, 10, 20), day(2026, 10, 23)]);
}

#[test]
fn intraday_gaps_are_merged_into_runs() {
    let date = day(2026, 10, 20);
    assert_eq!(expected_session_bars(date, 1).len(), 375, "09:15..15:29");
    assert_eq!(expected_session_bars(date, 15).len(), 25);
    assert_eq!(
        expected_session_bars(date, 60).len(),
        7,
        "incl. partial 15:15 bar"
    );

    let mut have = expected_session_bars(date, 5);
    let removed: Vec<_> = have.drain(10..13).collect(); // 3 contiguous
    have.remove(20); // + 1 isolated
    let gaps = missing_intraday_bars(&have, date, 5);
    assert_eq!(gaps.len(), 2);
    assert_eq!(
        (gaps[0].start, gaps[0].end, gaps[0].bars),
        (removed[0], removed[2], 3)
    );
    assert_eq!(gaps[1].bars, 1);
    assert!(missing_intraday_bars(&expected_session_bars(date, 5), date, 5).is_empty());
}

/// Source that serves a fixed bar set and records how it was called.
struct Mock {
    daily: Vec<Bar>,
    intraday: Vec<Bar>,
    calls: Mutex<Vec<String>>,
}

#[async_trait]
impl HistoricalBarSource for Mock {
    async fn daily_bars(
        &self,
        _: &InstrumentId,
        from: NaiveDate,
        to: NaiveDate,
    ) -> LoaderResult<Vec<Bar>> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("daily {from}..{to}"));
        Ok(self.daily.clone())
    }
    async fn intraday_bars(
        &self,
        _: &InstrumentId,
        date: NaiveDate,
        m: u32,
    ) -> LoaderResult<Vec<Bar>> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("intraday {date} {m}m"));
        Ok(self.intraday.clone())
    }
}

fn bar(ts: DateTime<Utc>, close: &str) -> Bar {
    Bar {
        instrument: InstrumentId::new(Exchange::Nse, "INFY"),
        open: Price(d(close)),
        high: Price(d(close)),
        low: Price(d(close)),
        close: Price(d(close)),
        volume: Quantity(d("10")),
        timestamp: ts,
    }
}

#[tokio::test]
async fn daily_reconcile_fetches_only_the_gap_and_is_idempotent() {
    let tmp = TempDir::new("daily");
    let lake = Lakehouse::new(&tmp.0);
    let at = |dd| session_close_utc(day(2026, 10, dd));
    // Local store has Mon 19 and Thu 22; Tue 20, Wed 21 (holiday) and Fri 23 are absent.
    lake.write_bars(Timeframe::D1, &[bar(at(19), "100"), bar(at(22), "103")])
        .unwrap();
    let src = Mock {
        daily: vec![bar(at(19), "999"), bar(at(20), "101"), bar(at(23), "104")],
        intraday: vec![],
        calls: Mutex::default(),
    };
    let holidays: BTreeSet<_> = [day(2026, 10, 21)].into();
    let inst = InstrumentId::new(Exchange::Nse, "INFY").with_token("1");

    let r = reconcile_daily(
        &lake,
        &src,
        &inst,
        day(2026, 10, 19),
        day(2026, 10, 23),
        &holidays,
    )
    .await
    .unwrap();
    assert_eq!((r.missing, r.fetched, r.written), (2, 3, 2));
    assert!(r.unfilled_days.is_empty());
    assert_eq!(
        src.calls.lock().unwrap()[0],
        "daily 2026-10-20..2026-10-23",
        "tight range"
    );

    let lo = at(19) - chrono::Duration::hours(1);
    let hi = at(23) + chrono::Duration::hours(1);
    let bars = lake.read_bars_range(&inst, Timeframe::D1, lo, hi).unwrap();
    let closes: Vec<_> = bars.iter().map(|b| b.close.0).collect();
    assert_eq!(
        closes,
        [d("100"), d("101"), d("103"), d("104")],
        "existing 19th (100) kept, not 999"
    );

    let again = reconcile_daily(
        &lake,
        &src,
        &inst,
        day(2026, 10, 19),
        day(2026, 10, 23),
        &holidays,
    )
    .await
    .unwrap();
    assert_eq!((again.missing, again.written), (0, 0));
    assert_eq!(
        src.calls.lock().unwrap().len(),
        1,
        "no broker call when nothing is missing"
    );
}

#[tokio::test]
async fn daily_reconcile_reports_days_the_broker_could_not_fill() {
    let tmp = TempDir::new("unfilled");
    let lake = Lakehouse::new(&tmp.0);
    let src = Mock {
        daily: vec![],
        intraday: vec![],
        calls: Mutex::default(),
    };
    let inst = infy();
    let r = reconcile_daily(
        &lake,
        &src,
        &inst,
        day(2026, 10, 20),
        day(2026, 10, 20),
        &BTreeSet::new(),
    )
    .await
    .unwrap();
    assert_eq!(r.unfilled_days, [day(2026, 10, 20)]);
}

#[tokio::test]
async fn intraday_reconcile_backfills_only_missing_slots() {
    let tmp = TempDir::new("intraday");
    let lake = Lakehouse::new(&tmp.0);
    let date = day(2026, 10, 20);
    let slots = expected_session_bars(date, 15);
    // Local has all but slots 3 and 4; the broker returns everything (with different closes).
    let local: Vec<_> = slots
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != 3 && *i != 4)
        .map(|(_, t)| bar(*t, "100"))
        .collect();
    lake.write_bars(Timeframe::M15, &local).unwrap();
    let src = Mock {
        daily: vec![],
        intraday: slots.iter().map(|t| bar(*t, "555")).collect(),
        calls: Mutex::default(),
    };
    let inst = infy();
    let r = reconcile_intraday(&lake, &src, &inst, Timeframe::M15, date)
        .await
        .unwrap();
    assert_eq!((r.missing, r.fetched, r.written), (2, 25, 2));
    assert_eq!(src.calls.lock().unwrap()[0], "intraday 2026-10-20 15m");

    let all = lake
        .read_bars_range(&inst, Timeframe::M15, slots[0], *slots.last().unwrap())
        .unwrap();
    assert_eq!(all.len(), 25);
    assert_eq!(all[3].close.0, d("555"));
    assert_eq!(all[2].close.0, d("100"), "existing bars untouched");

    let again = reconcile_intraday(&lake, &src, &inst, Timeframe::M15, date)
        .await
        .unwrap();
    assert_eq!((again.missing, again.written), (0, 0));

    let err = reconcile_intraday(&lake, &src, &inst, Timeframe::D1, date)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("not an intraday"), "{err}");
}
