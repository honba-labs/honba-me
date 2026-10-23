//! A dated instrument catalog.
//!
//! [`CatalogInstrumentMaster`] reads a directory of annotated snapshots — one
//! Parquet file per date, named `YYYY-MM-DD.parquet` under
//! `{root}/instruments/` — and answers the [`InstrumentMaster`] port as of a
//! backtest date. The layout matches the `honba-data` catalog convention
//! (dated files under a per-entity directory) so a snapshot can be produced
//! beside the bar catalog, but the file format is defined here.
//!
//! Each row carries one instrument as it stood on the file's date: a record's
//! `effective_from` is that date and its `effective_to` is unbounded. A symbol
//! whose lot size changes, or a contract that expires, is represented by
//! successive snapshots rather than by editing history in place.
//!
//! # Snapshot columns
//!
//! `symbol`, `exchange`, `kind`, `currency`, `expiry`, `option_type` and
//! `underlying` are UTF8; `lot_size`, `tick_size`, `freeze_qty` and `strike`
//! are read as `f64` and accept any numeric Arrow type (real feeds may deliver
//! integers as doubles). `expiry`, `strike`, `option_type` and `underlying` are
//! nullable; `expiry` is a `YYYY-MM-DD` string.
//!
//! The reader is `Send + Sync` and does all I/O once, at [`open`](CatalogInstrumentMaster::open);
//! every answer is a pure function of the loaded snapshots.

use std::collections::BTreeMap;
use std::fs::File;
use std::path::Path;

use arrow::array::{Array, ArrayRef, Float64Array, StringArray};
use arrow::compute::cast;
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use async_trait::async_trait;
use chrono::NaiveDate;
use honba_core::{Currency, Exchange, InstrumentId, InstrumentKind, OptionType};

use crate::ports::{DatedInstrument, InstrumentMaster, PortError, PortResult};
use crate::wire::Instrument;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use thiserror::Error;

/// The directory under the catalog root that holds the dated snapshots.
pub const SNAPSHOT_DIR: &str = "instruments";

/// The column layout of a dated snapshot file, so producers and consumers agree.
pub fn snapshot_schema() -> Schema {
    Schema::new(vec![
        Field::new("symbol", DataType::Utf8, false),
        Field::new("exchange", DataType::Utf8, false),
        Field::new("kind", DataType::Utf8, false),
        Field::new("currency", DataType::Utf8, false),
        Field::new("lot_size", DataType::Float64, false),
        Field::new("tick_size", DataType::Float64, false),
        Field::new("freeze_qty", DataType::Float64, false),
        Field::new("expiry", DataType::Utf8, true),
        Field::new("strike", DataType::Float64, true),
        Field::new("option_type", DataType::Utf8, true),
        Field::new("underlying", DataType::Utf8, true),
    ])
}

/// Why a [`CatalogInstrumentMaster`] could not be opened or read.
#[derive(Debug, Error)]
pub enum CatalogError {
    /// The snapshot directory or one of its files could not be read.
    #[error("cannot read instrument catalog: {0}")]
    Io(#[from] std::io::Error),
    /// A `.parquet` file could not be decoded.
    #[error("parquet error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),
    /// An Arrow array could not be converted.
    #[error("arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),
    /// A snapshot file is not named `YYYY-MM-DD.parquet`.
    #[error("snapshot file name {0:?} is not YYYY-MM-DD.parquet")]
    FileName(String),
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
    /// A value could not be parsed as a `YYYY-MM-DD` date.
    #[error("invalid date {value:?} in column {column}")]
    BadDate {
        /// Name of the column.
        column: &'static str,
        /// The unparsable value.
        value: String,
    },
    /// A value is not a known member of its enum.
    #[error("unknown {column} {value:?}")]
    UnknownValue {
        /// Name of the column.
        column: &'static str,
        /// The unknown value.
        value: String,
    },
}

impl From<CatalogError> for PortError {
    fn from(error: CatalogError) -> Self {
        PortError::Internal(error.to_string())
    }
}

/// An [`InstrumentMaster`] over a directory of dated Parquet snapshots.
#[derive(Clone, Debug)]
pub struct CatalogInstrumentMaster {
    snapshots: BTreeMap<NaiveDate, Vec<DatedInstrument>>,
}

impl CatalogInstrumentMaster {
    /// Loads every `YYYY-MM-DD.parquet` snapshot under `{root}/instruments/`.
    ///
    /// This blocks on I/O once, at start-up. Files are read in name (date)
    /// order; a repeated date keeps the last file read.
    pub fn open(root: &Path) -> Result<Self, CatalogError> {
        let dir = root.join(SNAPSHOT_DIR);
        let mut paths = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|ext| ext == "parquet") {
                paths.push(path);
            }
        }
        paths.sort();
        let mut snapshots = BTreeMap::new();
        for path in paths {
            let date = snapshot_date(&path)?;
            snapshots.insert(date, read_snapshot(&path, date)?);
        }
        Ok(Self { snapshots })
    }

    /// The latest snapshot of `id` at or before `as_of`.
    fn latest_as_of(&self, id: &InstrumentId, as_of: NaiveDate) -> Option<&DatedInstrument> {
        self.snapshots
            .range(..=as_of)
            .rev()
            .flat_map(|(_, records)| records.iter())
            .find(|dated| dated.instrument.id() == id)
    }

    /// The latest snapshot of `id` on any date.
    fn latest(&self, id: &InstrumentId) -> Option<&DatedInstrument> {
        self.snapshots
            .values()
            .rev()
            .flat_map(|records| records.iter())
            .find(|dated| dated.instrument.id() == id)
    }

    /// One row per instrument: the newest snapshot at or before each date.
    fn merged(&self) -> BTreeMap<&InstrumentId, &DatedInstrument> {
        let mut merged: BTreeMap<&InstrumentId, &DatedInstrument> = BTreeMap::new();
        for records in self.snapshots.values() {
            for dated in records {
                merged.insert(dated.instrument.id(), dated);
            }
        }
        merged
    }

    /// Every in-force, non-expired record on `as_of`, in instrument-id order.
    fn in_force(&self, as_of: NaiveDate) -> Vec<DatedInstrument> {
        let mut merged: BTreeMap<&InstrumentId, &DatedInstrument> = BTreeMap::new();
        for (_, records) in self.snapshots.range(..=as_of) {
            for dated in records {
                merged.insert(dated.instrument.id(), dated);
            }
        }
        merged
            .into_values()
            .filter(|dated| !expired(dated, as_of))
            .cloned()
            .collect()
    }
}

#[async_trait]
impl InstrumentMaster for CatalogInstrumentMaster {
    async fn get_instrument(&self, id: &InstrumentId) -> PortResult<Option<Instrument>> {
        Ok(self.latest(id).map(|dated| dated.instrument.clone()))
    }

    async fn list_instruments(&self) -> PortResult<Vec<Instrument>> {
        Ok(self
            .merged()
            .into_values()
            .map(|dated| dated.instrument.clone())
            .collect())
    }

    async fn snapshot(&self, as_of: NaiveDate) -> PortResult<Vec<DatedInstrument>> {
        Ok(self.in_force(as_of))
    }

    async fn resolve_as_of(
        &self,
        id: &InstrumentId,
        as_of: NaiveDate,
    ) -> PortResult<Option<Instrument>> {
        Ok(self.latest_as_of(id, as_of).and_then(|dated| {
            if expired(dated, as_of) {
                None
            } else {
                Some(dated.instrument.clone())
            }
        }))
    }
}

/// True when the record's contract expired strictly before `as_of`; a contract
/// is still tradable on its expiry date.
fn expired(dated: &DatedInstrument, as_of: NaiveDate) -> bool {
    dated
        .instrument
        .expiry()
        .is_some_and(|expiry| expiry < as_of)
}

/// Parses the `YYYY-MM-DD` stem of a snapshot path.
fn snapshot_date(path: &Path) -> Result<NaiveDate, CatalogError> {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    NaiveDate::parse_from_str(stem, "%Y-%m-%d").map_err(|_| CatalogError::FileName(file_name(path)))
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn read_snapshot(path: &Path, date: NaiveDate) -> Result<Vec<DatedInstrument>, CatalogError> {
    let file = File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let reader = builder.build()?;
    let mut records = Vec::new();
    for batch in reader {
        append_batch(&batch?, date, &mut records)?;
    }
    Ok(records)
}

fn append_batch(
    batch: &RecordBatch,
    date: NaiveDate,
    records: &mut Vec<DatedInstrument>,
) -> Result<(), CatalogError> {
    let schema = batch.schema();
    let column = |name: &'static str| -> Result<ArrayRef, CatalogError> {
        let index = schema
            .index_of(name)
            .map_err(|_| CatalogError::MissingColumn(name))?;
        Ok(batch.column(index).clone())
    };
    let utf8 = |name: &'static str| -> Result<ArrayRef, CatalogError> {
        as_utf8(column(name)?.as_ref(), name)
    };
    let number = |name: &'static str| -> Result<ArrayRef, CatalogError> {
        as_f64(column(name)?.as_ref(), name)
    };

    let symbols = downcast_string(utf8("symbol")?, "symbol");
    let exchanges = downcast_string(utf8("exchange")?, "exchange");
    let kinds = downcast_string(utf8("kind")?, "kind");
    let currencies = downcast_string(utf8("currency")?, "currency");
    let lot_sizes = downcast_f64(number("lot_size")?, "lot_size");
    let tick_sizes = downcast_f64(number("tick_size")?, "tick_size");
    let freeze_qtys = downcast_f64(number("freeze_qty")?, "freeze_qty");
    let expiries = downcast_string(utf8("expiry")?, "expiry");
    let strikes = downcast_f64(number("strike")?, "strike");
    let option_types = downcast_string(utf8("option_type")?, "option_type");
    let underlyings = downcast_string(utf8("underlying")?, "underlying");

    for row in 0..batch.num_rows() {
        let expiry = optional(&expiries, row, |value| parse_date(value, "expiry"))?;
        let option_type = optional(&option_types, row, parse_option_type)?;
        let instrument = Instrument::new(
            InstrumentId::new(
                parse_exchange(exchanges.value(row))?,
                symbols.value(row),
            ),
            parse_kind(kinds.value(row))?,
            parse_currency(currencies.value(row))?,
            lot_sizes.value(row),
            tick_sizes.value(row),
        )
        .with_derivative(
            expiry,
            optional_f64(&strikes, row),
            option_type,
            optional_string(&underlyings, row),
        );
        records.push(DatedInstrument {
            instrument,
            freeze_qty: freeze_qtys.value(row),
            effective_from: date,
            effective_to: None,
        });
    }
    Ok(())
}

fn as_utf8(array: &dyn Array, name: &'static str) -> Result<ArrayRef, CatalogError> {
    match array.data_type() {
        DataType::Utf8 | DataType::LargeUtf8 => Ok(cast(array, &DataType::Utf8)?),
        got => Err(CatalogError::WrongType {
            name,
            got: got.clone(),
        }),
    }
}

fn as_f64(array: &dyn Array, name: &'static str) -> Result<ArrayRef, CatalogError> {
    if !array.data_type().is_numeric() {
        return Err(CatalogError::WrongType {
            name,
            got: array.data_type().clone(),
        });
    }
    Ok(cast(array, &DataType::Float64)?)
}

fn downcast_string(array: ArrayRef, _name: &'static str) -> StringArray {
    array
        .as_any()
        .downcast_ref::<StringArray>()
        .expect("as_utf8 casts to Utf8")
        .clone()
}

fn downcast_f64(array: ArrayRef, _name: &'static str) -> Float64Array {
    array
        .as_any()
        .downcast_ref::<Float64Array>()
        .expect("as_f64 casts to Float64")
        .clone()
}

fn optional_string(array: &StringArray, row: usize) -> Option<String> {
    (!array.is_null(row)).then(|| array.value(row).to_string())
}

fn optional_f64(array: &Float64Array, row: usize) -> Option<f64> {
    (!array.is_null(row)).then(|| array.value(row))
}

fn optional<T>(
    array: &StringArray,
    row: usize,
    parse: impl Fn(&str) -> Result<T, CatalogError>,
) -> Result<Option<T>, CatalogError> {
    if array.is_null(row) {
        Ok(None)
    } else {
        parse(array.value(row)).map(Some)
    }
}

fn parse_date(value: &str, column: &'static str) -> Result<NaiveDate, CatalogError> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| CatalogError::BadDate {
        column,
        value: value.to_string(),
    })
}

fn parse_kind(value: &str) -> Result<InstrumentKind, CatalogError> {
    from_json(value, "kind")
}

fn parse_currency(value: &str) -> Result<Currency, CatalogError> {
    // `Currency`'s derived (de)serialization expects its Rust spelling
    // ("Inr"), not the stored ISO code ("INR"); `from_code` is the
    // case-insensitive ISO-4217 parser `honba_core` provides for this.
    Currency::from_code(value).map_err(|_| CatalogError::UnknownValue {
        column: "currency",
        value: value.to_string(),
    })
}

fn parse_option_type(value: &str) -> Result<OptionType, CatalogError> {
    // NSE/BSE F&O conventions spell these "CE"/"PE", not `OptionType`'s
    // derived snake_case ("call"/"put"); try the exchange alias first.
    match value.to_uppercase().as_str() {
        "CE" => Ok(OptionType::Call),
        "PE" => Ok(OptionType::Put),
        _ => from_json(value, "option_type"),
    }
}

fn parse_exchange(value: &str) -> Result<Exchange, CatalogError> {
    from_json(value, "exchange")
}

/// Deserializes an enum from its wire spelling, reusing the entity's serde
/// rules (snake_case kinds, `UPPERCASE` currencies, `CE`/`PE` option aliases).
fn from_json<T: serde::de::DeserializeOwned>(
    value: &str,
    column: &'static str,
) -> Result<T, CatalogError> {
    serde_json::from_value(serde_json::Value::String(value.to_string())).map_err(|_| {
        CatalogError::UnknownValue {
            column,
            value: value.to_string(),
        }
    })
}
