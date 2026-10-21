//! Unit tests for `CatalogInstrumentMaster`: dated snapshots and resolution.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow::array::{ArrayRef, Float64Array, StringArray};
use arrow::record_batch::RecordBatch;
use chrono::NaiveDate;
use honba_entities::OptionType;
use honba_messages::{Exchange, InstrumentId};
use honba_ports::InstrumentMaster;
use parquet::arrow::ArrowWriter;

use crate::instruments::{CatalogInstrumentMaster, snapshot_schema};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "honba_instruments_unit_{}_{name}",
            std::process::id()
        ));
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

struct Row {
    symbol: &'static str,
    exchange: &'static str,
    kind: &'static str,
    currency: &'static str,
    lot_size: f64,
    tick_size: f64,
    freeze_qty: f64,
    expiry: Option<&'static str>,
    strike: Option<f64>,
    option_type: Option<&'static str>,
    underlying: Option<&'static str>,
}

fn equity(symbol: &'static str, lot_size: f64, freeze_qty: f64) -> Row {
    Row {
        symbol,
        exchange: "NSE",
        kind: "equity",
        currency: "INR",
        lot_size,
        tick_size: 0.05,
        freeze_qty,
        expiry: None,
        strike: None,
        option_type: None,
        underlying: None,
    }
}

fn future(symbol: &'static str, lot_size: f64, freeze_qty: f64, expiry: &'static str) -> Row {
    Row {
        symbol,
        exchange: "NSE",
        kind: "future",
        currency: "INR",
        lot_size,
        tick_size: 0.05,
        freeze_qty,
        expiry: Some(expiry),
        strike: None,
        option_type: None,
        underlying: Some("NIFTY"),
    }
}

fn option(
    symbol: &'static str,
    lot_size: f64,
    freeze_qty: f64,
    expiry: &'static str,
    strike: f64,
    option_type: &'static str,
) -> Row {
    Row {
        symbol,
        exchange: "NSE",
        kind: "option",
        currency: "INR",
        lot_size,
        tick_size: 0.05,
        freeze_qty,
        expiry: Some(expiry),
        strike: Some(strike),
        option_type: Some(option_type),
        underlying: Some("NIFTY"),
    }
}

/// Writes one `{root}/instruments/{date}.parquet` snapshot with `snapshot_schema`.
fn write_snapshot(root: &Path, date: &str, rows: &[Row]) {
    let dir = root.join("instruments");
    std::fs::create_dir_all(&dir).unwrap();
    let strings = |pick: fn(&Row) -> Option<&'static str>| -> ArrayRef {
        Arc::new(StringArray::from(
            rows.iter().map(pick).collect::<Vec<Option<&str>>>(),
        ))
    };
    let numbers = |pick: fn(&Row) -> Option<f64>| -> ArrayRef {
        Arc::new(Float64Array::from(
            rows.iter().map(pick).collect::<Vec<Option<f64>>>(),
        ))
    };
    let columns: Vec<ArrayRef> = vec![
        strings(|r| Some(r.symbol)),
        strings(|r| Some(r.exchange)),
        strings(|r| Some(r.kind)),
        strings(|r| Some(r.currency)),
        numbers(|r| Some(r.lot_size)),
        numbers(|r| Some(r.tick_size)),
        numbers(|r| Some(r.freeze_qty)),
        strings(|r| r.expiry),
        numbers(|r| r.strike),
        strings(|r| r.option_type),
        strings(|r| r.underlying),
    ];
    let schema = Arc::new(snapshot_schema());
    let batch = RecordBatch::try_new(schema.clone(), columns).unwrap();
    let file = File::create(dir.join(format!("{date}.parquet"))).unwrap();
    let mut writer = ArrowWriter::try_new(file, schema, None).unwrap();
    writer.write(&batch).unwrap();
    writer.close().unwrap();
}

fn id(symbol: &str) -> InstrumentId {
    InstrumentId::new(symbol, Exchange::new("NSE"))
}

fn date(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

#[tokio::test]
async fn resolve_as_of_old_symbol() {
    let dir = TempDir::new("resolve_old");
    write_snapshot(
        &dir.0,
        "2024-01-01",
        &[
            equity("RELIANCE", 1.0, 1000.0),
            future("NIFTY24OCTFUT", 25.0, 500.0, "2024-10-31"),
            option(
                "NIFTY24OCT24500CE",
                25.0,
                500.0,
                "2024-10-31",
                24500.0,
                "CE",
            ),
        ],
    );
    write_snapshot(
        &dir.0,
        "2024-06-01",
        &[
            equity("RELIANCE", 2.0, 2000.0),
            future("NIFTY24DECFUT", 75.0, 1500.0, "2024-12-26"),
        ],
    );
    let master = CatalogInstrumentMaster::open(&dir.0).unwrap();

    // The same symbol resolves to the lot in force at each date.
    let old = master
        .resolve_as_of(&id("RELIANCE"), date(2024, 3, 1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(old.lot_size(), 1.0);
    let new = master
        .resolve_as_of(&id("RELIANCE"), date(2024, 7, 1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(new.lot_size(), 2.0);

    // An india::fno-style future resolves before expiry, not after.
    let before = master
        .resolve_as_of(&id("NIFTY24OCTFUT"), date(2024, 10, 1))
        .await
        .unwrap();
    assert!(before.is_some(), "contract resolves before expiry");
    let after = master
        .resolve_as_of(&id("NIFTY24OCTFUT"), date(2024, 11, 15))
        .await
        .unwrap();
    assert!(after.is_none(), "expired future must not resolve");

    // An option carries strike, option type and underlying through the catalog.
    let call = master
        .resolve_as_of(&id("NIFTY24OCT24500CE"), date(2024, 10, 1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(call.option_type(), Some(OptionType::Call));
    assert_eq!(call.strike(), Some(24500.0));
    assert_eq!(call.underlying(), Some("NIFTY"));
    assert!(
        master
            .resolve_as_of(&id("NIFTY24OCT24500CE"), date(2024, 11, 15))
            .await
            .unwrap()
            .is_none()
    );

    // The live December contract still resolves after the October one expired.
    let live = master
        .resolve_as_of(&id("NIFTY24DECFUT"), date(2024, 11, 15))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(live.lot_size(), 75.0);

    // snapshot() returns every in-force, non-expired record with its dated metadata.
    let snap = master.snapshot(date(2024, 7, 1)).await.unwrap();
    assert_eq!(snap.len(), 4);
    let reliance = snap
        .iter()
        .find(|d| d.instrument.id() == &id("RELIANCE"))
        .unwrap();
    assert_eq!(reliance.effective_from, date(2024, 6, 1));
    assert_eq!(reliance.effective_to, None);
    assert_eq!(reliance.freeze_qty, 2000.0);
    assert_eq!(reliance.instrument.lot_size(), 2.0);

    // After the October expiry only the live instruments remain.
    let later = master.snapshot(date(2024, 11, 15)).await.unwrap();
    assert_eq!(later.len(), 2);
    assert!(later.iter().all(|d| {
        d.instrument
            .expiry()
            .map_or(true, |e| e >= date(2024, 11, 15))
    }));
}
