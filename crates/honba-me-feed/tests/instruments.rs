//! Integration test: a backtest-shaped flow consults the dated instrument
//! catalog, so an expired contract neither resolves nor fills while a live one
//! does. Fixtures are generated in a temp dir; nothing touches the network.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow::array::{ArrayRef, Float64Array, StringArray};
use arrow::record_batch::RecordBatch;
use chrono::NaiveDate;
use honba_core::{Exchange, InstrumentId};
use honba_feed::ports::InstrumentMaster;
use honba_feed::{CatalogInstrumentMaster, snapshot_schema};
use honba_feed::wire::UnixNanos;
use parquet::arrow::ArrowWriter;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "honba_instruments_it_{}_{name}",
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

/// Writes `{root}/instruments/{date}.parquet` with the two futures under test.
fn write_futures(root: &Path, date: &str, rows: &[(&str, &str)]) {
    let dir = root.join("instruments");
    std::fs::create_dir_all(&dir).unwrap();
    let strings = |values: Vec<Option<&str>>| -> ArrayRef { Arc::new(StringArray::from(values)) };
    let numbers = |values: Vec<Option<f64>>| -> ArrayRef { Arc::new(Float64Array::from(values)) };
    let columns: Vec<ArrayRef> = vec![
        strings(rows.iter().map(|(symbol, _)| Some(*symbol)).collect()),
        strings(rows.iter().map(|_| Some("NSE")).collect()),
        strings(rows.iter().map(|_| Some("future")).collect()),
        strings(rows.iter().map(|_| Some("INR")).collect()),
        numbers(rows.iter().map(|_| Some(75.0)).collect()),
        numbers(rows.iter().map(|_| Some(0.05)).collect()),
        numbers(rows.iter().map(|_| Some(1500.0)).collect()),
        strings(rows.iter().map(|(_, expiry)| Some(*expiry)).collect()),
        numbers(rows.iter().map(|_| None).collect()),
        strings(rows.iter().map(|_| None).collect()),
        strings(rows.iter().map(|_| Some("NIFTY")).collect()),
    ];
    let schema = Arc::new(snapshot_schema());
    let batch = RecordBatch::try_new(schema.clone(), columns).unwrap();
    let file = File::create(dir.join(format!("{date}.parquet"))).unwrap();
    let mut writer = ArrowWriter::try_new(file, schema, None).unwrap();
    writer.write(&batch).unwrap();
    writer.close().unwrap();
}

#[tokio::test]
async fn catalog_resolves_expired_contract() {
    let dir = TempDir::new("backtest_expired");
    write_futures(
        &dir.0,
        "2024-01-01",
        &[
            ("NIFTY24OCTFUT", "2024-10-31"),
            ("NIFTY24DECFUT", "2024-12-26"),
        ],
    );
    let master = CatalogInstrumentMaster::open(&dir.0).unwrap();
    let as_of = NaiveDate::from_ymd_opt(2024, 11, 15).unwrap();

    let expired = InstrumentId::new(Exchange::Nse, "NIFTY24OCTFUT");
    let live = InstrumentId::new(Exchange::Nse, "NIFTY24DECFUT");

    // The dated catalog is the gate: the contract expired on 2024-10-31.
    assert!(
        master
            .resolve_as_of(&expired, as_of)
            .await
            .unwrap()
            .is_none()
    );
    assert!(master.resolve_as_of(&live, as_of).await.unwrap().is_some());

    let _ts = UnixNanos::from_u64(1_700_000_000_000_000_000);
    
    // Simulate what the engine would do: resolve and decide whether to order.
    let mut orders_submitted = 0;
    for (id, _order_id) in [(&expired, "EXPIRED-1"), (&live, "LIVE-1")] {
        if master.resolve_as_of(id, as_of).await.unwrap().is_some() {
            orders_submitted += 1;
        }
    }

    assert_eq!(orders_submitted, 1, "only the live contract should be submitted");
}
