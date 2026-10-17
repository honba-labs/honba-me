use std::fs::File;
use std::sync::Arc;

use arrow::array::{Float64Array, Int64Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;

use honba_messages::{BarAggregation, BarSpecification, Exchange, InstrumentId, PriceType};

use crate::import::parquet_source::{bar_schema, ParquetBarSource, ParquetError};

struct TempFileGuard {
    path: std::path::PathBuf,
}

impl TempFileGuard {
    fn new(name: &str) -> Self {
        let mut path = std::env::temp_dir();
        let unique_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        path.push(format!("honba_test_unit_{unique_id}_{name}"));
        Self { path }
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[test]
fn test_parquet_bar_source_read_valid_bars() {
    let temp = TempFileGuard::new("valid_bars.parquet");
    let file = File::create(temp.path()).expect("create temp file");

    let schema = Arc::new(bar_schema());

    let ts = Arc::new(Int64Array::from(vec![
        1_700_000_000_000_000_000,
        1_700_000_060_000_000_000,
    ]));
    let open = Arc::new(Float64Array::from(vec![100.0, 101.0]));
    let high = Arc::new(Float64Array::from(vec![102.0, 103.0]));
    let low = Arc::new(Float64Array::from(vec![99.0, 100.5]));
    let close = Arc::new(Float64Array::from(vec![101.0, 102.5]));
    let volume = Arc::new(Float64Array::from(vec![1000.0, 1500.0]));

    let batch = RecordBatch::try_new(schema.clone(), vec![ts, open, high, low, close, volume])
        .expect("create batch");

    let mut writer = ArrowWriter::try_new(file, schema, None).expect("arrow writer");
    writer.write(&batch).expect("write batch");
    writer.close().expect("close writer");

    let inst = InstrumentId::new("TCS", Exchange::new("NSE"));
    let source = ParquetBarSource::new(temp.path(), inst.clone());
    let bars = source.bars().expect("read bars");

    assert_eq!(bars.len(), 2);
    assert_eq!(*bars[0].bar_type().instrument_id(), inst);
    assert_eq!(bars[0].open(), 100.0);
    assert_eq!(bars[0].high(), 102.0);
    assert_eq!(bars[0].low(), 99.0);
    assert_eq!(bars[0].close(), 101.0);
    assert_eq!(bars[0].volume(), 1000.0);
    assert_eq!(bars[0].ts_event().as_u64(), 1_700_000_000_000_000_000);

    assert_eq!(bars[1].open(), 101.0);
    assert_eq!(bars[1].high(), 103.0);
    assert_eq!(bars[1].low(), 100.5);
    assert_eq!(bars[1].close(), 102.5);
    assert_eq!(bars[1].volume(), 1500.0);
    assert_eq!(bars[1].ts_event().as_u64(), 1_700_000_060_000_000_000);
}

#[test]
fn test_parquet_bar_source_with_custom_spec() {
    let temp = TempFileGuard::new("custom_spec.parquet");
    let file = File::create(temp.path()).expect("create temp file");

    let schema = Arc::new(bar_schema());
    let ts = Arc::new(Int64Array::from(vec![1_700_000_000_000_000_000]));
    let open = Arc::new(Float64Array::from(vec![50.0]));
    let high = Arc::new(Float64Array::from(vec![55.0]));
    let low = Arc::new(Float64Array::from(vec![49.0]));
    let close = Arc::new(Float64Array::from(vec![53.0]));
    let volume = Arc::new(Float64Array::from(vec![500.0]));

    let batch = RecordBatch::try_new(schema.clone(), vec![ts, open, high, low, close, volume])
        .expect("create batch");

    let mut writer = ArrowWriter::try_new(file, schema, None).expect("arrow writer");
    writer.write(&batch).expect("write batch");
    writer.close().expect("close writer");

    let inst = InstrumentId::new("INFY", Exchange::new("NSE"));
    let spec = BarSpecification::new(5, BarAggregation::Minute, PriceType::Bid);
    let source = ParquetBarSource::with_spec(temp.path(), inst, spec);
    let bars = source.bars().expect("read bars");

    assert_eq!(bars.len(), 1);
    assert_eq!(bars[0].bar_type().spec().step(), 5);
    assert_eq!(
        bars[0].bar_type().spec().aggregation(),
        BarAggregation::Minute
    );
    assert_eq!(bars[0].bar_type().spec().price_type(), PriceType::Bid);
}

#[test]
fn test_parquet_bar_source_missing_column() {
    let temp = TempFileGuard::new("missing_column.parquet");
    let file = File::create(temp.path()).expect("create temp file");

    let incomplete_schema = Arc::new(Schema::new(vec![
        Field::new("ts", DataType::Int64, false),
        Field::new("open", DataType::Float64, false),
        Field::new("high", DataType::Float64, false),
        Field::new("low", DataType::Float64, false),
        Field::new("volume", DataType::Float64, false),
    ]));

    let ts = Arc::new(Int64Array::from(vec![1_700_000_000_000_000_000]));
    let open = Arc::new(Float64Array::from(vec![100.0]));
    let high = Arc::new(Float64Array::from(vec![105.0]));
    let low = Arc::new(Float64Array::from(vec![98.0]));
    let volume = Arc::new(Float64Array::from(vec![200.0]));

    let batch = RecordBatch::try_new(incomplete_schema.clone(), vec![ts, open, high, low, volume])
        .expect("create batch");

    let mut writer = ArrowWriter::try_new(file, incomplete_schema, None).expect("arrow writer");
    writer.write(&batch).expect("write batch");
    writer.close().expect("close writer");

    let inst = InstrumentId::new("RELIANCE", Exchange::new("NSE"));
    let source = ParquetBarSource::new(temp.path(), inst);
    let err = source.bars().expect_err("should fail with missing column");

    match err {
        ParquetError::MissingColumn(col) => assert_eq!(col, "close"),
        other => panic!("expected MissingColumn(\"close\"), got {:?}", other),
    }
}

#[test]
fn test_parquet_bar_source_wrong_data_type() {
    let temp = TempFileGuard::new("wrong_type.parquet");
    let file = File::create(temp.path()).expect("create temp file");

    let wrong_schema = Arc::new(Schema::new(vec![
        Field::new("ts", DataType::Utf8, false),
        Field::new("open", DataType::Float64, false),
        Field::new("high", DataType::Float64, false),
        Field::new("low", DataType::Float64, false),
        Field::new("close", DataType::Float64, false),
        Field::new("volume", DataType::Float64, false),
    ]));

    let ts = Arc::new(StringArray::from(vec!["2024-01-01"]));
    let open = Arc::new(Float64Array::from(vec![100.0]));
    let high = Arc::new(Float64Array::from(vec![105.0]));
    let low = Arc::new(Float64Array::from(vec![98.0]));
    let close = Arc::new(Float64Array::from(vec![102.0]));
    let volume = Arc::new(Float64Array::from(vec![200.0]));

    let batch = RecordBatch::try_new(
        wrong_schema.clone(),
        vec![ts, open, high, low, close, volume],
    )
    .expect("create batch");

    let mut writer = ArrowWriter::try_new(file, wrong_schema, None).expect("arrow writer");
    writer.write(&batch).expect("write batch");
    writer.close().expect("close writer");

    let inst = InstrumentId::new("SBIN", Exchange::new("NSE"));
    let source = ParquetBarSource::new(temp.path(), inst);
    let err = source.bars().expect_err("should fail with wrong type");

    match err {
        ParquetError::WrongType { name, got, want } => {
            assert_eq!(name, "ts");
            assert_eq!(got, DataType::Utf8);
            assert_eq!(want, DataType::Int64);
        }
        other => panic!("expected WrongType for ts, got {:?}", other),
    }
}

#[test]
fn test_parquet_bar_source_non_existent_file() {
    let inst = InstrumentId::new("UNKNOWN", Exchange::new("NSE"));
    let source = ParquetBarSource::new("/tmp/non_existent_parquet_file_12345.parquet", inst);
    let err = source.bars().expect_err("should fail on missing file");

    match err {
        ParquetError::Io(e) => assert_eq!(e.kind(), std::io::ErrorKind::NotFound),
        other => panic!("expected ParquetError::Io, got {:?}", other),
    }
}
