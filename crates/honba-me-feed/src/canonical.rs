//! Bridge from the legacy `f64` wire types to canonical [`honba_core`] types.
//!
//! The legacy readers ([`ParquetTickStore`], [`DatasetReader`](crate::DatasetReader)) still speak
//! the `f64`-based [`crate::wire`] vocabulary because their on-disk files predate the
//! lakehouse. These helpers convert their output into exact-decimal [`Tick`] / [`Bar`] so the
//! rest of the stack — and [`Lakehouse::write_ticks`] / [`Lakehouse::write_bars`] — only ever
//! sees canonical types, and old data can be migrated.
//!
//! `f64` → `Decimal` goes through the shortest round-trip decimal string (so `1500.1` becomes
//! exactly `1500.1`, not `1500.0999999999999`). Non-finite values yield `None`.

use std::path::Path;
use std::str::FromStr;

use honba_core::rust_decimal::Decimal;
use honba_core::{Bar, Price, Quantity, Tick, Timeframe};

use crate::lakehouse::{Lakehouse, LakehouseError};
use crate::parquet_ticks::{ParquetTickStore, TickStoreError};
use crate::wire::{self, QuoteTick, TradeTick};

/// Exact decimal for a finite `f64`, or `None` for NaN / infinity.
pub fn decimal_from_f64(v: f64) -> Option<Decimal> {
    if !v.is_finite() {
        return None;
    }
    let s = format!("{v}");
    Decimal::from_str(&s)
        .or_else(|_| Decimal::from_scientific(&s))
        .ok()
}

/// Converts a legacy bar to a canonical [`Bar`] (timestamp = `ts_event`).
pub fn bar_to_canonical(bar: &wire::Bar) -> Option<Bar> {
    Some(Bar {
        instrument: bar.bar_type().instrument_id().clone(),
        open: Price(decimal_from_f64(bar.open())?),
        high: Price(decimal_from_f64(bar.high())?),
        low: Price(decimal_from_f64(bar.low())?),
        close: Price(decimal_from_f64(bar.close())?),
        volume: Quantity(decimal_from_f64(bar.volume())?),
        timestamp: bar.ts_event().to_datetime(),
    })
}

/// Converts legacy bars, skipping any with a non-finite field.
pub fn bars_to_canonical(bars: &[wire::Bar]) -> Vec<Bar> {
    bars.iter().filter_map(bar_to_canonical).collect()
}

/// Converts a legacy quote to a canonical [`Tick::Quote`] (no last price, no band).
pub fn quote_to_canonical(q: &QuoteTick) -> Option<Tick> {
    Some(Tick::Quote {
        instrument: q.instrument_id().clone(),
        bid: Price(decimal_from_f64(q.bid_price())?),
        ask: Price(decimal_from_f64(q.ask_price())?),
        bid_qty: Quantity(decimal_from_f64(q.bid_size())?),
        ask_qty: Quantity(decimal_from_f64(q.ask_size())?),
        last_price: None,
        exchange_ts: q.ts_event().to_datetime(),
        receive_ts: q.ts_init().to_datetime(),
        band: None,
    })
}

/// Converts a legacy trade to a canonical [`Tick::Ltp`] (`volume` = trade size).
pub fn trade_to_canonical(t: &TradeTick) -> Option<Tick> {
    Some(Tick::Ltp {
        instrument: t.instrument_id().clone(),
        price: Price(decimal_from_f64(t.price())?),
        volume: Quantity(decimal_from_f64(t.size())?),
        exchange_ts: t.ts_event().to_datetime(),
        receive_ts: t.ts_init().to_datetime(),
    })
}

impl ParquetTickStore {
    /// Every stored quote and trade as canonical [`Tick`]s, merged in exchange-time order.
    ///
    /// Rows with a non-finite value are skipped.
    pub fn canonical_ticks(&self) -> Vec<Tick> {
        let (quotes, trades) = self.raw();
        let mut out: Vec<Tick> = quotes
            .iter()
            .filter_map(quote_to_canonical)
            .chain(trades.iter().filter_map(trade_to_canonical))
            .collect();
        out.sort_by_key(|t| match t {
            Tick::Ltp { exchange_ts, .. }
            | Tick::Quote { exchange_ts, .. }
            | Tick::Depth { exchange_ts, .. } => *exchange_ts,
        });
        out
    }
}

/// Why a legacy → lakehouse migration failed.
#[derive(Debug, thiserror::Error)]
pub enum MigrateError {
    /// The legacy store could not be opened.
    #[error(transparent)]
    Legacy(#[from] TickStoreError),
    /// The lakehouse write failed.
    #[error(transparent)]
    Lakehouse(#[from] LakehouseError),
}

/// Migrates a legacy `{legacy_root}/ticks/*.parquet` store into `lakehouse`.
///
/// Returns the number of ticks migrated.
pub fn migrate_tick_store(
    legacy_root: &Path,
    lakehouse: &Lakehouse,
) -> Result<usize, MigrateError> {
    let ticks = ParquetTickStore::open(legacy_root)?.canonical_ticks();
    lakehouse.write_ticks(&ticks)?;
    Ok(ticks.len())
}

/// Writes canonical conversions of legacy bars into the lakehouse. Returns the bars written.
pub fn migrate_bars(
    lakehouse: &Lakehouse,
    timeframe: Timeframe,
    bars: &[wire::Bar],
) -> Result<usize, LakehouseError> {
    let canonical = bars_to_canonical(bars);
    lakehouse.write_bars(timeframe, &canonical)?;
    Ok(canonical.len())
}
