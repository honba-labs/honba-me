//! Parquet bar source.
//!
//! Expects a single file with columns:
//! `ts` (int64, UnixNanos), `open`, `high`, `low`, `close` (float64),
//! `volume` (float64). Extra columns are ignored.
//!
//! The bar type is fixed at construction; all rows are attributed to the
//! same instrument and specification.
//!
//! Three read paths share one set of column rules:
//! [`ParquetBarSource::batches`] streams record batches,
//! [`ParquetBarSource::columnar`] builds the shared [`ColumnarSlice`] those
//! batches feed, and [`ParquetBarSource::bars`] materialises owned [`Bar`]s
//! from that slice. Rows whose `ts` is null are skipped by all of them; rows
//! that break a dataset invariant (a timestamp that goes backwards, a price or
//! volume that is not finite) are a typed error rather than a silent skip,
//! because a sweep cannot replay what it is not told about.

use std::fs::File;
use std::path::Path;

use arrow::array::{Array, Float64Array, Int64Array};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use parquet::arrow::arrow_reader::{ParquetRecordBatchReader, ParquetRecordBatchReaderBuilder};
use thiserror::Error;

use honba_messages::{
    Bar, BarAggregation, BarSpecification, BarType, InstrumentId, PriceType, UnixNanos,
};

use crate::dataset::{ColumnarSlice, ColumnarSliceBuilder, DatasetBuildError};

/// Errors that can occur when reading parquet files.
#[derive(Debug, Error)]
pub enum ParquetError {
    /// An I/O error occurred.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// A parquet reader error occurred.
    #[error("parquet error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),
    /// An Arrow array conversion error occurred.
    #[error("arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),
    /// A required column is missing.
    #[error("missing column: {0}")]
    MissingColumn(&'static str),
    /// A column had an unexpected data type.
    #[error("column {name} has type {got:?}, expected {want:?}")]
    WrongType {
        /// Name of the column.
        name: &'static str,
        /// Actual data type found.
        got: DataType,
        /// Expected data type.
        want: DataType,
    },
    /// The file's rows break a dataset invariant.
    ///
    /// This wraps the [`DatasetBuildError`] the columnar read raised, so one
    /// error type covers both a file that cannot be read and a file whose bars
    /// cannot be used. It is boxed because a dataset build error can itself
    /// wrap this error.
    #[error("invalid bar data: {0}")]
    InvalidBarData(#[source] Box<DatasetBuildError>),
}

impl From<DatasetBuildError> for ParquetError {
    fn from(error: DatasetBuildError) -> Self {
        ParquetError::InvalidBarData(Box::new(error))
    }
}

/// A reader that parses Parquet files into [`Bar`] streams.
pub struct ParquetBarSource {
    bar_type: BarType,
    path: std::path::PathBuf,
}

impl ParquetBarSource {
    /// Default spec: 1-minute bars, last price. Override with
    /// [`ParquetBarSource::with_spec`] when the file contains other intervals.
    pub fn new(path: impl AsRef<Path>, instrument: InstrumentId) -> Self {
        let spec = BarSpecification::new(1, BarAggregation::Minute, PriceType::Last);
        Self::with_spec(path, instrument, spec)
    }

    /// Creates a source with a custom bar specification.
    pub fn with_spec(
        path: impl AsRef<Path>,
        instrument: InstrumentId,
        spec: BarSpecification,
    ) -> Self {
        Self {
            bar_type: BarType::new(instrument, spec),
            path: path.as_ref().to_path_buf(),
        }
    }

    /// Reads all bars from the underlying Parquet file.
    ///
    /// This is [`ParquetBarSource::columnar`] mapped to owned bars: the same
    /// rows, in the same order, with each bar's `ts_event` and `ts_init` both
    /// set to its column timestamp.
    pub fn bars(&self) -> Result<Vec<Bar>, ParquetError> {
        let slice = self.columnar()?;
        Ok(bars_of(&slice))
    }

    /// Reads the file into one shared [`ColumnarSlice`].
    ///
    /// The record batches are streamed through a [`ColumnarSliceBuilder`], so
    /// the columns are built once and the result is `Send + Sync` and cheap to
    /// clone, rather than a `Vec<Bar>` per instrument. Rows whose `ts` is null
    /// are skipped; a missing column, a wrong column type, a backwards
    /// timestamp or a non-finite price or volume is a typed error.
    pub fn columnar(&self) -> Result<ColumnarSlice, ParquetError> {
        let mut builder =
            ColumnarSliceBuilder::new(self.bar_type.instrument_id().clone(), self.bar_type.spec());
        for batch in self.batches()? {
            self.append_batch(&batch?, &mut builder)?;
        }
        Ok(builder.finish()?)
    }

    /// Opens the file and streams its record batches.
    ///
    /// The returned iterator owns the file handle and yields one batch at a
    /// time, so a caller can walk row group by row group and process each batch
    /// before the next is read. This is the path that never materialises every
    /// row: nothing is held beyond the batch being read.
    pub fn batches(&self) -> Result<ParquetRecordBatchReader, ParquetError> {
        let file = File::open(&self.path)?;
        let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
        Ok(builder.build()?)
    }

    fn append_batch(
        &self,
        batch: &RecordBatch,
        builder: &mut ColumnarSliceBuilder,
    ) -> Result<(), ParquetError> {
        let schema = batch.schema();
        let idx = |name: &'static str| -> Result<usize, ParquetError> {
            schema
                .index_of(name)
                .map_err(|_| ParquetError::MissingColumn(name))
        };
        let i_ts = idx("ts")?;
        let i_o = idx("open")?;
        let i_h = idx("high")?;
        let i_l = idx("low")?;
        let i_c = idx("close")?;
        let i_v = idx("volume")?;

        let ts = downcast_i64(batch.column(i_ts).as_ref(), "ts")?;
        let o = downcast_f64(batch.column(i_o).as_ref(), "open")?;
        let h = downcast_f64(batch.column(i_h).as_ref(), "high")?;
        let l = downcast_f64(batch.column(i_l).as_ref(), "low")?;
        let c = downcast_f64(batch.column(i_c).as_ref(), "close")?;
        let v = downcast_f64(batch.column(i_v).as_ref(), "volume")?;

        if ts.null_count() == 0 {
            let timestamps: Vec<UnixNanos> = (0..batch.num_rows())
                .map(|row| UnixNanos::from_u64(ts.value(row) as u64))
                .collect();
            return Ok(builder.push_rows(
                &timestamps,
                o.values(),
                h.values(),
                l.values(),
                c.values(),
                v.values(),
            )?);
        }

        for row in 0..batch.num_rows() {
            if ts.is_null(row) {
                continue;
            }
            builder.push(
                UnixNanos::from_u64(ts.value(row) as u64),
                o.value(row),
                h.value(row),
                l.value(row),
                c.value(row),
                v.value(row),
            )?;
        }
        Ok(())
    }
}

fn bars_of(slice: &ColumnarSlice) -> Vec<Bar> {
    let bar_type = BarType::new(slice.instrument().clone(), *slice.bar_spec());
    slice
        .timestamps()
        .iter()
        .enumerate()
        .map(|(index, ts)| {
            Bar::new(
                bar_type.clone(),
                slice.opens()[index],
                slice.highs()[index],
                slice.lows()[index],
                slice.closes()[index],
                slice.volumes()[index],
                *ts,
                *ts,
            )
        })
        .collect()
}

fn downcast_i64<'a>(
    arr: &'a dyn Array,
    name: &'static str,
) -> Result<&'a Int64Array, ParquetError> {
    arr.as_any()
        .downcast_ref::<Int64Array>()
        .ok_or_else(|| ParquetError::WrongType {
            name,
            got: arr.data_type().clone(),
            want: DataType::Int64,
        })
}

fn downcast_f64<'a>(
    arr: &'a dyn Array,
    name: &'static str,
) -> Result<&'a Float64Array, ParquetError> {
    arr.as_any()
        .downcast_ref::<Float64Array>()
        .ok_or_else(|| ParquetError::WrongType {
            name,
            got: arr.data_type().clone(),
            want: DataType::Float64,
        })
}

/// Schema helper so producers and consumers agree on column layout.
pub fn bar_schema() -> Schema {
    Schema::new(vec![
        Field::new("ts", DataType::Int64, false),
        Field::new("open", DataType::Float64, false),
        Field::new("high", DataType::Float64, false),
        Field::new("low", DataType::Float64, false),
        Field::new("close", DataType::Float64, false),
        Field::new("volume", DataType::Float64, false),
    ])
}
