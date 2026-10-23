//! Integration tests for the shared dataset types and the streaming Parquet read path.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow::array::{ArrayRef, Float64Array, Int64Array};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;

use honba_core::{Exchange, InstrumentId};
use honba_feed::import::parquet_source::{ParquetBarSource, bar_schema};
use honba_feed::{Dataset, DatasetFeed};
use honba_feed::wire::{BarAggregation, BarSpecification, PriceType, UnixNanos};
use honba_feed::wire::BarType;

struct TempFileGuard {
    path: PathBuf,
}

impl TempFileGuard {
    fn new(name: &str) -> Self {
        let mut path = std::env::temp_dir();
        let unique_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        path.push(format!("honba_test_dataset_{unique_id}_{name}"));
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn write_batch(path: &Path, schema: Schema, columns: Vec<ArrayRef>) {
    let file = File::create(path).expect("create temp file");
    let schema = Arc::new(schema);
    let batch = RecordBatch::try_new(schema.clone(), columns).expect("create batch");
    let mut writer = ArrowWriter::try_new(file, schema, None).expect("arrow writer");
    writer.write(&batch).expect("write batch");
    writer.close().expect("close writer");
}

fn bar_columns(timestamps: &[u64], closes: &[f64]) -> Vec<ArrayRef> {
    let shifted = |delta: f64| -> Vec<f64> { closes.iter().map(|close| close + delta).collect() };
    vec![
        Arc::new(Int64Array::from(
            timestamps.iter().map(|ts| *ts as i64).collect::<Vec<i64>>(),
        )),
        Arc::new(Float64Array::from(shifted(-1.0))),
        Arc::new(Float64Array::from(shifted(1.0))),
        Arc::new(Float64Array::from(shifted(-2.0))),
        Arc::new(Float64Array::from(closes.to_vec())),
        Arc::new(Float64Array::from(
            closes
                .iter()
                .map(|close| close * 10.0)
                .collect::<Vec<f64>>(),
        )),
    ]
}

fn write_bars(path: &Path, timestamps: &[u64], closes: &[f64]) {
    write_batch(path, bar_schema(), bar_columns(timestamps, closes));
}

fn instrument(symbol: &str) -> InstrumentId {
    InstrumentId::new(Exchange::Nse, symbol)
}

fn default_spec() -> BarSpecification {
    BarSpecification::new(1, BarAggregation::Minute, PriceType::Last)
}

fn replayed_bars(feed: &mut DatasetFeed) -> Vec<(String, u64)> {
    let mut replayed = Vec::new();
    while let Some(message) = feed.next().expect("the feed does not fail") {
        let bar = message.bar().expect("the feed replays bars");
        replayed.push((
            bar.bar_type().instrument_id().symbol().to_string(),
            bar.ts_event().as_u64(),
        ));
    }
    replayed
}

#[test]
fn parquet_round_trips_through_a_columnar_slice() {
    let temp = TempFileGuard::new("columnar_round_trip.parquet");
    let timestamps = [
        1_700_000_000_000_000_000u64,
        1_700_000_060_000_000_000,
        1_700_000_120_000_000_000,
    ];
    let closes = [101.0, 102.5, 103.5];
    write_bars(temp.path(), &timestamps, &closes);

    let id = instrument("TCS");
    let spec = BarSpecification::new(5, BarAggregation::Minute, PriceType::Bid);
    let source = ParquetBarSource::with_spec(temp.path(), id.clone(), spec);

    let slice = source.columnar().expect("read the columnar slice");

    assert_eq!(slice.instrument(), &id);
    assert_eq!(*slice.bar_spec(), spec);
    assert_eq!(slice.len(), 3);
    assert_eq!(
        slice.timestamps(),
        &[
            UnixNanos::from_u64(timestamps[0]),
            UnixNanos::from_u64(timestamps[1]),
            UnixNanos::from_u64(timestamps[2]),
        ]
    );
    assert_eq!(slice.opens(), &[100.0, 101.5, 102.5]);
    assert_eq!(slice.highs(), &[102.0, 103.5, 104.5]);
    assert_eq!(slice.lows(), &[99.0, 100.5, 101.5]);
    assert_eq!(slice.closes(), &closes);
    assert_eq!(slice.volumes(), &[1010.0, 1025.0, 1035.0]);

    let last = slice.bar(2).expect("the third bar exists");
    assert_eq!(last.bar_type(), &BarType::new(id, spec));
    assert_eq!(last.ts_event(), UnixNanos::from_u64(timestamps[2]));
    assert_eq!(last.ts_init(), UnixNanos::from_u64(timestamps[2]));
    assert_eq!(last.close(), 103.5);
    assert!(slice.bar(3).is_none());
}

#[test]
fn batches_stream_the_rows_without_building_bars() {
    let temp = TempFileGuard::new("streaming_batches.parquet");
    write_bars(temp.path(), &[10, 20, 30, 40], &[1.0, 2.0, 3.0, 4.0]);

    let source = ParquetBarSource::new(temp.path(), instrument("TCS"));

    let mut rows = 0;
    let mut batches = 0;
    for batch in source.batches().expect("stream the batches") {
        let batch = batch.expect("read a batch");
        assert_eq!(batch.schema(), Arc::new(bar_schema()));
        rows += batch.num_rows();
        batches += 1;
    }
    assert!(batches >= 1);
    assert_eq!(rows, 4);
}

#[test]
fn bars_and_columnar_agree() {
    let temp = TempFileGuard::new("bars_and_columnar.parquet");
    write_bars(temp.path(), &[10, 20, 30], &[1.0, 2.0, 3.0]);

    let id = instrument("INFY");
    let source = ParquetBarSource::new(temp.path(), id.clone());

    let bars = source.bars().expect("read the bars");
    let slice = source.columnar().expect("read the columnar slice");

    assert_eq!(bars.len(), slice.len());
    for (index, bar) in bars.iter().enumerate() {
        assert_eq!(Some(bar), slice.bar(index).as_ref());
    }
    assert_eq!(
        bars[2].bar_type(),
        &BarType::new(id.clone(), default_spec())
    );
    assert_eq!(bars[2].ts_event(), UnixNanos::from_u64(30));
    assert_eq!(bars[2].ts_init(), UnixNanos::from_u64(30));
    assert_eq!(bars[0].close(), 1.0);
    assert_eq!(bars[1].volume(), 20.0);
}

#[test]
fn a_dataset_built_from_two_files_replays_in_instrument_order() {
    let tcs_file = TempFileGuard::new("dataset_tcs.parquet");
    write_bars(tcs_file.path(), &[10, 20], &[5.0, 6.0]);
    let infy_file = TempFileGuard::new("dataset_infy.parquet");
    write_bars(infy_file.path(), &[30, 40, 50], &[7.0, 8.0, 9.0]);

    let dataset = Dataset::from_parquet(&[
        (instrument("TCS"), tcs_file.path().to_path_buf()),
        (instrument("INFY"), infy_file.path().to_path_buf()),
    ])
    .expect("build the dataset");

    assert_eq!(dataset.len(), 2);
    assert_eq!(dataset.bar_count(), 5);

    let ts_init = UnixNanos::from_u64(1_700_000_000_000_000_000);
    let mut feed = DatasetFeed::new(&dataset, ts_init);
    assert_eq!(
        replayed_bars(&mut feed),
        vec![
            ("INFY".to_string(), 30),
            ("INFY".to_string(), 40),
            ("INFY".to_string(), 50),
            ("TCS".to_string(), 10),
            ("TCS".to_string(), 20),
        ]
    );
}
