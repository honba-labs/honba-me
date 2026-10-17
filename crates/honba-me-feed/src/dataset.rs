//! Shared, immutable, columnar datasets.
//!
//! A [`Dataset`] is the read-only input a backtest or a parameter sweep runs
//! against: one [`ColumnarSlice`] per instrument, ordered by [`InstrumentId`].
//! Both types are `Send + Sync + 'static` and cheap to clone, so one
//! `Arc<Dataset>` can be shared by every trial in a sweep without copying the
//! bars.
//!
//! [`DatasetFeed`] replays a dataset into the sync event kernel
//! ([`honba_engine::DataFeed`]) in a fixed order, which is what makes a sweep's
//! journals comparable across thread counts.

use std::path::PathBuf;
use std::sync::Arc;

use honba_engine::DataFeed;
use honba_messages::{
    Bar, BarSpecification, BarType, DatasetId, Event, InstrumentId, Message, UnixNanos,
};
use thiserror::Error;

use crate::import::parquet_source::{ParquetBarSource, ParquetError};

/// Immutable, columnar bars for one instrument, cheap to clone.
///
/// The columns are held once behind an [`Arc`], so a clone shares them instead
/// of copying, and [`ColumnarSlice::bar`] materialises a single [`Bar`] on
/// demand. A slice is built once through a [`ColumnarSliceBuilder`], which
/// rejects rows that break the dataset invariants
/// ([`DatasetBuildError`]) instead of storing them.
///
/// ```
/// use honba_feed::{ColumnarSliceBuilder, Dataset};
/// use honba_messages::{BarAggregation, BarSpecification, Exchange, InstrumentId, PriceType, UnixNanos};
///
/// let id = InstrumentId::new("TCS", Exchange::new("NSE"));
/// let spec = BarSpecification::new(1, BarAggregation::Minute, PriceType::Last);
/// let mut builder = ColumnarSliceBuilder::new(id.clone(), spec);
/// builder.push(UnixNanos::from_u64(100), 10.0, 12.0, 9.0, 11.0, 500.0)?;
/// builder.push(UnixNanos::from_u64(200), 11.0, 13.0, 10.0, 12.0, 600.0)?;
///
/// let slice = builder.finish()?;
/// assert_eq!(slice.len(), 2);
/// assert_eq!(slice.closes(), &[11.0, 12.0]);
/// assert_eq!(slice.bar(1).map(|bar| bar.close()), Some(12.0));
///
/// let dataset = Dataset::from_slices(vec![slice])?;
/// assert_eq!(dataset.bar_count(), 2);
/// assert!(dataset.slice(&id).is_some());
/// # Ok::<(), honba_feed::DatasetBuildError>(())
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnarSlice(Arc<SliceInner>);

/// The single allocation behind every [`ColumnarSlice`] handle.
#[derive(Debug, PartialEq)]
struct SliceInner {
    instrument: InstrumentId,
    bar_spec: BarSpecification,
    ts: Vec<UnixNanos>,
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    close: Vec<f64>,
    volume: Vec<f64>,
}

impl ColumnarSlice {
    /// Starts a builder for one instrument and bar specification.
    pub fn builder(instrument: InstrumentId, bar_spec: BarSpecification) -> ColumnarSliceBuilder {
        ColumnarSliceBuilder::new(instrument, bar_spec)
    }

    /// Returns the instrument these bars belong to.
    pub fn instrument(&self) -> &InstrumentId {
        &self.0.instrument
    }

    /// Returns how the bars aggregate their inputs.
    pub fn bar_spec(&self) -> &BarSpecification {
        &self.0.bar_spec
    }

    /// Returns the number of bars.
    pub fn len(&self) -> usize {
        self.0.ts.len()
    }

    /// Returns `true` when there are no bars.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the bar timestamps, in non-decreasing order.
    pub fn timestamps(&self) -> &[UnixNanos] {
        &self.0.ts
    }

    /// Returns the open prices.
    pub fn opens(&self) -> &[f64] {
        &self.0.open
    }

    /// Returns the high prices.
    pub fn highs(&self) -> &[f64] {
        &self.0.high
    }

    /// Returns the low prices.
    pub fn lows(&self) -> &[f64] {
        &self.0.low
    }

    /// Returns the close prices.
    pub fn closes(&self) -> &[f64] {
        &self.0.close
    }

    /// Returns the volumes.
    pub fn volumes(&self) -> &[f64] {
        &self.0.volume
    }

    /// Builds the bar at `index`, or `None` when `index` is out of range.
    ///
    /// The bar is constructed on demand and its `ts_event` and `ts_init` are
    /// both the column timestamp: a stored column has no separate creation
    /// time. Use [`ColumnarSlice::messages`] to stamp a run's `ts_init`.
    pub fn bar(&self, index: usize) -> Option<Bar> {
        (index < self.len()).then(|| self.bar_at(index))
    }

    /// Every bar as a kernel [`Message`], all stamped with `ts_init`.
    pub fn messages(&self, ts_init: UnixNanos) -> Vec<Message> {
        (0..self.len())
            .map(|index| Message::new(Event::Bar(self.bar_at(index)), ts_init))
            .collect()
    }

    fn bar_at(&self, index: usize) -> Bar {
        let inner = &self.0;
        let ts = inner.ts[index];
        Bar::new(
            BarType::new(inner.instrument.clone(), inner.bar_spec),
            inner.open[index],
            inner.high[index],
            inner.low[index],
            inner.close[index],
            inner.volume[index],
            ts,
            ts,
        )
    }

    /// Extracts a contiguous subslice from `[start, start + len)`.
    ///
    /// Returns `None` if `start + len > self.len()`.
    pub fn subslice(&self, start: usize, len: usize) -> Option<Self> {
        if start + len > self.len() {
            return None;
        }
        let inner = &self.0;
        Some(Self(Arc::new(SliceInner {
            instrument: inner.instrument.clone(),
            bar_spec: inner.bar_spec,
            ts: inner.ts[start..start + len].to_vec(),
            open: inner.open[start..start + len].to_vec(),
            high: inner.high[start..start + len].to_vec(),
            low: inner.low[start..start + len].to_vec(),
            close: inner.close[start..start + len].to_vec(),
            volume: inner.volume[start..start + len].to_vec(),
        })))
    }

    /// Partitions the slice into contiguous chunks of at most `chunk_size` bars.
    ///
    /// # Panics
    /// Panics if `chunk_size == 0`.
    pub fn chunks(&self, chunk_size: usize) -> Vec<Self> {
        assert!(chunk_size > 0, "chunk_size must be positive");
        if self.is_empty() {
            return Vec::new();
        }
        let mut result = Vec::new();
        let total = self.len();
        let mut offset = 0;
        while offset < total {
            let len = chunk_size.min(total - offset);
            result.push(self.subslice(offset, len).expect("valid subslice bounds"));
            offset += len;
        }
        result
    }

    /// Reports whether two handles point at the same columns.
    #[cfg(test)]
    pub(crate) fn ptr_eq(left: &Self, right: &Self) -> bool {
        Arc::ptr_eq(&left.0, &right.0)
    }
}

impl PartialEq for Dataset {
    fn eq(&self, other: &Self) -> bool {
        self.0.id == other.0.id && self.0.slices == other.0.slices
    }
}

/// A chunk partition of a dataset.
#[derive(Clone, Debug, PartialEq)]
pub struct DatasetChunk {
    /// Zero-based chunk index.
    pub index: usize,
    /// Total number of chunks in the partition.
    pub total_chunks: usize,
    /// The dataset partition for this chunk.
    pub dataset: Dataset,
}

/// A sync [`DataFeed`] that replays a [`Dataset`] in chunked streaming batches.
pub struct StreamedDatasetFeed {
    chunks: Vec<DatasetChunk>,
    current_chunk: usize,
    current_feed: Option<DatasetFeed>,
    ts_init: UnixNanos,
}

impl StreamedDatasetFeed {
    /// Creates a feed that replays `dataset` chunk by chunk.
    pub fn new(dataset: &Dataset, chunk_size: usize, ts_init: UnixNanos) -> Self {
        let chunks = dataset.chunks(chunk_size);
        let current_feed = chunks
            .first()
            .map(|c| DatasetFeed::new(&c.dataset, ts_init));
        Self {
            chunks,
            current_chunk: 0,
            current_feed,
            ts_init,
        }
    }
}

impl DataFeed for StreamedDatasetFeed {
    fn next(&mut self) -> honba_engine::Result<Option<Message>> {
        while let Some(feed) = &mut self.current_feed {
            if let Some(msg) = feed.next()? {
                return Ok(Some(msg));
            }
            self.current_chunk += 1;
            if self.current_chunk < self.chunks.len() {
                self.current_feed = Some(DatasetFeed::new(
                    &self.chunks[self.current_chunk].dataset,
                    self.ts_init,
                ));
            } else {
                self.current_feed = None;
            }
        }
        Ok(None)
    }
}

/// Builds a [`ColumnarSlice`], validating every row as it arrives.
///
/// Rows are appended in ascending timestamp order and every price and volume
/// must be finite; a row that breaks either rule is rejected with a
/// [`DatasetBuildError`] and the builder is left unchanged, so a failed build
/// can never leave a half-written slice behind. [`ColumnarSliceBuilder::finish`]
/// hands out one immutable slice that is cheap to clone.
pub struct ColumnarSliceBuilder {
    instrument: InstrumentId,
    bar_spec: BarSpecification,
    ts: Vec<UnixNanos>,
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    close: Vec<f64>,
    volume: Vec<f64>,
}

impl ColumnarSliceBuilder {
    /// Creates an empty builder.
    pub fn new(instrument: InstrumentId, bar_spec: BarSpecification) -> Self {
        Self {
            instrument,
            bar_spec,
            ts: Vec::new(),
            open: Vec::new(),
            high: Vec::new(),
            low: Vec::new(),
            close: Vec::new(),
            volume: Vec::new(),
        }
    }

    /// Returns the number of rows appended so far.
    pub fn len(&self) -> usize {
        self.ts.len()
    }

    /// Returns `true` when no rows have been appended.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Appends one row.
    pub fn push(
        &mut self,
        ts: UnixNanos,
        open: f64,
        high: f64,
        low: f64,
        close: f64,
        volume: f64,
    ) -> Result<(), DatasetBuildError> {
        let row = Row {
            ts,
            open,
            high,
            low,
            close,
            volume,
        };
        let index = self.len();
        self.check_row(index, self.ts.last().copied(), &row)?;
        self.commit(row);
        Ok(())
    }

    /// Appends one row taken from a [`Bar`], using its `ts_event`.
    pub fn push_bar(&mut self, bar: &Bar) -> Result<(), DatasetBuildError> {
        self.push(
            bar.ts_event(),
            bar.open(),
            bar.high(),
            bar.low(),
            bar.close(),
            bar.volume(),
        )
    }

    /// Appends a block of rows given as parallel column slices.
    ///
    /// The timestamp column sets the expected length: a column of any other
    /// length is a [`DatasetBuildError::LengthMismatch`], and the whole block
    /// is rejected or accepted together.
    pub fn push_rows(
        &mut self,
        ts: &[UnixNanos],
        open: &[f64],
        high: &[f64],
        low: &[f64],
        close: &[f64],
        volume: &[f64],
    ) -> Result<(), DatasetBuildError> {
        let expected = ts.len();
        for got in [open.len(), high.len(), low.len(), close.len(), volume.len()] {
            if got != expected {
                return Err(DatasetBuildError::LengthMismatch { expected, got });
            }
        }

        let row_at = |offset: usize| Row {
            ts: ts[offset],
            open: open[offset],
            high: high[offset],
            low: low[offset],
            close: close[offset],
            volume: volume[offset],
        };

        let mut previous = self.ts.last().copied();
        for offset in 0..expected {
            let row = row_at(offset);
            self.check_row(self.len() + offset, previous, &row)?;
            previous = Some(row.ts);
        }
        for offset in 0..expected {
            self.commit(row_at(offset));
        }
        Ok(())
    }

    /// Finishes the slice, handing out shared columns.
    pub fn finish(self) -> Result<ColumnarSlice, DatasetBuildError> {
        Ok(ColumnarSlice(Arc::new(SliceInner {
            instrument: self.instrument,
            bar_spec: self.bar_spec,
            ts: self.ts,
            open: self.open,
            high: self.high,
            low: self.low,
            close: self.close,
            volume: self.volume,
        })))
    }

    fn check_row(
        &self,
        index: usize,
        previous: Option<UnixNanos>,
        row: &Row,
    ) -> Result<(), DatasetBuildError> {
        if let Some(previous) = previous {
            if row.ts < previous {
                return Err(DatasetBuildError::NonMonotonicTimestamp {
                    index,
                    previous: previous.as_u64(),
                    got: row.ts.as_u64(),
                });
            }
        }
        for (column, value) in [
            ("open", row.open),
            ("high", row.high),
            ("low", row.low),
            ("close", row.close),
            ("volume", row.volume),
        ] {
            if !value.is_finite() {
                return Err(DatasetBuildError::NonFiniteValue { index, column });
            }
        }
        Ok(())
    }

    fn commit(&mut self, row: Row) {
        self.ts.push(row.ts);
        self.open.push(row.open);
        self.high.push(row.high);
        self.low.push(row.low);
        self.close.push(row.close);
        self.volume.push(row.volume);
    }
}

/// One bar's worth of values on their way into the columns.
#[derive(Clone, Copy, Debug)]
struct Row {
    ts: UnixNanos,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
}

/// A shared, read-only, deterministically ordered set of instruments and bars.
///
/// The slices are ordered by [`InstrumentId`] and each instrument appears at
/// most once: two slices for one instrument are rejected with
/// [`DatasetBuildError::DuplicateInstrument`] rather than silently keeping one
/// of them, so the order a replay produces depends on the data alone and not on
/// the order the caller happened to supply. Cloning a dataset copies the
/// [`Arc`], not the bars, and the dataset is `Send + Sync + 'static`: share it
/// with `Arc<Dataset>` across the threads of a sweep.
///
/// ```
/// use honba_feed::{ColumnarSliceBuilder, Dataset};
/// use honba_messages::{BarAggregation, BarSpecification, Exchange, InstrumentId, PriceType, UnixNanos};
///
/// fn slice(symbol: &str, ts: u64) -> honba_feed::ColumnarSlice {
///     let spec = BarSpecification::new(1, BarAggregation::Minute, PriceType::Last);
///     let mut builder = ColumnarSliceBuilder::new(InstrumentId::new(symbol, Exchange::new("NSE")), spec);
///     builder.push(UnixNanos::from_u64(ts), 1.0, 1.0, 1.0, 1.0, 1.0).unwrap();
///     builder.finish().unwrap()
/// }
///
/// let dataset = Dataset::from_slices(vec![slice("TCS", 2), slice("INFY", 1)])?;
/// let order: Vec<&str> = dataset.instruments().map(|id| id.symbol()).collect();
/// assert_eq!(order, vec!["INFY", "TCS"]);
/// assert_eq!(dataset.bar_count(), 2);
/// # Ok::<(), honba_feed::DatasetBuildError>(())
/// ```
#[derive(Clone, Debug)]
pub struct Dataset(Arc<DatasetInner>);

/// The single allocation behind every [`Dataset`] handle.
#[derive(Debug)]
struct DatasetInner {
    id: Option<DatasetId>,
    slices: Vec<ColumnarSlice>,
}

impl Default for Dataset {
    fn default() -> Self {
        Self::new()
    }
}

impl Dataset {
    /// Creates an empty dataset.
    pub fn new() -> Self {
        Self(Arc::new(DatasetInner {
            id: None,
            slices: Vec::new(),
        }))
    }

    /// Builds a dataset from slices, ordered by instrument id.
    ///
    /// The input order is irrelevant: slices are sorted by
    /// [`InstrumentId`], and a repeated instrument is an error.
    pub fn from_slices(slices: Vec<ColumnarSlice>) -> Result<Dataset, DatasetBuildError> {
        Self::from_slices_with_id(slices, None)
    }

    /// Builds a dataset from slices with an explicit [`DatasetId`].
    pub fn from_slices_with_id(
        slices: Vec<ColumnarSlice>,
        id: Option<DatasetId>,
    ) -> Result<Dataset, DatasetBuildError> {
        let mut slices = slices;
        slices.sort_by(|left, right| left.instrument().cmp(right.instrument()));
        for pair in slices.windows(2) {
            if pair[0].instrument() == pair[1].instrument() {
                return Err(DatasetBuildError::DuplicateInstrument(
                    pair[0].instrument().clone(),
                ));
            }
        }
        Ok(Self(Arc::new(DatasetInner { id, slices })))
    }

    /// Sets the dataset ID, returning the updated dataset.
    pub fn with_id(mut self, id: DatasetId) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.0) {
            inner.id = Some(id);
            self
        } else {
            Self(Arc::new(DatasetInner {
                id: Some(id),
                slices: self.0.slices.clone(),
            }))
        }
    }

    /// Returns the dataset ID if configured.
    pub fn id(&self) -> Option<&DatasetId> {
        self.0.id.as_ref()
    }

    /// Partitions the dataset into chunk partitions with at most `chunk_size` bars per slice.
    ///
    /// # Panics
    /// Panics if `chunk_size == 0`.
    pub fn chunks(&self, chunk_size: usize) -> Vec<DatasetChunk> {
        assert!(chunk_size > 0, "chunk_size must be positive");
        if self.is_empty() {
            return Vec::new();
        }
        let per_slice_chunks: Vec<Vec<ColumnarSlice>> =
            self.0.slices.iter().map(|s| s.chunks(chunk_size)).collect();
        let max_chunks = per_slice_chunks.iter().map(|c| c.len()).max().unwrap_or(0);
        let mut chunks = Vec::with_capacity(max_chunks);
        for chunk_idx in 0..max_chunks {
            let mut chunk_slices = Vec::new();
            for slice_chunks in &per_slice_chunks {
                if let Some(c) = slice_chunks.get(chunk_idx) {
                    chunk_slices.push(c.clone());
                }
            }
            let ds = Dataset::from_slices_with_id(chunk_slices, self.0.id.clone())
                .expect("partition slices are sorted and unique");
            chunks.push(DatasetChunk {
                index: chunk_idx,
                total_chunks: max_chunks,
                dataset: ds,
            });
        }
        chunks
    }

    /// Reads one Parquet file per instrument and builds a dataset from them.
    ///
    /// This blocks on I/O; the async loaders call it from `spawn_blocking`.
    /// Each file is read with the default one-minute, last-price bar
    /// specification ([`ParquetBarSource::new`]), and the slices are ordered
    /// and de-duplicated by [`Dataset::from_slices`].
    ///
    /// A read failure comes back as [`DatasetBuildError::Parquet`]; a file
    /// whose rows break the dataset invariants comes back as that invariant
    /// error itself, so a caller can tell a broken file from a dirty one.
    pub fn from_parquet(files: &[(InstrumentId, PathBuf)]) -> Result<Dataset, DatasetBuildError> {
        let mut slices = Vec::with_capacity(files.len());
        for (instrument, path) in files {
            let slice = ParquetBarSource::new(path, instrument.clone())
                .columnar()
                .map_err(dataset_error_from_parquet)?;
            slices.push(slice);
        }
        Dataset::from_slices(slices)
    }

    /// Returns the number of instruments.
    pub fn len(&self) -> usize {
        self.0.slices.len()
    }

    /// Returns `true` when the dataset holds no instruments.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the instruments, in dataset order.
    pub fn instruments(&self) -> impl Iterator<Item = &InstrumentId> + '_ {
        self.0.slices.iter().map(ColumnarSlice::instrument)
    }

    /// Returns the slice for `instrument`, or `None` when it is absent.
    pub fn slice(&self, instrument: &InstrumentId) -> Option<&ColumnarSlice> {
        self.0
            .slices
            .binary_search_by(|slice| slice.instrument().cmp(instrument))
            .ok()
            .map(|index| &self.0.slices[index])
    }

    /// Returns every slice, in dataset order.
    pub fn slices(&self) -> &[ColumnarSlice] {
        &self.0.slices
    }

    /// Returns the total number of bars across every slice.
    pub fn bar_count(&self) -> usize {
        self.0.slices.iter().map(ColumnarSlice::len).sum()
    }

    /// Reports whether two handles point at the same slices.
    #[cfg(test)]
    pub(crate) fn ptr_eq(left: &Self, right: &Self) -> bool {
        Arc::ptr_eq(&left.0, &right.0)
    }
}

/// A sync [`DataFeed`] that replays a [`Dataset`] in deterministic order.
///
/// The ordering is the point of this feed and is fixed: every bar of one
/// instrument in ascending `ts_event`, instruments in ascending
/// [`InstrumentId`], and never interleaved. Two runs over the same dataset
/// therefore dispatch the same events in the same order, whatever the thread
/// count, which is what lets a sweep compare journals across trials. The kernel
/// still re-orders by `ts_event` when it queues a batch, so use
/// `Engine::with_batch_size(1)` for a dataset whose instruments overlap in
/// time and must be dispatched in feed order.
///
/// [`DatasetFeed::new`] takes the run's init timestamp, which is stamped on
/// every message it produces.
pub struct DatasetFeed {
    slices: Vec<ColumnarSlice>,
    current: usize,
    cursor: usize,
    ts_init: UnixNanos,
}

impl DatasetFeed {
    /// Creates a feed that replays `dataset` from its first bar.
    pub fn new(dataset: &Dataset, ts_init: UnixNanos) -> Self {
        Self {
            slices: dataset.slices().to_vec(),
            current: 0,
            cursor: 0,
            ts_init,
        }
    }
}

impl DataFeed for DatasetFeed {
    fn next(&mut self) -> honba_engine::Result<Option<Message>> {
        while self.current < self.slices.len() {
            let slice = &self.slices[self.current];
            if self.cursor < slice.len() {
                let bar = slice.bar_at(self.cursor);
                self.cursor += 1;
                return Ok(Some(Message::new(Event::Bar(bar), self.ts_init)));
            }
            self.current += 1;
            self.cursor = 0;
        }
        Ok(None)
    }
}

/// Why a [`ColumnarSlice`] or a [`Dataset`] could not be built.
///
/// Equality compares each variant's fields, except
/// [`DatasetBuildError::Parquet`], which compares the rendered message:
/// [`ParquetError`] is not `PartialEq`, and the typed error stays reachable
/// through that variant's [`source`](std::error::Error::source) for anything
/// finer.
#[derive(Debug, Error)]
pub enum DatasetBuildError {
    /// Two slices claimed the same instrument.
    #[error("duplicate instrument: {0}")]
    DuplicateInstrument(InstrumentId),
    /// A timestamp went backwards.
    #[error("timestamp at row {index} went backwards: {previous} then {got}")]
    NonMonotonicTimestamp {
        /// Index of the offending row.
        index: usize,
        /// The previous row's timestamp, in nanoseconds since the Unix epoch.
        previous: u64,
        /// The offending timestamp, in nanoseconds since the Unix epoch.
        got: u64,
    },
    /// Columns of different lengths were appended together.
    #[error("column of {got} values does not match {expected}")]
    LengthMismatch {
        /// Length of the timestamp column, the reference for the others.
        expected: usize,
        /// Length of the column that did not match.
        got: usize,
    },
    /// A price or volume was not finite.
    #[error("column {column} is not finite at row {index}")]
    NonFiniteValue {
        /// Index of the offending row.
        index: usize,
        /// Name of the offending column.
        column: &'static str,
    },
    /// A Parquet file could not be read.
    ///
    /// [`ParquetError`] is not `PartialEq`, so this variant keeps the typed
    /// error as its [`source`](std::error::Error::source) and compares the
    /// rendered `message` instead.
    #[error("parquet: {message}")]
    Parquet {
        /// The rendered Parquet error.
        message: String,
        /// The typed Parquet error.
        #[source]
        source: ParquetError,
    },
}

impl PartialEq for DatasetBuildError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::DuplicateInstrument(left), Self::DuplicateInstrument(right)) => left == right,
            (
                Self::NonMonotonicTimestamp {
                    index: left_index,
                    previous: left_previous,
                    got: left_got,
                },
                Self::NonMonotonicTimestamp {
                    index: right_index,
                    previous: right_previous,
                    got: right_got,
                },
            ) => {
                left_index == right_index
                    && left_previous == right_previous
                    && left_got == right_got
            }
            (
                Self::LengthMismatch {
                    expected: left_expected,
                    got: left_got,
                },
                Self::LengthMismatch {
                    expected: right_expected,
                    got: right_got,
                },
            ) => left_expected == right_expected && left_got == right_got,
            (
                Self::NonFiniteValue {
                    index: left_index,
                    column: left_column,
                },
                Self::NonFiniteValue {
                    index: right_index,
                    column: right_column,
                },
            ) => left_index == right_index && left_column == right_column,
            (Self::Parquet { message: left, .. }, Self::Parquet { message: right, .. }) => {
                left == right
            }
            _ => false,
        }
    }
}

impl From<ParquetError> for DatasetBuildError {
    fn from(source: ParquetError) -> Self {
        Self::Parquet {
            message: source.to_string(),
            source,
        }
    }
}

fn dataset_error_from_parquet(source: ParquetError) -> DatasetBuildError {
    match source {
        ParquetError::InvalidBarData(inner) => *inner,
        other => DatasetBuildError::from(other),
    }
}
