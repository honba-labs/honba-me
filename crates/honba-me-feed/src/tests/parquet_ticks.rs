use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow::array::{ArrayRef, Float64Array, Int64Array, StringArray};
use arrow::record_batch::RecordBatch;
use honba_messages::{
    AggressorSide, Exchange, InstrumentId, QuoteTick, TradeId, TradeTick, UnixNanos,
};
use honba_ports::{PortError, TickReader, TickRequest};
use parquet::arrow::ArrowWriter;

use crate::parquet_ticks::{ParquetTickStore, TICK_DIR, tick_schema, write_tick_file};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("honba_parquet_ticks_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(TICK_DIR)).unwrap();
        Self(dir)
    }

    fn root(&self) -> &Path {
        &self.0
    }

    fn store_path(&self, name: &str) -> PathBuf {
        self.0.join(TICK_DIR).join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn id(s: &str) -> InstrumentId {
    InstrumentId::new(s, Exchange::new("NSE"))
}

fn ts(n: u64) -> UnixNanos {
    UnixNanos::from_u64(n)
}

fn q(s: &str, bid: f64, t: u64) -> QuoteTick {
    QuoteTick::new(id(s), bid, bid + 1.0, 10.0, 10.0, ts(t), ts(t))
}

fn tr(s: &str, px: f64, t: u64) -> TradeTick {
    TradeTick::new(
        id(s),
        px,
        2.0,
        AggressorSide::Seller,
        TradeId::new(format!("T{t}")),
        ts(t),
        ts(t),
    )
}

fn all() -> TickRequest {
    TickRequest::new(id("TCS"), None, None).unwrap()
}

#[tokio::test]
async fn a_written_store_reads_back_the_same_ticks() {
    let dir = TempDir::new("roundtrip");
    let quotes = vec![q("TCS", 3.0, 30), q("TCS", 1.0, 10)];
    let trades = vec![tr("TCS", 5.0, 20), tr("TCS", 6.0, 40)];
    write_tick_file(&dir.store_path("ticks.parquet"), &quotes, &trades).unwrap();

    let store = ParquetTickStore::open(dir.root()).unwrap();
    assert_eq!(store.len(), (2, 2));

    let got = store.read_quotes(&all()).await.unwrap();
    let times: Vec<u64> = got.iter().map(|x| x.ts_event().as_u64()).collect();
    assert_eq!(times, [10, 30], "sorted by ts_event");
    assert_eq!(got[0].bid_price(), 1.0);
    assert_eq!(got[1].bid_price(), 3.0);

    let got = store.read_trades(&all()).await.unwrap();
    assert_eq!(got[0].price(), 5.0);
    assert_eq!(got[0].aggressor_side(), AggressorSide::Seller);
    assert_eq!(got[0].trade_id().as_str(), "T20");
    assert_eq!(got[1].price(), 6.0);
}

#[tokio::test]
async fn the_range_is_half_open() {
    let dir = TempDir::new("range");
    write_tick_file(
        &dir.store_path("ticks.parquet"),
        &[q("TCS", 1.0, 10), q("TCS", 2.0, 20), q("TCS", 3.0, 30)],
        &[tr("TCS", 9.0, 10), tr("TCS", 8.0, 20), tr("TCS", 7.0, 30)],
    )
    .unwrap();
    let store = ParquetTickStore::open(dir.root()).unwrap();
    let req = TickRequest::new(id("TCS"), Some(ts(10)), Some(ts(30))).unwrap();
    let quotes = store.read_quotes(&req).await.unwrap();
    let trades = store.read_trades(&req).await.unwrap();
    assert_eq!(
        quotes.iter().map(|x| x.bid_price()).collect::<Vec<_>>(),
        [1.0, 2.0]
    );
    assert_eq!(
        trades.iter().map(|x| x.price()).collect::<Vec<_>>(),
        [9.0, 8.0]
    );
}

#[tokio::test]
async fn an_instrument_with_no_ticks_is_empty_not_an_error() {
    let dir = TempDir::new("other-instrument");
    write_tick_file(&dir.store_path("ticks.parquet"), &[q("TCS", 1.0, 10)], &[]).unwrap();
    let store = ParquetTickStore::open(dir.root()).unwrap();
    let req = TickRequest::new(id("WIPRO"), None, None).unwrap();
    assert!(store.read_quotes(&req).await.unwrap().is_empty());
}

#[tokio::test]
async fn an_empty_store_is_unsupported() {
    let dir = TempDir::new("empty");
    let store = ParquetTickStore::open(dir.root()).unwrap();
    assert!(store.is_empty());
    let err = store.read_quotes(&all()).await.unwrap_err();
    assert!(matches!(err, PortError::Unsupported(_)), "{err:?}");
}

#[tokio::test]
async fn several_files_are_merged_in_name_order_and_ties_stay_stable() {
    let dir = TempDir::new("merge");
    // Same ts_event in both files: file-name order (a before b) decides the tie.
    write_tick_file(&dir.store_path("a.parquet"), &[q("TCS", 1.0, 10)], &[]).unwrap();
    write_tick_file(&dir.store_path("b.parquet"), &[q("TCS", 2.0, 10)], &[]).unwrap();
    let store = ParquetTickStore::open(dir.root()).unwrap();
    let got = store.read_quotes(&all()).await.unwrap();
    assert_eq!(
        got.iter().map(|x| x.bid_price()).collect::<Vec<_>>(),
        [1.0, 2.0]
    );
}

/// Writes a tick file from raw columns, so a test can lay out a bad row.
fn write_raw(path: &Path, rows: usize, columns: Vec<ArrayRef>) {
    let schema = Arc::new(tick_schema());
    let batch = RecordBatch::try_new(schema.clone(), columns).unwrap();
    let mut writer =
        ArrowWriter::try_new(std::fs::File::create(path).unwrap(), schema, None).unwrap();
    writer.write(&batch).unwrap();
    writer.close().unwrap();
    assert_eq!(batch.num_rows(), rows);
}

fn strings(values: Vec<Option<&str>>) -> ArrayRef {
    Arc::new(StringArray::from(values))
}

fn floats(values: Vec<Option<f64>>) -> ArrayRef {
    Arc::new(Float64Array::from(values))
}

fn ints(values: Vec<Option<i64>>) -> ArrayRef {
    Arc::new(Int64Array::from(values))
}

#[test]
fn a_row_of_an_unknown_kind_names_its_file_and_row() {
    let dir = TempDir::new("bad-kind");
    write_raw(
        &dir.store_path("x.parquet"),
        1,
        vec![
            strings(vec![Some("depth")]),
            strings(vec![Some("TCS")]),
            strings(vec![Some("NSE")]),
            ints(vec![Some(10)]),
            ints(vec![Some(10)]),
            floats(vec![Some(1.0)]),
            floats(vec![Some(2.0)]),
            floats(vec![Some(1.0)]),
            floats(vec![Some(1.0)]),
            floats(vec![None]),
            floats(vec![None]),
            strings(vec![None]),
            strings(vec![None]),
        ],
    );
    let err = ParquetTickStore::open(dir.root()).unwrap_err().to_string();
    assert!(err.contains("x.parquet") && err.contains("row 0"), "{err}");
    assert!(err.contains("unknown kind"), "{err}");
}

#[test]
fn a_quote_missing_a_required_column_is_a_bad_row() {
    let dir = TempDir::new("bad-quote");
    write_raw(
        &dir.store_path("x.parquet"),
        1,
        vec![
            strings(vec![Some("quote")]),
            strings(vec![Some("TCS")]),
            strings(vec![Some("NSE")]),
            ints(vec![Some(10)]),
            ints(vec![Some(10)]),
            floats(vec![None]), // bid_price
            floats(vec![Some(2.0)]),
            floats(vec![Some(1.0)]),
            floats(vec![Some(1.0)]),
            floats(vec![None]),
            floats(vec![None]),
            strings(vec![None]),
            strings(vec![None]),
        ],
    );
    let err = ParquetTickStore::open(dir.root()).unwrap_err().to_string();
    assert!(err.contains("bid_price") && err.contains("row 0"), "{err}");
}

#[test]
fn a_crossed_quote_is_refused_by_the_wire_invariant() {
    let dir = TempDir::new("crossed");
    write_raw(
        &dir.store_path("x.parquet"),
        1,
        vec![
            strings(vec![Some("quote")]),
            strings(vec![Some("TCS")]),
            strings(vec![Some("NSE")]),
            ints(vec![Some(10)]),
            ints(vec![Some(10)]),
            floats(vec![Some(3.0)]), // bid
            floats(vec![Some(2.0)]), // ask, below the bid
            floats(vec![Some(1.0)]),
            floats(vec![Some(1.0)]),
            floats(vec![None]),
            floats(vec![None]),
            strings(vec![None]),
            strings(vec![None]),
        ],
    );
    let err = ParquetTickStore::open(dir.root()).unwrap_err().to_string();
    assert!(err.contains("invalid quote"), "{err}");
}

#[test]
fn a_missing_column_is_named() {
    let dir = TempDir::new("missing-column");
    let schema = Arc::new(arrow::datatypes::Schema::new(vec![
        arrow::datatypes::Field::new("kind", arrow::datatypes::DataType::Utf8, false),
    ]));
    let batch = RecordBatch::try_new(schema.clone(), vec![strings(vec![Some("quote")])]).unwrap();
    let mut writer = ArrowWriter::try_new(
        std::fs::File::create(dir.store_path("x.parquet")).unwrap(),
        schema,
        None,
    )
    .unwrap();
    writer.write(&batch).unwrap();
    writer.close().unwrap();
    let err = ParquetTickStore::open(dir.root()).unwrap_err().to_string();
    assert!(err.contains("symbol"), "{err}");
}

#[test]
fn a_missing_store_directory_is_an_error() {
    let dir = TempDir::new("no-dir");
    let missing = dir.root().join("absent");
    assert!(ParquetTickStore::open(&missing).is_err());
}
