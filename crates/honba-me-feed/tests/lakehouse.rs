//! Lakehouse v2 + TickRecorder integration tests. Fixtures live in a temp dir; no network.

use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use honba_core::rust_decimal::Decimal;
use honba_core::{
    Bar, BarReader, BarRequest, DepthLevel, Exchange, InstrumentId, Price, PriceBand, Quantity,
    Tick, TickReader, TickRequest, Timeframe,
};
use honba_feed::lakehouse::{bar_schema_v2, decimal_type, ist_date, tick_schema_v2};
use honba_feed::{Lakehouse, LakehouseCompression, RecorderConfig, TickRecorder};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("honba_lake_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn d(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}
fn px(s: &str) -> Price {
    Price(d(s))
}
fn qty(n: u64) -> Quantity {
    Quantity::from_u64(n)
}
fn infy() -> InstrumentId {
    InstrumentId::new(Exchange::Nse, "INFY")
}
/// 2026-10-20 09:15:00 IST + `sec` seconds.
fn at(sec: i64) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 20, 3, 45, 0).unwrap() + chrono::Duration::seconds(sec)
}

fn ltp(sym: &str, price: &str, sec: i64) -> Tick {
    Tick::Ltp {
        instrument: InstrumentId::new(Exchange::Nse, sym),
        price: px(price),
        volume: qty(100),
        exchange_ts: at(sec),
        receive_ts: at(sec) + chrono::Duration::milliseconds(3),
    }
}

fn quote(sec: i64) -> Tick {
    Tick::Quote {
        instrument: infy().with_token("408065").with_isin("INE009A01021"),
        bid: px("1499.95"),
        ask: px("1500.05"),
        bid_qty: qty(10),
        ask_qty: qty(20),
        last_price: Some(px("1500.00")),
        exchange_ts: at(sec),
        receive_ts: at(sec),
        band: Some(PriceBand {
            lower: px("1350.00"),
            upper: px("1650.00"),
            reference_price: px("1500.00"),
            band_pct: Some(d("0.10")),
        }),
    }
}

fn depth(sec: i64) -> Tick {
    let lvl = |p: &str, q: u64, o: u32| DepthLevel {
        price: px(p),
        quantity: qty(q),
        orders_count: Some(o),
    };
    Tick::Depth {
        instrument: infy(),
        bids: vec![lvl("1499.95", 10, 2), lvl("1499.90", 30, 4)],
        asks: vec![lvl("1500.05", 20, 3), lvl("1500.10", 5, 1)],
        last_price: None,
        exchange_ts: at(sec),
        receive_ts: at(sec),
    }
}

fn window() -> (DateTime<Utc>, DateTime<Utc>) {
    (at(-3600), at(3600))
}

#[test]
fn schemas_use_exact_decimals_and_no_floats() {
    for schema in [tick_schema_v2(), bar_schema_v2()] {
        for f in schema.fields() {
            assert!(
                !matches!(
                    f.data_type(),
                    arrow::datatypes::DataType::Float32 | arrow::datatypes::DataType::Float64
                ),
                "{} is floating point",
                f.name()
            );
        }
    }
    assert_eq!(
        tick_schema_v2()
            .field_with_name("price")
            .unwrap()
            .data_type(),
        &decimal_type()
    );
}

#[test]
fn every_tick_kind_round_trips_exactly() {
    let tmp = TempDir::new("roundtrip");
    let lake = Lakehouse::new(&tmp.0);
    let ticks = vec![ltp("INFY", "1500.25", 0), quote(1), depth(2)];
    lake.write_ticks(&ticks).unwrap();

    let (from, to) = window();
    let back = lake.read_ticks_range(&infy(), from, to).unwrap();
    assert_eq!(back, ticks, "decimals, token/isin, band and depth survive");
}

#[test]
fn files_land_in_hive_partitions_by_ist_date() {
    let tmp = TempDir::new("layout");
    let lake = Lakehouse::new(&tmp.0);
    // 2026-10-20 20:00 UTC is already 21 Oct 01:30 IST -> next trading-date partition.
    let late = Tick::Ltp {
        instrument: infy(),
        price: px("1.00"),
        volume: qty(1),
        exchange_ts: Utc.with_ymd_and_hms(2026, 10, 20, 20, 0, 0).unwrap(),
        receive_ts: Utc.with_ymd_and_hms(2026, 10, 20, 20, 0, 0).unwrap(),
    };
    let mut paths = lake
        .write_ticks(&[ltp("INFY", "1500", 0), ltp("RELIANCE", "2900", 0), late])
        .unwrap();
    paths.sort();
    let rel: Vec<String> = paths
        .iter()
        .map(|p| {
            p.strip_prefix(&tmp.0)
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(
        rel,
        [
            "ticks/exchange=NSE/date=2026-10-20/INFY.parquet",
            "ticks/exchange=NSE/date=2026-10-20/RELIANCE.parquet",
            "ticks/exchange=NSE/date=2026-10-21/INFY.parquet",
        ]
    );
}

#[test]
fn repeated_writes_append_part_files_and_read_back_merged() {
    let tmp = TempDir::new("append");
    let lake = Lakehouse::new(&tmp.0).with_compression(LakehouseCompression::Snappy);
    lake.write_ticks(&[ltp("INFY", "1500", 0)]).unwrap();
    let second = lake.write_ticks(&[ltp("INFY", "1501", 5)]).unwrap();
    let third = lake.write_ticks(&[ltp("INFY", "1502", 2)]).unwrap();
    assert!(second[0].ends_with("INFY.part-0001.parquet"));
    assert!(third[0].ends_with("INFY.part-0002.parquet"));
    assert!(
        std::fs::read_dir(second[0].parent().unwrap())
            .unwrap()
            .all(|e| !e.unwrap().path().to_string_lossy().ends_with(".tmp")),
        "no temp files left behind"
    );

    let (from, to) = window();
    let prices: Vec<_> = lake
        .read_ticks_range(&infy(), from, to)
        .unwrap()
        .iter()
        .map(|t| t.last_price().unwrap().0)
        .collect();
    assert_eq!(
        prices,
        [d("1500"), d("1502"), d("1501")],
        "merged and time-sorted"
    );
}

#[test]
fn reads_are_scoped_to_instrument_and_inclusive_time_range() {
    let tmp = TempDir::new("scope");
    let lake = Lakehouse::new(&tmp.0);
    lake.write_ticks(&[
        ltp("INFY", "1", 0),
        ltp("INFY", "2", 10),
        ltp("INFY", "3", 20),
        ltp("TCS", "9", 10),
    ])
    .unwrap();
    let got = lake.read_ticks_range(&infy(), at(10), at(20)).unwrap();
    assert_eq!(got.len(), 2, "both bounds inclusive");
    assert!(got.iter().all(|t| t.instrument().symbol == "INFY"));
    let bse = InstrumentId::new(Exchange::Bse, "INFY");
    assert!(
        lake.read_ticks_range(&bse, at(-100), at(100))
            .unwrap()
            .is_empty()
    );
    let missing = Lakehouse::new(tmp.0.join("nope"));
    assert!(
        missing
            .read_ticks_range(&infy(), at(0), at(1))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn prices_are_rounded_to_the_column_scale() {
    let tmp = TempDir::new("scale");
    let lake = Lakehouse::new(&tmp.0);
    lake.write_ticks(&[ltp("INFY", "100.123456", 0)]).unwrap();
    let (from, to) = window();
    let t = lake.read_ticks_range(&infy(), from, to).unwrap();
    assert_eq!(t[0].last_price().unwrap().0, d("100.1235"));
}

#[test]
fn out_of_range_prices_are_rejected_not_truncated() {
    let tmp = TempDir::new("overflow");
    let lake = Lakehouse::new(&tmp.0);
    let err = lake
        .write_ticks(&[ltp("INFY", "100000000000000", 0)])
        .unwrap_err();
    assert!(err.to_string().contains("does not fit"), "{err}");
}

#[test]
fn bars_round_trip_across_month_partitions() {
    let tmp = TempDir::new("bars");
    let lake = Lakehouse::new(&tmp.0);
    let bar = |y, m, day, close: &str| Bar {
        instrument: infy(),
        open: px("100.00"),
        high: px("110.50"),
        low: px("99.25"),
        close: px(close),
        volume: qty(12345),
        timestamp: Utc.with_ymd_and_hms(y, m, day, 10, 0, 0).unwrap(),
    };
    let bars = vec![
        bar(2026, 9, 30, "101"),
        bar(2026, 10, 1, "102"),
        bar(2026, 10, 2, "103"),
    ];
    let paths = lake.write_bars(Timeframe::D1, &bars).unwrap();
    assert_eq!(paths.len(), 2);
    assert!(
        paths
            .iter()
            .any(|p| p.ends_with("bars/timeframe=1d/exchange=NSE/year=2026/month=09/INFY.parquet"))
    );

    let req = BarRequest {
        instrument: infy(),
        timeframe: Timeframe::D1,
        from: Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap(),
        to: Utc.with_ymd_and_hms(2026, 10, 2, 0, 0, 0).unwrap(),
    };
    let got = lake.read_bars(&req).unwrap();
    assert_eq!(got, bars[..2], "inclusive window across a month boundary");
    let other_tf = BarRequest {
        timeframe: Timeframe::M1,
        ..req
    };
    assert!(lake.read_bars(&other_tf).unwrap().is_empty());
}

#[test]
fn lakehouse_serves_the_canonical_core_ports() {
    let tmp = TempDir::new("ports");
    let lake = Lakehouse::new(&tmp.0);
    lake.write_ticks(&[quote(0)]).unwrap();
    let (from, to) = window();
    let reader: &dyn TickReader = &lake;
    let got = reader
        .read_ticks(&TickRequest {
            instrument: infy(),
            from,
            to,
        })
        .unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(ist_date(at(0)).to_string(), "2026-10-20");
}

fn fast_config(max_ticks: usize) -> RecorderConfig {
    RecorderConfig {
        max_ticks,
        flush_interval: Duration::from_secs(3600),
        ..RecorderConfig::default()
    }
}

#[tokio::test]
async fn recorder_flushes_at_max_ticks_and_on_shutdown() {
    let tmp = TempDir::new("rec_size");
    let lake = Lakehouse::new(&tmp.0);
    let rec = TickRecorder::spawn(lake.clone(), fast_config(3));
    for i in 0..7 {
        assert!(rec.record(ltp("INFY", "1500", i)).await);
    }
    let stats = rec.shutdown().await.unwrap();
    assert_eq!(stats.received, 7);
    assert_eq!(stats.written, 7);
    assert_eq!(stats.flushes, 3, "3 + 3 on size, remaining 1 on shutdown");
    assert_eq!(stats.files, 3);
    assert_eq!(stats.failed_flushes + stats.dropped, 0);
    let (from, to) = window();
    assert_eq!(lake.read_ticks_range(&infy(), from, to).unwrap().len(), 7);
}

#[tokio::test]
async fn recorder_flushes_on_the_interval() {
    let tmp = TempDir::new("rec_timer");
    let lake = Lakehouse::new(&tmp.0);
    let cfg = RecorderConfig {
        max_ticks: 1_000_000,
        flush_interval: Duration::from_millis(100),
        ..RecorderConfig::default()
    };
    let rec = TickRecorder::spawn(lake.clone(), cfg);
    assert!(rec.record(ltp("INFY", "1500", 0)).await);
    let (from, to) = window();
    let mut seen = 0;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        seen = lake.read_ticks_range(&infy(), from, to).unwrap().len();
        if seen > 0 {
            break;
        }
    }
    assert_eq!(seen, 1, "timer flushed without hitting max_ticks or shutdown");
    let stats = rec.shutdown().await.unwrap();
    assert_eq!(stats.written, 1);
}

#[tokio::test]
async fn recorder_ingests_a_broadcast_stream() {
    let tmp = TempDir::new("rec_bcast");
    let lake = Lakehouse::new(&tmp.0);
    let rec = TickRecorder::spawn(lake.clone(), fast_config(1000));
    let (tx, rx) = tokio::sync::broadcast::channel(64);
    let forwarder = rec.attach_broadcast(rx);
    for i in 0..5 {
        tx.send(ltp("INFY", "1500", i)).unwrap();
    }
    drop(tx);
    assert_eq!(forwarder.await.unwrap(), 0, "nothing lagged");
    let stats = rec.shutdown().await.unwrap();
    assert_eq!(stats.written, 5);
}

#[tokio::test]
async fn failed_flush_keeps_ticks_and_retries_then_surfaces_the_error() {
    let tmp = TempDir::new("rec_fail");
    // Make the lakehouse root a *file* so every create_dir_all fails.
    let blocked = tmp.0.join("blocked");
    std::fs::write(&blocked, b"x").unwrap();
    let rec = TickRecorder::spawn(Lakehouse::new(&blocked), fast_config(2));
    for i in 0..4 {
        rec.record(ltp("INFY", "1500", i)).await;
    }
    let err = rec.shutdown().await.unwrap_err();
    assert!(err.to_string().contains("io error"), "{err}");
}

#[test]
fn legacy_tick_store_migrates_into_the_lakehouse_as_exact_decimals() {
    use honba_feed::canonical::{bars_to_canonical, decimal_from_f64, migrate_tick_store};
    use honba_feed::wire::{
        AggressorSide, Bar as WireBar, BarAggregation, BarSpecification, BarType, PriceType,
        QuoteTick, TradeId, TradeTick, UnixNanos,
    };
    use honba_feed::write_tick_file;

    assert_eq!(decimal_from_f64(1500.1), Some(d("1500.1")), "no binary-float noise");
    assert_eq!(decimal_from_f64(f64::NAN), None);

    let tmp = TempDir::new("migrate");
    let legacy = tmp.0.join("legacy");
    let ns = |s: i64| UnixNanos::from_datetime(at(s));
    let quote = QuoteTick::new(infy(), 1499.95, 1500.05, 10.0, 20.0, ns(0), ns(0));
    let trade = TradeTick::new(
        infy(), 1500.0, 5.0, AggressorSide::Buyer, TradeId::new("t1"), ns(1), ns(1),
    );
    write_tick_file(&legacy.join("ticks").join("a.parquet"), &[quote], &[trade]).unwrap();

    let lake = Lakehouse::new(tmp.0.join("lake"));
    assert_eq!(migrate_tick_store(&legacy, &lake).unwrap(), 2);
    let (from, to) = window();
    let ticks = lake.read_ticks_range(&infy(), from, to).unwrap();
    assert!(matches!(&ticks[0], Tick::Quote { bid, .. } if bid.0 == d("1499.95")));
    assert!(matches!(&ticks[1], Tick::Ltp { price, volume, .. }
        if price.0 == d("1500") && volume.0 == d("5")));

    let spec = BarSpecification::new(1, BarAggregation::Day, PriceType::Last);
    let wire_bar = WireBar::new(
        BarType::new(infy(), spec), 100.5, 101.0, 99.5, 100.75, 1000.0, ns(0), ns(0),
    );
    let bars = bars_to_canonical(&[wire_bar]);
    assert_eq!(bars[0].close.0, d("100.75"));
    assert_eq!(bars[0].timestamp, at(0));
}
