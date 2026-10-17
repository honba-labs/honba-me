//! A read-only market-data catalogue over a [`Dataset`].
//!
//! [`DatasetReader`] is the infrastructure adapter for two ports owned by
//! `honba-ports`: [`InstrumentMaster`] (what instruments exist) and
//! [`BarReader`] (their stored bars over a range); it also derives last-price quotes
//! ([`QuoteReader`]) and states that it holds no order book ([`DepthReader`]). The REST and MCP surfaces
//! depend on the ports, never on this type, so the data source can change
//! without touching them.
//!
//! The dataset is immutable and shared, so the reader is `Send + Sync` and
//! every answer is deterministic: instruments come back in [`InstrumentId`]
//! order and bars in ascending `ts_event`.

use std::collections::BTreeMap;
use std::path::Path;

use async_trait::async_trait;
use honba_entities::{Currency, Instrument, InstrumentKind};
use honba_messages::{Bar, Exchange, InstrumentId, QuoteTick, UnixNanos};
use honba_ports::{
    BarReader, BarRequest, DepthReader, DepthSnapshot, InstrumentMaster, PortError, PortResult,
    QuoteReader,
};
use thiserror::Error;

use crate::dataset::{Dataset, DatasetBuildError};

/// Lot size of an instrument whose metadata was derived, not supplied.
const DEFAULT_LOT_SIZE: f64 = 1.0;
/// Tick size of an instrument whose metadata was derived, not supplied.
const DEFAULT_TICK_SIZE: f64 = 0.05;

/// Why a [`DatasetReader`] could not be built from a directory.
#[derive(Debug, Error)]
pub enum ReaderError {
    /// The directory or one of its entries could not be read.
    #[error("cannot read data directory: {0}")]
    Io(#[from] std::io::Error),
    /// A `.parquet` file is not named `SYMBOL.EXCHANGE.parquet`.
    #[error("parquet file name {0:?} is not SYMBOL.EXCHANGE.parquet")]
    FileName(String),
    /// A file could not be read, or its rows broke the dataset invariants.
    #[error(transparent)]
    Dataset(#[from] DatasetBuildError),
}

/// Serves instrument listings and historical bars from an in-memory [`Dataset`].
#[derive(Clone, Debug)]
pub struct DatasetReader {
    dataset: Dataset,
    instruments: BTreeMap<InstrumentId, Instrument>,
}

impl DatasetReader {
    /// Creates a reader over `dataset` listing exactly `instruments`.
    ///
    /// A repeated id keeps the last entry. Metadata and bars are independent: an
    /// instrument may be listed without bars (its bar read is empty).
    pub fn new(dataset: Dataset, instruments: Vec<Instrument>) -> Self {
        let instruments = instruments
            .into_iter()
            .map(|instrument| (instrument.id().clone(), instrument))
            .collect();
        Self {
            dataset,
            instruments,
        }
    }

    /// Creates a reader that lists every instrument in `dataset` with derived
    /// metadata: an INR equity with lot size 1 and tick size 0.05.
    ///
    /// A bar file carries no reference data, so these are stand-ins until a
    /// symbol master supplies the real values through [`DatasetReader::new`].
    pub fn from_dataset(dataset: Dataset) -> Self {
        let instruments = dataset
            .instruments()
            .map(|id| {
                Instrument::new(
                    id.clone(),
                    InstrumentKind::Equity,
                    Currency::Inr,
                    DEFAULT_LOT_SIZE,
                    DEFAULT_TICK_SIZE,
                )
            })
            .collect();
        Self::new(dataset, instruments)
    }

    /// Loads every `SYMBOL.EXCHANGE.parquet` file in `dir` (non-recursive).
    ///
    /// Other files are ignored. Files are read in file-name order and each is
    /// read as one-minute last-price bars, the [`ParquetBarSource`] default. This
    /// blocks on I/O and is meant to run once at start-up.
    ///
    /// [`ParquetBarSource`]: crate::ParquetBarSource
    pub fn from_parquet_dir(dir: &Path) -> Result<Self, ReaderError> {
        let mut paths = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|ext| ext == "parquet") {
                paths.push(path);
            }
        }
        paths.sort();
        let mut files = Vec::with_capacity(paths.len());
        for path in paths {
            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default()
                .to_owned();
            match stem.rsplit_once('.') {
                Some((symbol, exchange)) if !symbol.is_empty() && !exchange.is_empty() => {
                    files.push((InstrumentId::new(symbol, Exchange::new(exchange)), path));
                }
                _ => return Err(ReaderError::FileName(file_name(&path))),
            }
        }
        Ok(Self::from_dataset(Dataset::from_parquet(&files)?))
    }

    /// Returns the number of listed instruments.
    pub fn len(&self) -> usize {
        self.instruments.len()
    }

    /// Returns `true` when no instrument is listed.
    pub fn is_empty(&self) -> bool {
        self.instruments.is_empty()
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[async_trait]
impl InstrumentMaster for DatasetReader {
    async fn get_instrument(&self, id: &InstrumentId) -> PortResult<Option<Instrument>> {
        Ok(self.instruments.get(id).cloned())
    }

    async fn list_instruments(&self) -> PortResult<Vec<Instrument>> {
        Ok(self.instruments.values().cloned().collect())
    }
}

#[async_trait]
impl BarReader for DatasetReader {
    async fn read_bars(&self, request: &BarRequest) -> PortResult<Vec<Bar>> {
        let Some(slice) = self.dataset.slice(request.instrument()) else {
            return Ok(Vec::new());
        };
        if *slice.bar_spec() != request.spec() {
            return Err(PortError::Unsupported(format!(
                "{} has no bars at the requested timeframe",
                request.instrument()
            )));
        }
        let stamps = slice.timestamps();
        let start = request
            .from()
            .map_or(0, |from| stamps.partition_point(|ts| *ts < from));
        let end = request
            .to()
            .map_or(stamps.len(), |to| stamps.partition_point(|ts| *ts < to));
        Ok((start..end.max(start))
            .filter_map(|index| slice.bar(index))
            .collect())
    }
}

/// A bar store has no bid or ask, so a quote is derived: bid and ask are both the close of the
/// latest bar stamped at or before the query time, with zero size on each side. The zero sizes
/// mark "last price, no book" so a consumer cannot mistake it for tradable liquidity. Both
/// timestamps are the bar's column timestamp, so the answer depends only on the data.
#[async_trait]
impl QuoteReader for DatasetReader {
    async fn read_quote(
        &self,
        id: &InstrumentId,
        as_of: Option<UnixNanos>,
    ) -> PortResult<Option<QuoteTick>> {
        let Some(slice) = self.dataset.slice(id) else {
            return Ok(None);
        };
        let stamps = slice.timestamps();
        let end = as_of.map_or(stamps.len(), |at| stamps.partition_point(|ts| *ts <= at));
        Ok(end
            .checked_sub(1)
            .and_then(|index| slice.bar(index))
            .map(|bar| {
                let close = bar.close();
                QuoteTick::new(
                    id.clone(),
                    close,
                    close,
                    0.0,
                    0.0,
                    bar.ts_event(),
                    bar.ts_event(),
                )
            }))
    }
}

/// A dataset of bars carries no order book, so depth is never available from it.
#[async_trait]
impl DepthReader for DatasetReader {
    async fn read_depth(&self, id: &InstrumentId, _levels: usize) -> PortResult<DepthSnapshot> {
        Err(PortError::Unsupported(format!(
            "{id} has bars only; the dataset holds no order book"
        )))
    }
}
