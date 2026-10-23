//! Integration tests: `DatasetReader` over a Parquet data directory, read through
//! the `InstrumentMaster` and `BarReader` ports. Fixtures are generated in a temp
//! dir; nothing touches the network.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow::array::{ArrayRef, Float64Array, Int64Array};
use arrow::record_batch::RecordBatch;
use honba_core::{Exchange, InstrumentId};
use honba_feed::import::parquet_source::bar_schema;
use honba_feed::ports::{
    BarReader, BarRequest, DepthReader, InstrumentMaster, PortError, QuoteReader,
};
use honba_feed::wire::{BarAggregation, BarSpecification, PriceType, UnixNanos};
use honba_feed::{DatasetReader, ReaderError};
use parquet::arrow::ArrowWriter;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("honba_reader_{}_{name}", std::process::id()));
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

fn write_bars(path: &Path, stamps: &[i64]) {
    let schema = Arc::new(bar_schema());
    let col = |delta: f64| -> ArrayRef {
        Arc::new(Float64Array::from(
            stamps
                .iter()
                .enumerate()
                .map(|(i, _)| 100.0 + i as f64 + delta)
                .collect::<Vec<f64>>(),
        ))
    };
    let columns: Vec<ArrayRef> = vec![
        Arc::new(Int64Array::from(stamps.to_vec())),
        col(-1.0),
        col(1.0),
        col(-2.0),
        col(0.0),
        col(900.0),
    ];
    let batch = RecordBatch::try_new(schema.clone(), columns).unwrap();
    let mut writer = ArrowWriter::try_new(File::create(path).unwrap(), schema, None).unwrap();
    writer.write(&batch).unwrap();
    writer.close().unwrap();
}

fn fixture(name: &str) -> TempDir {
    let dir = TempDir::new(name);
    write_bars(&dir.0.join("TCS.NSE.parquet"), &[10, 20, 30]);
    write_bars(&dir.0.join("INFY.NSE.parquet"), &[10, 20]);
    std::fs::write(dir.0.join("README.txt"), "ignored").unwrap();
    dir
}

#[tokio::test]
async fn a_data_directory_lists_its_instruments_in_id_order() {
    let dir = fixture("list");
    let reader = DatasetReader::from_parquet_dir(&dir.0).unwrap();
    let ids: Vec<String> = reader
        .list_instruments()
        .await
        .unwrap()
        .iter()
        .map(|i| i.id().to_string())
        .collect();
    // honba_core::InstrumentId displays as "EXCHANGE:SYMBOL" (not "SYMBOL.EXCHANGE",
    // the legacy format); same exchange here, so symbol order is still what's checked.
    assert_eq!(ids, vec!["NSE:INFY", "NSE:TCS"]);
}

#[tokio::test]
async fn bars_are_read_back_from_parquet_with_the_requested_range() {
    let dir = fixture("bars");
    let reader = DatasetReader::from_parquet_dir(&dir.0).unwrap();
    let spec = BarSpecification::new(1, BarAggregation::Minute, PriceType::Last);
    let id = InstrumentId::new(Exchange::Nse, "TCS");
    let req = BarRequest::new(id, spec, Some(UnixNanos::from_u64(20)), None).unwrap();
    let bars = reader.read_bars(&req).await.unwrap();
    let stamps: Vec<u64> = bars.iter().map(|b| b.ts_event().as_u64()).collect();
    assert_eq!(stamps, vec![20, 30]);
    assert_eq!(bars[0].close(), 101.0);
    assert_eq!(bars[0].volume(), 1001.0);
}

#[test]
fn an_empty_directory_is_an_empty_reader() {
    let dir = TempDir::new("empty");
    assert!(DatasetReader::from_parquet_dir(&dir.0).unwrap().is_empty());
}

#[test]
fn a_missing_directory_is_an_io_error() {
    let err = DatasetReader::from_parquet_dir(Path::new("/nonexistent/honba")).unwrap_err();
    assert!(matches!(err, ReaderError::Io(_)), "{err:?}");
}

#[test]
fn a_parquet_file_without_an_exchange_suffix_is_rejected() {
    let dir = TempDir::new("badname");
    write_bars(&dir.0.join("TCS.parquet"), &[1]);
    let err = DatasetReader::from_parquet_dir(&dir.0).unwrap_err();
    assert!(matches!(err, ReaderError::FileName(_)), "{err:?}");
}

#[test]
fn a_corrupt_file_is_a_dataset_error() {
    let dir = TempDir::new("corrupt");
    std::fs::write(dir.0.join("TCS.NSE.parquet"), b"not parquet").unwrap();
    let err = DatasetReader::from_parquet_dir(&dir.0).unwrap_err();
    assert!(matches!(err, ReaderError::Dataset(_)), "{err:?}");
}

#[tokio::test]
async fn quotes_and_depth_are_served_from_a_parquet_directory() {
    let dir = fixture("quotes");
    let reader = DatasetReader::from_parquet_dir(&dir.0).unwrap();
    let id = InstrumentId::new(Exchange::Nse, "TCS");
    let quote = reader.read_quote(&id, None).await.unwrap().unwrap();
    assert_eq!(quote.bid_price(), 102.0);
    assert_eq!(quote.ts_event(), UnixNanos::from_u64(30));
    let early = reader
        .read_quote(&id, Some(UnixNanos::from_u64(10)))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(early.bid_price(), 100.0);
    assert!(matches!(
        reader.read_depth(&id, 5).await,
        Err(PortError::Unsupported(_))
    ));
}
