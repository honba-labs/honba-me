//! A Parquet-backed tick store: the persistent half of the quote/trade tick input an
//! `fill_model = l2` run matches against (ADR 0030, "no parquet tick store yet").
//!
//! [`ParquetTickStore`] reads a directory of tick files — `{root}/ticks/*.parquet`, read in
//! file-name order and merged — and answers the [`TickReader`] port as of a half-open
//! `[from, to)` range. The layout leaves room for a later date- or instrument-partitioned
//! directory; today every file under `ticks/` is read.
//!
//! [`write_tick_file`] is the producer: it lays out the quotes and trades it is given (quotes
//! first, each kind in input order) as one Parquet file. The reader sorts by `ts_event` (stable
//! for ties), so the writer's order only decides ties.
//!
//! # Tick columns
//!
//! Every row is one tick tagged by `kind` (`quote` or `trade`). `symbol`, `exchange` and
//! `kind` are UTF8; `ts_event` and `ts_init` are unix nanoseconds as `Int64` (`ts_init` is
//! nullable and defaults to `ts_event`); `bid_price`, `ask_price`, `bid_size`, `ask_size`,
//! `price` and `size` are `Float64`; `aggressor` (one of `buyer`, `seller`, `no_aggressor`)
//! and `trade_id` are UTF8. The columns a row's kind does not use are null. A quote row must
//! carry the four quote prices/sizes; a trade row its price, size, aggressor and trade id.
//!
//! The reader is `Send + Sync` and does all I/O once, at [`open`](ParquetTickStore::open), like
//! [`CatalogInstrumentMaster`](crate::CatalogInstrumentMaster); every answer is a pure
//! function of the loaded ticks. A store with no files, or files with no ticks, answers
//! [`PortError::Unsupported`] rather than inventing an empty series.

use std::fs::File;
use std::path::Path;
use std::str::FromStr;

use arrow::array::{Array, ArrayRef, Float64Array, Int64Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use async_trait::async_trait;
use honba_core::{Exchange, InstrumentId};

use crate::ports::{PortError, PortResult, TickReader, TickRequest};
use crate::wire::{AggressorSide, QuoteTick, TradeId, TradeTick, UnixNanos};
use parquet::arrow::ArrowWriter;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use thiserror::Error;

/// The directory under a store root that holds the tick Parquet files.
pub const TICK_DIR: &str = "ticks";

/// The column layout of a tick file, so producers and consumers agree.
pub fn tick_schema() -> Schema {
    Schema::new(vec![
        Field::new("kind", DataType::Utf8, false),
        Field::new("symbol", DataType::Utf8, false),
        Field::new("exchange", DataType::Utf8, false),
        Field::new("ts_event", DataType::Int64, false),
        Field::new("ts_init", DataType::Int64, true),
        Field::new("bid_price", DataType::Float64, true),
        Field::new("ask_price", DataType::Float64, true),
        Field::new("bid_size", DataType::Float64, true),
        Field::new("ask_size", DataType::Float64, true),
        Field::new("price", DataType::Float64, true),
        Field::new("size", DataType::Float64, true),
        Field::new("aggressor", DataType::Utf8, true),
        Field::new("trade_id", DataType::Utf8, true),
    ])
}

/// Why a tick store could not be opened, written or read.
#[derive(Debug, Error)]
pub enum TickStoreError {
    /// The store directory or one of its files could not be read or written.
    #[error("cannot access tick store: {0}")]
    Io(#[from] std::io::Error),
    /// A `.parquet` file could not be decoded.
    #[error("parquet error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),
    /// An Arrow array or batch could not be built.
    #[error("arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),
    /// A required column is missing.
    #[error("missing column: {0}")]
    MissingColumn(&'static str),
    /// A column had an unexpected data type.
    #[error("column {name} has type {got:?}, which is not usable here")]
    WrongType {
        /// Name of the column.
        name: &'static str,
        /// Actual data type found.
        got: DataType,
    },
    /// A row is not a valid tick.
    #[error("{file}: row {row}: {message}")]
    BadRow {
        /// Name of the file the row came from.
        file: String,
        /// Zero-based row index within that file.
        row: usize,
        /// Why the row is not a tick.
        message: String,
    },
}

impl From<TickStoreError> for PortError {
    fn from(error: TickStoreError) -> Self {
        PortError::Internal(error.to_string())
    }
}

/// An in-memory view of a directory of tick Parquet files (ADR 0030).
#[derive(Clone, Debug, Default)]
pub struct ParquetTickStore {
    quotes: Vec<QuoteTick>,
    trades: Vec<TradeTick>,
}

impl ParquetTickStore {
    /// Loads every `*.parquet` file under `{root}/ticks/`.
    ///
    /// This blocks on I/O once, at start-up. Files are read in name order; ticks from every
    /// file are merged and sorted by `ts_event` (stable, so a tie keeps file and row order).
    /// A missing store directory is an error; a present but empty store answers
    /// [`PortError::Unsupported`] on every read.
    pub fn open(root: &Path) -> Result<Self, TickStoreError> {
        let dir = root.join(TICK_DIR);
        let mut paths = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|ext| ext == "parquet") {
                paths.push(path);
            }
        }
        paths.sort();
        let mut quotes = Vec::new();
        let mut trades = Vec::new();
        for path in &paths {
            read_file(path, &mut quotes, &mut trades)?;
        }
        quotes.sort_by_key(QuoteTick::ts_event);
        trades.sort_by_key(TradeTick::ts_event);
        Ok(Self { quotes, trades })
    }

    /// The quotes and trades loaded, for tests and inspection.
    pub fn len(&self) -> (usize, usize) {
        (self.quotes.len(), self.trades.len())
    }

    /// Borrowed quotes and trades, for the canonical bridge in [`crate::canonical`].
    pub(crate) fn raw(&self) -> (&[QuoteTick], &[TradeTick]) {
        (&self.quotes, &self.trades)
    }

    /// Returns `true` when the store holds no ticks at all.
    pub fn is_empty(&self) -> bool {
        self.quotes.is_empty() && self.trades.is_empty()
    }

    fn check_held(&self) -> PortResult<()> {
        if self.is_empty() {
            return Err(PortError::Unsupported(
                "this store holds no tick data".into(),
            ));
        }
        Ok(())
    }
}

fn in_range(t: UnixNanos, request: &TickRequest) -> bool {
    request.from().is_none_or(|f| t >= f) && request.to().is_none_or(|e| t < e)
}

#[async_trait]
impl TickReader for ParquetTickStore {
    async fn read_quotes(&self, request: &TickRequest) -> PortResult<Vec<QuoteTick>> {
        self.check_held()?;
        Ok(self
            .quotes
            .iter()
            .filter(|q| {
                q.instrument_id() == request.instrument() && in_range(q.ts_event(), request)
            })
            .cloned()
            .collect())
    }

    async fn read_trades(&self, request: &TickRequest) -> PortResult<Vec<TradeTick>> {
        self.check_held()?;
        Ok(self
            .trades
            .iter()
            .filter(|t| {
                t.instrument_id() == request.instrument() && in_range(t.ts_event(), request)
            })
            .cloned()
            .collect())
    }
}

/// Writes `quotes` then `trades`, in the order given, as one tick file at `path`.
///
/// Creates the parent directory if needed and overwrites an existing file. The reader sorts by
/// `ts_event`, so the order here only decides ties.
pub fn write_tick_file(
    path: &Path,
    quotes: &[QuoteTick],
    trades: &[TradeTick],
) -> Result<(), TickStoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut kinds = Vec::with_capacity(quotes.len() + trades.len());
    let mut symbols = Vec::with_capacity(quotes.len() + trades.len());
    let mut exchanges = Vec::with_capacity(quotes.len() + trades.len());
    let mut ts_events = Vec::with_capacity(quotes.len() + trades.len());
    let mut ts_inits = Vec::with_capacity(quotes.len() + trades.len());
    let mut bid_prices = Vec::with_capacity(quotes.len() + trades.len());
    let mut ask_prices = Vec::with_capacity(quotes.len() + trades.len());
    let mut bid_sizes = Vec::with_capacity(quotes.len() + trades.len());
    let mut ask_sizes = Vec::with_capacity(quotes.len() + trades.len());
    let mut prices = Vec::with_capacity(quotes.len() + trades.len());
    let mut sizes = Vec::with_capacity(quotes.len() + trades.len());
    let mut aggressors = Vec::with_capacity(quotes.len() + trades.len());
    let mut trade_ids = Vec::with_capacity(quotes.len() + trades.len());

    for q in quotes {
        kinds.push("quote");
        symbols.push(q.instrument_id().symbol.clone());
        exchanges.push(exchange_str(q.instrument_id().exchange));
        ts_events.push(q.ts_event().as_u64() as i64);
        ts_inits.push(Some(q.ts_init().as_u64() as i64));
        bid_prices.push(Some(q.bid_price()));
        ask_prices.push(Some(q.ask_price()));
        bid_sizes.push(Some(q.bid_size()));
        ask_sizes.push(Some(q.ask_size()));
        prices.push(None);
        sizes.push(None);
        aggressors.push(None);
        trade_ids.push(None);
    }
    for t in trades {
        kinds.push("trade");
        symbols.push(t.instrument_id().symbol.clone());
        exchanges.push(exchange_str(t.instrument_id().exchange));
        ts_events.push(t.ts_event().as_u64() as i64);
        ts_inits.push(Some(t.ts_init().as_u64() as i64));
        bid_prices.push(None);
        ask_prices.push(None);
        bid_sizes.push(None);
        ask_sizes.push(None);
        prices.push(Some(t.price()));
        sizes.push(Some(t.size()));
        aggressors.push(Some(aggressor_name(t.aggressor_side())));
        trade_ids.push(Some(t.trade_id().as_str()));
    }

    let ints = |values: Vec<i64>| -> ArrayRef { std::sync::Arc::new(Int64Array::from(values)) };
    let opt_ints =
        |values: Vec<Option<i64>>| -> ArrayRef { std::sync::Arc::new(Int64Array::from(values)) };
    let floats =
        |values: Vec<Option<f64>>| -> ArrayRef { std::sync::Arc::new(Float64Array::from(values)) };
    let strings =
        |values: Vec<&str>| -> ArrayRef { std::sync::Arc::new(StringArray::from(values)) };
    let owned_strings =
        |values: Vec<String>| -> ArrayRef { std::sync::Arc::new(StringArray::from_iter_values(values)) };
    let opt_strings =
        |values: Vec<Option<&str>>| -> ArrayRef { std::sync::Arc::new(StringArray::from(values)) };
    let opt_owned = |values: Vec<Option<String>>| -> ArrayRef {
        std::sync::Arc::new(StringArray::from_iter(values))
    };

    let columns: Vec<ArrayRef> = vec![
        strings(kinds),
        owned_strings(symbols),
        strings(exchanges),
        ints(ts_events),
        opt_ints(ts_inits),
        floats(bid_prices),
        floats(ask_prices),
        floats(bid_sizes),
        floats(ask_sizes),
        floats(prices),
        floats(sizes),
        opt_owned(aggressors),
        opt_strings(trade_ids),
    ];
    let schema = std::sync::Arc::new(tick_schema());
    let batch = RecordBatch::try_new(schema.clone(), columns)?;
    let mut writer = ArrowWriter::try_new(File::create(path)?, schema, None)?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}

/// The stored spelling of an exchange (`NSE`/`BSE`/`MCX`), via `Display`.
///
/// `honba_core::Exchange` has only three variants, so a `&'static str` per
/// variant avoids an allocation in the hot write path.
fn exchange_str(exchange: Exchange) -> &'static str {
    match exchange {
        Exchange::Nse => "NSE",
        Exchange::Bse => "BSE",
        Exchange::Mcx => "MCX",
    }
}

/// The stored spelling of an aggressor side: its serde name, so the column and the wire agree
/// and a new variant stored by a later build is still readable text.
fn aggressor_name(side: AggressorSide) -> String {
    serde_json::to_value(side)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "no_aggressor".to_string())
}

/// Parses the stored aggressor spelling through the wire type, so a name this build does not
/// know is `None` rather than silently coerced.
fn parse_aggressor(value: &str) -> Option<AggressorSide> {
    serde_json::from_value(serde_json::Value::String(value.to_string())).ok()
}

fn read_file(
    path: &Path,
    quotes: &mut Vec<QuoteTick>,
    trades: &mut Vec<TradeTick>,
) -> Result<(), TickStoreError> {
    let file = File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let reader = builder.build()?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    for batch in reader {
        append_batch(&batch?, &name, quotes, trades)?;
    }
    Ok(())
}

fn append_batch(
    batch: &RecordBatch,
    file: &str,
    quotes: &mut Vec<QuoteTick>,
    trades: &mut Vec<TradeTick>,
) -> Result<(), TickStoreError> {
    let schema = batch.schema();
    let column = |name: &'static str| -> Result<ArrayRef, TickStoreError> {
        let index = schema
            .index_of(name)
            .map_err(|_| TickStoreError::MissingColumn(name))?;
        Ok(batch.column(index).clone())
    };
    let utf8 = |name: &'static str| -> Result<StringArray, TickStoreError> {
        let array = column(name)?;
        match array.data_type() {
            DataType::Utf8 => Ok(array
                .as_any()
                .downcast_ref::<StringArray>()
                .expect("checked Utf8")
                .clone()),
            got => Err(TickStoreError::WrongType {
                name,
                got: got.clone(),
            }),
        }
    };
    let nanos = |name: &'static str| -> Result<Int64Array, TickStoreError> {
        let array = column(name)?;
        match array.data_type() {
            DataType::Int64 => Ok(array
                .as_any()
                .downcast_ref::<Int64Array>()
                .expect("checked Int64")
                .clone()),
            got => Err(TickStoreError::WrongType {
                name,
                got: got.clone(),
            }),
        }
    };
    let floats = |name: &'static str| -> Result<Float64Array, TickStoreError> {
        let array = column(name)?;
        match array.data_type() {
            DataType::Float64 => Ok(array
                .as_any()
                .downcast_ref::<Float64Array>()
                .expect("checked Float64")
                .clone()),
            got => Err(TickStoreError::WrongType {
                name,
                got: got.clone(),
            }),
        }
    };

    let kinds = utf8("kind")?;
    let symbols = utf8("symbol")?;
    let exchanges = utf8("exchange")?;
    let ts_events = nanos("ts_event")?;
    let ts_inits = nanos("ts_init")?;
    let bid_prices = floats("bid_price")?;
    let ask_prices = floats("ask_price")?;
    let bid_sizes = floats("bid_size")?;
    let ask_sizes = floats("ask_size")?;
    let prices = floats("price")?;
    let sizes = floats("size")?;
    let aggressors = utf8("aggressor")?;
    let trade_ids = utf8("trade_id")?;

    let bad = |row: usize, message: String| TickStoreError::BadRow {
        file: file.to_string(),
        row,
        message,
    };
    let required_f64 =
        |array: &Float64Array, row: usize, name: &str| -> Result<f64, TickStoreError> {
            if array.is_null(row) {
                Err(bad(row, format!("{name} is null on a tick that needs it")))
            } else {
                Ok(array.value(row))
            }
        };

    for row in 0..batch.num_rows() {
        let exchange = Exchange::from_str(exchanges.value(row)).map_err(|_| {
            bad(
                row,
                format!("{} is not a known exchange", exchanges.value(row)),
            )
        })?;
        let instrument = InstrumentId::new(exchange, symbols.value(row).to_string());
        let wire_instrument = serde_json::to_value(&instrument).expect("an instrument serializes");
        let ts_event = UnixNanos::from_u64(ts_events.value(row) as u64);
        let ts_init = if ts_inits.is_null(row) {
            ts_event
        } else {
            UnixNanos::from_u64(ts_inits.value(row) as u64)
        };
        let wire_event = serde_json::to_value(ts_event).expect("a timestamp serializes");
        let wire_init = serde_json::to_value(ts_init).expect("a timestamp serializes");
        match kinds.value(row) {
            // Through the wire type, like the NDJSON loader, so its invariants are checked once
            // in `honba-messages` and a bad row is an error, never a panic.
            "quote" => {
                let value = serde_json::json!({
                    "instrument_id": wire_instrument,
                    "bid_price": required_f64(&bid_prices, row, "bid_price")?,
                    "ask_price": required_f64(&ask_prices, row, "ask_price")?,
                    "bid_size": required_f64(&bid_sizes, row, "bid_size")?,
                    "ask_size": required_f64(&ask_sizes, row, "ask_size")?,
                    "ts_event": wire_event,
                    "ts_init": wire_init,
                });
                let tick: QuoteTick = serde_json::from_value(value)
                    .map_err(|e| bad(row, format!("invalid quote: {e}")))?;
                quotes.push(tick);
            }
            "trade" => {
                let aggressor = if aggressors.is_null(row) {
                    return Err(bad(row, "aggressor is null on a trade".to_string()));
                } else {
                    parse_aggressor(aggressors.value(row)).ok_or_else(|| {
                        bad(
                            row,
                            format!("unknown aggressor {:?}", aggressors.value(row)),
                        )
                    })?
                };
                let trade_id = if trade_ids.is_null(row) {
                    return Err(bad(row, "trade_id is null on a trade".to_string()));
                } else {
                    TradeId::new(trade_ids.value(row))
                };
                let value = serde_json::json!({
                    "instrument_id": wire_instrument,
                    "price": required_f64(&prices, row, "price")?,
                    "size": required_f64(&sizes, row, "size")?,
                    "aggressor_side": serde_json::to_value(aggressor).expect("an aggressor serializes"),
                    "trade_id": trade_id.as_str(),
                    "ts_event": wire_event,
                    "ts_init": wire_init,
                });
                let tick: TradeTick = serde_json::from_value(value)
                    .map_err(|e| bad(row, format!("invalid trade: {e}")))?;
                trades.push(tick);
            }
            other => return Err(bad(row, format!("unknown kind {other:?}"))),
        }
    }
    Ok(())
}
