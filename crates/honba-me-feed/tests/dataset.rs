//! Integration tests for the shared dataset types and the streaming Parquet read path.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use arrow::array::{ArrayRef, Float64Array, Int64Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;

use honba_engine::{AuditKind, DataFeed, Engine, EngineOutput, Handler, Result};
use honba_feed::import::parquet_source::{ParquetBarSource, ParquetError, bar_schema};
use honba_feed::{Dataset, DatasetBuildError, DatasetFeed};
use honba_messages::{
    BarAggregation, BarSpecification, BarType, Event, Exchange, InstrumentId, PriceType, UnixNanos,
};

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
    InstrumentId::new(symbol, Exchange::new("NSE"))
}

fn default_spec() -> BarSpecification {
    BarSpecification::new(1, BarAggregation::Minute, PriceType::Last)
}

#[derive(Clone)]
struct Recorder {
    seen: Arc<Mutex<Vec<(String, u64, u64)>>>,
}

impl Recorder {
    fn new() -> Self {
        Self {
            seen: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn recorded(&self) -> Vec<(String, u64, u64)> {
        self.seen.lock().expect("lock the recorder").clone()
    }
}

impl Handler for Recorder {
    fn on_event(&mut self, event: &Event, ts_init: UnixNanos) -> Result<EngineOutput> {
        let Event::Bar(bar) = event else {
            return Ok(EngineOutput::None);
        };
        self.seen.lock().expect("lock the recorder").push((
            bar.bar_type().instrument_id().symbol().to_string(),
            bar.ts_event().as_u64(),
            ts_init.as_u64(),
        ));
        Ok(EngineOutput::None)
    }
}

fn replayed_bars(feed: &mut DatasetFeed) -> Vec<(String, u64)> {
    let mut replayed = Vec::new();
    while let Some(message) = feed.next().expect("the feed does not fail") {
        assert_eq!(
            message.ts_init(),
            UnixNanos::from_u64(1_700_000_000_000_000_000)
        );
        let Event::Bar(bar) = message.into_event() else {
            panic!("the feed replays bars");
        };
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

    let recorder = Recorder::new();
    let mut engine = Engine::new();
    engine.add_handler(recorder.clone());
    let mut feed = DatasetFeed::new(&dataset, ts_init);
    engine.run(&mut feed).expect("run the engine");

    let dispatched: Vec<u64> = engine
        .audit()
        .iter()
        .filter_map(|record| match record.kind {
            AuditKind::EventDispatched { ts_event } => Some(ts_event),
            _ => None,
        })
        .collect();
    assert_eq!(dispatched, vec![10, 20, 30, 40, 50]);
    assert_eq!(dispatched.len(), dataset.bar_count());
    assert_eq!(recorder.recorded().len(), dataset.bar_count());
    assert_eq!(engine.now(), UnixNanos::from_u64(50));
}

#[test]
fn the_engine_hands_every_dataset_bar_to_the_handler_once() {
    let temp = TempFileGuard::new("engine_dispatch.parquet");
    write_bars(temp.path(), &[10, 20], &[5.0, 6.0]);
    let dataset = Dataset::from_parquet(&[(instrument("TCS"), temp.path().to_path_buf())])
        .expect("build the dataset");

    let recorder = Recorder::new();
    let mut engine = Engine::new();
    engine.add_handler(recorder.clone());
    let mut feed = DatasetFeed::new(&dataset, UnixNanos::from_u64(1));
    engine.run(&mut feed).expect("run the engine");

    assert_eq!(
        recorder.recorded(),
        vec![("TCS".to_string(), 10, 1), ("TCS".to_string(), 20, 1)]
    );
    assert_eq!(recorder.recorded().len(), dataset.bar_count());
}

#[test]
fn null_timestamps_are_skipped_and_unknown_columns_ignored() {
    let temp = TempFileGuard::new("nulls_and_extra.parquet");
    let schema = Schema::new(vec![
        Field::new("ts", DataType::Int64, true),
        Field::new("open", DataType::Float64, false),
        Field::new("high", DataType::Float64, false),
        Field::new("low", DataType::Float64, false),
        Field::new("close", DataType::Float64, false),
        Field::new("volume", DataType::Float64, false),
        Field::new("vwap", DataType::Float64, false),
        Field::new("label", DataType::Utf8, false),
    ]);
    let columns: Vec<ArrayRef> = vec![
        Arc::new(Int64Array::from(vec![Some(10), None, Some(30)])),
        Arc::new(Float64Array::from(vec![1.0, 2.0, 3.0])),
        Arc::new(Float64Array::from(vec![2.0, 3.0, 4.0])),
        Arc::new(Float64Array::from(vec![0.0, 1.0, 2.0])),
        Arc::new(Float64Array::from(vec![1.5, 2.5, 3.5])),
        Arc::new(Float64Array::from(vec![10.0, 20.0, 30.0])),
        Arc::new(Float64Array::from(vec![1.4, 2.4, 3.4])),
        Arc::new(StringArray::from(vec!["a", "b", "c"])),
    ];
    write_batch(temp.path(), schema, columns);

    let source = ParquetBarSource::new(temp.path(), instrument("SBIN"));

    let slice = source.columnar().expect("read the columnar slice");
    assert_eq!(slice.len(), 2);
    assert_eq!(
        slice.timestamps(),
        &[UnixNanos::from_u64(10), UnixNanos::from_u64(30)]
    );
    assert_eq!(slice.closes(), &[1.5, 3.5]);
    assert_eq!(slice.volumes(), &[10.0, 30.0]);

    let bars = source.bars().expect("read the bars");
    assert_eq!(bars.len(), 2);
    assert_eq!(bars[0].close(), 1.5);
    assert_eq!(bars[1].close(), 3.5);
    assert_eq!(bars[1].ts_event(), UnixNanos::from_u64(30));
}

#[test]
fn wrong_column_type_is_a_typed_error() {
    let temp = TempFileGuard::new("string_close.parquet");
    let schema = Schema::new(vec![
        Field::new("ts", DataType::Int64, false),
        Field::new("open", DataType::Float64, false),
        Field::new("high", DataType::Float64, false),
        Field::new("low", DataType::Float64, false),
        Field::new("close", DataType::Utf8, false),
        Field::new("volume", DataType::Float64, false),
    ]);
    let columns: Vec<ArrayRef> = vec![
        Arc::new(Int64Array::from(vec![10])),
        Arc::new(Float64Array::from(vec![1.0])),
        Arc::new(Float64Array::from(vec![2.0])),
        Arc::new(Float64Array::from(vec![0.0])),
        Arc::new(StringArray::from(vec!["1.5"])),
        Arc::new(Float64Array::from(vec![10.0])),
    ];
    write_batch(temp.path(), schema, columns);

    let source = ParquetBarSource::new(temp.path(), instrument("SBIN"));

    match source.columnar().expect_err("a string close is rejected") {
        ParquetError::WrongType { name, got, want } => {
            assert_eq!(name, "close");
            assert_eq!(got, DataType::Utf8);
            assert_eq!(want, DataType::Float64);
        }
        other => panic!("expected WrongType for close, got {other:?}"),
    }

    match source.bars().expect_err("a string close is rejected") {
        ParquetError::WrongType { name, .. } => assert_eq!(name, "close"),
        other => panic!("expected WrongType for close, got {other:?}"),
    }

    let err = Dataset::from_parquet(&[(instrument("SBIN"), temp.path().to_path_buf())])
        .expect_err("the dataset cannot be built");
    match err {
        DatasetBuildError::Parquet { message, source } => {
            assert!(message.contains("close"), "unexpected message: {message}");
            assert!(matches!(
                source,
                ParquetError::WrongType { name: "close", .. }
            ));
        }
        other => panic!("expected a Parquet error, got {other:?}"),
    }
}

#[test]
fn missing_column_is_a_typed_error() {
    let temp = TempFileGuard::new("missing_volume.parquet");
    let schema = Schema::new(vec![
        Field::new("ts", DataType::Int64, false),
        Field::new("open", DataType::Float64, false),
        Field::new("high", DataType::Float64, false),
        Field::new("low", DataType::Float64, false),
        Field::new("close", DataType::Float64, false),
    ]);
    let columns: Vec<ArrayRef> = vec![
        Arc::new(Int64Array::from(vec![10])),
        Arc::new(Float64Array::from(vec![1.0])),
        Arc::new(Float64Array::from(vec![2.0])),
        Arc::new(Float64Array::from(vec![0.0])),
        Arc::new(Float64Array::from(vec![1.5])),
    ];
    write_batch(temp.path(), schema, columns);

    let source = ParquetBarSource::new(temp.path(), instrument("SBIN"));

    match source.columnar().expect_err("a missing volume is rejected") {
        ParquetError::MissingColumn(column) => assert_eq!(column, "volume"),
        other => panic!("expected MissingColumn(\"volume\"), got {other:?}"),
    }
}

#[test]
fn a_non_monotonic_file_is_a_build_error() {
    let temp = TempFileGuard::new("non_monotonic.parquet");
    write_bars(temp.path(), &[10, 20, 5], &[1.0, 2.0, 3.0]);

    let source = ParquetBarSource::new(temp.path(), instrument("SBIN"));

    match source.columnar().expect_err("a regression is rejected") {
        ParquetError::InvalidBarData(error) => assert_eq!(
            *error,
            DatasetBuildError::NonMonotonicTimestamp {
                index: 2,
                previous: 20,
                got: 5,
            }
        ),
        other => panic!("expected InvalidBarData, got {other:?}"),
    }

    match source.bars().expect_err("a regression is rejected") {
        ParquetError::InvalidBarData(error) => assert_eq!(
            *error,
            DatasetBuildError::NonMonotonicTimestamp {
                index: 2,
                previous: 20,
                got: 5,
            }
        ),
        other => panic!("expected InvalidBarData, got {other:?}"),
    }

    let err = Dataset::from_parquet(&[(instrument("SBIN"), temp.path().to_path_buf())])
        .expect_err("the dataset cannot be built");
    assert_eq!(
        err,
        DatasetBuildError::NonMonotonicTimestamp {
            index: 2,
            previous: 20,
            got: 5,
        }
    );
}

#[test]
fn a_non_finite_file_is_a_build_error() {
    let temp = TempFileGuard::new("nan_close.parquet");
    let columns: Vec<ArrayRef> = vec![
        Arc::new(Int64Array::from(vec![10])),
        Arc::new(Float64Array::from(vec![1.0])),
        Arc::new(Float64Array::from(vec![2.0])),
        Arc::new(Float64Array::from(vec![0.0])),
        Arc::new(Float64Array::from(vec![f64::NAN])),
        Arc::new(Float64Array::from(vec![10.0])),
    ];
    write_batch(temp.path(), bar_schema(), columns);

    let source = ParquetBarSource::new(temp.path(), instrument("SBIN"));

    match source.columnar().expect_err("a NaN close is rejected") {
        ParquetError::InvalidBarData(error) => assert_eq!(
            *error,
            DatasetBuildError::NonFiniteValue {
                index: 0,
                column: "close"
            }
        ),
        other => panic!("expected InvalidBarData, got {other:?}"),
    }
}

#[test]
fn a_missing_file_is_a_dataset_build_error() {
    let err = Dataset::from_parquet(&[(
        instrument("SBIN"),
        PathBuf::from("/tmp/honba_no_such_dataset_file_98765.parquet"),
    )])
    .expect_err("the file does not exist");

    match err {
        DatasetBuildError::Parquet { message, source } => {
            assert!(message.contains("io error"), "{message}");
            match source {
                ParquetError::Io(error) => {
                    assert_eq!(error.kind(), std::io::ErrorKind::NotFound)
                }
                other => panic!("expected an Io error, got {other:?}"),
            }
        }
        other => panic!("expected a Parquet error, got {other:?}"),
    }
}

#[test]
fn a_duplicate_instrument_in_two_files_is_rejected() {
    let first = TempFileGuard::new("duplicate_a.parquet");
    write_bars(first.path(), &[10], &[1.0]);
    let second = TempFileGuard::new("duplicate_b.parquet");
    write_bars(second.path(), &[20], &[2.0]);

    let err = Dataset::from_parquet(&[
        (instrument("TCS"), first.path().to_path_buf()),
        (instrument("TCS"), second.path().to_path_buf()),
    ])
    .expect_err("the instrument appears twice");

    assert_eq!(
        err,
        DatasetBuildError::DuplicateInstrument(instrument("TCS"))
    );
}

#[test]
fn a_source_can_be_read_repeatedly() {
    let temp = TempFileGuard::new("reusable_reader.parquet");
    write_bars(temp.path(), &[10, 20], &[1.0, 2.0]);

    let source = ParquetBarSource::new(temp.path(), instrument("TCS"));

    assert_eq!(source.columnar().expect("read the slice").len(), 2);
    assert_eq!(source.bars().expect("read the bars").len(), 2);
    let rows = source
        .batches()
        .expect("stream the batches")
        .map(|batch| batch.expect("read a batch").num_rows())
        .sum::<usize>();
    assert_eq!(rows, 2);
}
