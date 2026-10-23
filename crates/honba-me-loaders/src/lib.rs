//! Data loaders for Honba market data.
//!
//! * [`bhavcopy`] — NSE daily Bhavcopy (`sec_bhavdata_full`) and delivery-position (`MTO`)
//!   downloaders and parsers.
//! * [`candles`] — Kite and Upstox REST historical-candle loaders behind the
//!   [`HistoricalBarSource`] trait.
//! * [`gaps`] — detection of missing bars in the local [`honba_feed::Lakehouse`] and idempotent
//!   backfill from a broker.
//! * [`duckdb`] — generated DuckDB views over the Parquet lakehouse.
//!
//! Everything produces canonical `honba_core` types with exact decimals. Parsing and gap
//! detection are pure functions; only the `download_*` methods and the `reconcile_*` functions
//! perform I/O.

pub mod bhavcopy;
pub mod candles;
pub mod duckdb;
pub mod gaps;

use chrono::FixedOffset;
use thiserror::Error;

pub use bhavcopy::{
    BhavRecord, BhavcopyDownloader, DeliveryRecord, bars_of, merge_delivery, parse_bhavcopy_csv,
    parse_delivery_dat,
};
pub use candles::{HistoricalBarSource, KiteCandlesLoader, UpstoxCandlesLoader, parse_candles};
pub use duckdb::DuckDbViews;
pub use gaps::{
    GapRange, ReconcileReport, expected_session_bars, missing_intraday_bars, missing_trading_days,
    reconcile_daily, reconcile_intraday,
};

/// Errors from data loaders.
#[derive(Debug, Error)]
pub enum LoaderError {
    /// HTTP request failed.
    #[error("HTTP error: {0}")]
    Http(String),
    /// CSV parsing failed.
    #[error("CSV parse error: {0}")]
    Csv(String),
    /// JSON parsing failed.
    #[error("JSON parse error: {0}")]
    Json(String),
    /// Remote file / instrument not found (e.g. no Bhavcopy on a holiday).
    #[error("not found: {0}")]
    NotFound(String),
    /// The broker or request shape does not support the operation.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// File IO error.
    #[error("IO error: {0}")]
    Io(String),
    /// Lakehouse read/write error.
    #[error("lakehouse error: {0}")]
    Lakehouse(String),
}

/// Result type for data loaders.
pub type LoaderResult<T> = Result<T, LoaderError>;

/// `Asia/Kolkata` (UTC+05:30), the exchange time zone.
pub(crate) fn ist() -> FixedOffset {
    FixedOffset::east_opt(5 * 3600 + 30 * 60).expect("valid IST offset")
}
