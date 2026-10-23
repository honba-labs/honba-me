//! Local market-data wire types.
//!
//! These used to come from the deleted `honba-kokoro-{entities,messages}`
//! crates. `honba_core` now owns the canonical, `Decimal`-based `Bar`/`Tick`
//! and sync port traits, but this crate's columnar engine and Parquet
//! readers predate that redesign: they work in `f64` OHLCV columns and a
//! dual `ts_event`/`ts_init` timestamp (event time vs. processing time,
//! needed for backtest/live determinism), which `honba_core::Bar` (single
//! `Decimal` timestamp) does not model. Porting this crate's numeric engine
//! onto `Decimal` and a single timestamp is a separate, much larger
//! migration; until then these mirror the original shapes exactly, kept
//! local to this crate instead of shared.
//!
//! `InstrumentId`, `Exchange`, `Currency`, `DatasetId`, `InstrumentKind` and
//! `OptionType` are the canonical `honba_core` ones — those shapes matched
//! and are reused directly. `UnixNanos` is the one exception: see below.

use honba_core::InstrumentId;
use serde::{Deserialize, Serialize};

/// Orders instruments by exchange, then symbol.
///
/// Delegates to `honba_core::InstrumentId`'s derived `Ord` (order:
/// `Exchange::Nse < Bse < Mcx`, then `symbol`, then `token`, then `isin`).
/// This provides a total order consistent with the core crate.
pub fn instrument_cmp(a: &InstrumentId, b: &InstrumentId) -> std::cmp::Ordering {
    a.cmp(b)
}

/// A nanosecond-resolution timestamp, counted from the Unix epoch.
///
/// `honba_core` models time as `chrono::DateTime<Utc>`, which is what the
/// domain crates use. This crate's columnar engine is built around a dense
/// `u64` nanosecond column and a dual `ts_event`/`ts_init` pair (event time
/// vs. processing time) that a single `DateTime` field cannot express, so the
/// timestamp is kept as a plain `u64` count of nanoseconds and converted at
/// the chrono boundary.
///
/// It serializes as a bare `u64`, which is the on-disk representation the
/// Parquet catalog and the JSON exports already use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnixNanos(u64);

impl UnixNanos {
    /// Creates a timestamp from a nanosecond count since the Unix epoch.
    pub const fn from_u64(nanos: u64) -> Self {
        Self(nanos)
    }

    /// The timestamp as a nanosecond count since the Unix epoch.
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Creates a timestamp from a UTC date-time.
    pub fn from_datetime(ts: chrono::DateTime<chrono::Utc>) -> Self {
        // Use i128 for the nanosecond computation to avoid overflow/wrap
        // on pre-1970 timestamps; then saturate into u64::MAX.
        let total_nanos =
            (ts.timestamp() as i128 * 1_000_000_000) + i128::from(ts.timestamp_subsec_nanos());
        let clamped = total_nanos.clamp(0, u64::MAX as i128);
        Self(clamped as u64)
    }

    /// The timestamp as a UTC date-time.
    pub fn to_datetime(self) -> chrono::DateTime<chrono::Utc> {
        let secs = (self.0 / 1_000_000_000) as i64;
        let nanos = u32::try_from(self.0 % 1_000_000_000).unwrap_or(0);
        chrono::DateTime::from_timestamp(secs, nanos).unwrap_or(chrono::DateTime::UNIX_EPOCH)
    }
}

impl std::fmt::Display for UnixNanos {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for UnixNanos {
    fn from(nanos: u64) -> Self {
        Self::from_u64(nanos)
    }
}

impl From<UnixNanos> for u64 {
    fn from(ts: UnixNanos) -> u64 {
        ts.as_u64()
    }
}

/// Which price a bar's OHLC is built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PriceType {
    /// Last traded price.
    Last,
    /// Best bid.
    Bid,
    /// Best ask.
    Ask,
    /// Bid/ask midpoint.
    Mid,
}

/// The unit a bar's `step` counts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BarAggregation {
    /// Second bars.
    Second,
    /// Minute bars.
    Minute,
    /// Hour bars.
    Hour,
    /// Day bars.
    Day,
    /// Week bars.
    Week,
    /// Month bars.
    Month,
}

/// How a bar aggregates its inputs: `step` units of `aggregation`, built from `price_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BarSpecification {
    step: u32,
    aggregation: BarAggregation,
    price_type: PriceType,
}

impl BarSpecification {
    /// Creates a bar specification.
    pub fn new(step: u32, aggregation: BarAggregation, price_type: PriceType) -> Self {
        Self {
            step,
            aggregation,
            price_type,
        }
    }

    /// The step count (e.g. `5` in "5-minute bars").
    pub fn step(&self) -> u32 {
        self.step
    }

    /// The aggregation unit.
    pub fn aggregation(&self) -> BarAggregation {
        self.aggregation
    }

    /// The price the OHLC is built from.
    pub fn price_type(&self) -> PriceType {
        self.price_type
    }
}

/// An instrument paired with the bar specification its bars are built at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BarType {
    instrument_id: InstrumentId,
    spec: BarSpecification,
}

impl BarType {
    /// Creates a bar type.
    pub fn new(instrument_id: InstrumentId, spec: BarSpecification) -> Self {
        Self {
            instrument_id,
            spec,
        }
    }

    /// The instrument these bars belong to.
    pub fn instrument_id(&self) -> &InstrumentId {
        &self.instrument_id
    }

    /// The bar specification.
    pub fn spec(&self) -> BarSpecification {
        self.spec
    }
}

/// A single OHLCV bar, `f64`-valued, with separate event and init timestamps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bar {
    bar_type: BarType,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
}

impl Bar {
    /// Creates a bar.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        bar_type: BarType,
        open: f64,
        high: f64,
        low: f64,
        close: f64,
        volume: f64,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            bar_type,
            open,
            high,
            low,
            close,
            volume,
            ts_event,
            ts_init,
        }
    }

    /// The bar type (instrument + specification).
    pub fn bar_type(&self) -> &BarType {
        &self.bar_type
    }

    /// Opening price.
    pub fn open(&self) -> f64 {
        self.open
    }

    /// High price.
    pub fn high(&self) -> f64 {
        self.high
    }

    /// Low price.
    pub fn low(&self) -> f64 {
        self.low
    }

    /// Closing price.
    pub fn close(&self) -> f64 {
        self.close
    }

    /// Volume.
    pub fn volume(&self) -> f64 {
        self.volume
    }

    /// Event timestamp: when the bar closed.
    pub fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    /// Init timestamp: when this bar instance was created.
    pub fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

/// A top-of-book quote tick, `f64`-valued.
///
/// `Deserialize` is hand-written, not derived: a crossed quote (`bid_price >
/// ask_price`) is rejected at the wire boundary rather than stored, since
/// [`ParquetTickStore`](crate::ParquetTickStore) reads ticks through this
/// type's deserializer.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QuoteTick {
    instrument_id: InstrumentId,
    bid_price: f64,
    ask_price: f64,
    bid_size: f64,
    ask_size: f64,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
}

impl QuoteTick {
    /// Creates a quote tick.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        bid_price: f64,
        ask_price: f64,
        bid_size: f64,
        ask_size: f64,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            ts_event,
            ts_init,
        }
    }

    /// The quoted instrument.
    pub fn instrument_id(&self) -> &InstrumentId {
        &self.instrument_id
    }

    /// Best bid price.
    pub fn bid_price(&self) -> f64 {
        self.bid_price
    }

    /// Best ask price.
    pub fn ask_price(&self) -> f64 {
        self.ask_price
    }

    /// Best bid size.
    pub fn bid_size(&self) -> f64 {
        self.bid_size
    }

    /// Best ask size.
    pub fn ask_size(&self) -> f64 {
        self.ask_size
    }

    /// Event timestamp.
    pub fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    /// Init timestamp.
    pub fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl<'de> Deserialize<'de> for QuoteTick {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            instrument_id: InstrumentId,
            bid_price: f64,
            ask_price: f64,
            bid_size: f64,
            ask_size: f64,
            ts_event: UnixNanos,
            ts_init: UnixNanos,
        }
        let raw = Raw::deserialize(deserializer)?;
        if raw.bid_price > raw.ask_price {
            return Err(serde::de::Error::custom(format!(
                "crossed quote: bid {} > ask {}",
                raw.bid_price, raw.ask_price
            )));
        }
        Ok(Self {
            instrument_id: raw.instrument_id,
            bid_price: raw.bid_price,
            ask_price: raw.ask_price,
            bid_size: raw.bid_size,
            ask_size: raw.ask_size,
            ts_event: raw.ts_event,
            ts_init: raw.ts_init,
        })
    }
}

/// The initiator side of a trade print.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AggressorSide {
    /// The buyer crossed the spread.
    Buyer,
    /// The seller crossed the spread.
    Seller,
    /// No aggressor could be determined.
    NoAggressor,
}

/// An exchange-assigned trade identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TradeId(String);

impl TradeId {
    /// Creates a trade id.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A single trade print, `f64`-valued.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradeTick {
    instrument_id: InstrumentId,
    price: f64,
    size: f64,
    aggressor_side: AggressorSide,
    trade_id: TradeId,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
}

impl TradeTick {
    /// Creates a trade tick.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        price: f64,
        size: f64,
        aggressor_side: AggressorSide,
        trade_id: TradeId,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            price,
            size,
            aggressor_side,
            trade_id,
            ts_event,
            ts_init,
        }
    }

    /// The traded instrument.
    pub fn instrument_id(&self) -> &InstrumentId {
        &self.instrument_id
    }

    /// Trade price.
    pub fn price(&self) -> f64 {
        self.price
    }

    /// Trade size.
    pub fn size(&self) -> f64 {
        self.size
    }

    /// The initiator side.
    pub fn aggressor_side(&self) -> AggressorSide {
        self.aggressor_side
    }

    /// The exchange trade id.
    pub fn trade_id(&self) -> &TradeId {
        &self.trade_id
    }

    /// Event timestamp.
    pub fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    /// Init timestamp.
    pub fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

/// Instrument reference data, `f64`-valued (lot size, tick size, strike).
///
/// Distinct from `honba_core::Instrument`, which is `Decimal`-valued and has
/// no `currency` field; this crate's Parquet catalog format predates that
/// type and stores currency alongside the instrument.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Instrument {
    id: InstrumentId,
    kind: honba_core::InstrumentKind,
    currency: honba_core::Currency,
    lot_size: f64,
    tick_size: f64,
    expiry: Option<chrono::NaiveDate>,
    strike: Option<f64>,
    option_type: Option<honba_core::OptionType>,
    underlying: Option<String>,
}

impl Instrument {
    /// Creates an instrument with no derivative fields set.
    pub fn new(
        id: InstrumentId,
        kind: honba_core::InstrumentKind,
        currency: honba_core::Currency,
        lot_size: f64,
        tick_size: f64,
    ) -> Self {
        Self {
            id,
            kind,
            currency,
            lot_size,
            tick_size,
            expiry: None,
            strike: None,
            option_type: None,
            underlying: None,
        }
    }

    /// Sets the derivative fields (expiry, strike, option type, underlying), returning `self`.
    #[must_use]
    pub fn with_derivative(
        mut self,
        expiry: Option<chrono::NaiveDate>,
        strike: Option<f64>,
        option_type: Option<honba_core::OptionType>,
        underlying: Option<String>,
    ) -> Self {
        self.expiry = expiry;
        self.strike = strike;
        self.option_type = option_type;
        self.underlying = underlying;
        self
    }

    /// The instrument id.
    pub fn id(&self) -> &InstrumentId {
        &self.id
    }

    /// The instrument kind.
    pub fn kind(&self) -> honba_core::InstrumentKind {
        self.kind
    }

    /// The quoting currency.
    pub fn currency(&self) -> honba_core::Currency {
        self.currency
    }

    /// Lot size.
    pub fn lot_size(&self) -> f64 {
        self.lot_size
    }

    /// Tick size.
    pub fn tick_size(&self) -> f64 {
        self.tick_size
    }

    /// Contract expiry, for futures and options.
    pub fn expiry(&self) -> Option<chrono::NaiveDate> {
        self.expiry
    }

    /// Strike price, for options.
    pub fn strike(&self) -> Option<f64> {
        self.strike
    }

    /// Option type (call/put), for options.
    pub fn option_type(&self) -> Option<honba_core::OptionType> {
        self.option_type
    }

    /// Underlying instrument symbol, for derivatives.
    pub fn underlying(&self) -> Option<&str> {
        self.underlying.as_deref()
    }
}
