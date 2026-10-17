//! Integration tests for honba-feed crate.

use std::fs::File;
use std::sync::Arc;

use arrow::array::{Float64Array, Int64Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;

use honba_analytics::{EquityStats, PerformanceReport, TradeStats};
use honba_feed::export::{CsvReportWriter, JsonReportWriter, MarkdownReportWriter, ReportWriter};
use honba_feed::import::parquet_source::{bar_schema, ParquetBarSource, ParquetError};
use honba_messages::{BarAggregation, BarSpecification, Exchange, InstrumentId, PriceType};

fn sample_report() -> PerformanceReport {
    PerformanceReport {
        trades: TradeStats {
            n_trades: 10,
            n_wins: 6,
            n_losses: 4,
            n_flat: 0,
            win_rate: 0.60,
            gross_profit: 1200.0,
            gross_loss: 400.0,
            profit_factor: Some(3.0),
            avg_win: Some(200.0),
            avg_loss: Some(-100.0),
            total_pnl: 800.0,
            expectancy: 80.0,
            total_fees: 15.0,
        },
        equity: EquityStats {
            n_periods: 252,
            total_return: 0.25,
            annualized_return: 0.25,
            annualized_volatility: 0.15,
            sharpe: Some(1.6667),
            sortino: Some(2.1000),
            max_drawdown: 50.0,
            max_drawdown_pct: 0.05,
            calmar: Some(5.0),
        },
    }
}

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
        path.push(format!("honba_test_{unique_id}_{name}"));
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
fn test_json_report_writer_compact_and_pretty() {
    let report = sample_report();

    // Compact
    let mut compact_buf = Vec::new();
    let mut compact_writer = JsonReportWriter::new(&mut compact_buf);
    compact_writer.write(&report).expect("compact write");
    let compact_str = String::from_utf8(compact_buf).expect("utf8");
    assert!(!compact_str.contains('\n'));

    let deserialized: PerformanceReport =
        serde_json::from_str(&compact_str).expect("deserialize compact");
    assert_eq!(deserialized, report);

    // Pretty
    let mut pretty_buf = Vec::new();
    let mut pretty_writer = JsonReportWriter::pretty(&mut pretty_buf);
    pretty_writer.write(&report).expect("pretty write");
    let pretty_str = String::from_utf8(pretty_buf).expect("utf8");
    assert!(pretty_str.contains('\n'));

    let deserialized_pretty: PerformanceReport =
        serde_json::from_str(&pretty_str).expect("deserialize pretty");
    assert_eq!(deserialized_pretty, report);
}

#[test]
fn test_csv_report_writer() {
    let report = sample_report();
    let mut buf = Vec::new();
    let mut writer = CsvReportWriter::new(&mut buf);
    writer.write(&report).expect("csv write");

    let csv_str = String::from_utf8(buf).expect("utf8");
    let lines: Vec<&str> = csv_str.lines().collect();

    assert_eq!(lines[0], "metric,value");
    assert!(lines.contains(&"trades.n_trades,10"));
    assert!(lines.contains(&"trades.win_rate,0.6"));
    assert!(lines.contains(&"trades.profit_factor,3"));
    assert!(lines.contains(&"equity.n_periods,252"));
    assert!(lines.contains(&"equity.total_return,0.25"));
    assert!(lines.contains(&"equity.sharpe,1.6667"));
}

#[test]
fn test_markdown_report_writer() {
    let report = sample_report();
    let mut buf = Vec::new();
    let mut writer = MarkdownReportWriter::new(&mut buf);
    writer.write(&report).expect("markdown write");

    let md_str = String::from_utf8(buf).expect("utf8");
    assert!(md_str.contains("# Performance Report"));
    assert!(md_str.contains("## Trades"));
    assert!(md_str.contains("| Win rate | 60.00% |"));
    assert!(md_str.contains("## Equity"));
    assert!(md_str.contains("| Sharpe | 1.6667 |"));
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
        // "close" is missing
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
        Field::new("ts", DataType::Utf8, false), // should be Int64
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
