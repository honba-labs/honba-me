//! Local async market-data ports.
//!
//! These used to come from the deleted `honba-kokoro-ports` crate.
//! `honba_core::ports` now owns a *synchronous* port-trait vocabulary,
//! deliberately kept dependency-light so the Kernel stays zero-I/O and
//! WASM-portable. This crate's actual readers (Parquet, in-memory, bhavcopy)
//! do real I/O and are naturally `async`, so they cannot literally implement
//! `honba_core`'s sync traits. These are the local, async equivalents this
//! crate implements instead, built on the local `crate::wire` types.
//!
//! `honba_core::PortError`/`PortResult` would almost fit, but this crate's
//! real usage needs an `Internal` and a `Terminal` variant that don't exist
//! there (the Kernel's version doesn't model a terminal/non-retryable
//! connector failure), so the error type is local too.

use chrono::NaiveDate;
use honba_core::InstrumentId;

use crate::wire::{Bar, BarSpecification, Instrument, QuoteTick, TradeTick, UnixNanos};

/// Errors a port can return.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PortError {
    /// The request cannot be served by this port implementation.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// An internal failure (I/O, decoding, ...).
    #[error("internal: {0}")]
    Internal(String),
    /// A permanent failure; the caller should stop, not retry.
    #[error("terminal: {0}")]
    Terminal(String),
}

impl PortError {
    /// Whether a caller should retry after this error.
    ///
    /// Only [`PortError::Terminal`] is not retryable; the connector or
    /// session that produced it is permanently done.
    pub fn is_retryable(&self) -> bool {
        !matches!(self, Self::Terminal(_))
    }
}

/// Result type for port operations.
pub type PortResult<T> = Result<T, PortError>;

/// Connection state of a [`MarketDataConnector`].
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectorState {
    /// Not connected.
    Disconnected {
        /// Why the connector is disconnected.
        reason: String,
    },
    /// Attempting to reconnect.
    Reconnecting {
        /// Which attempt this is (1-based).
        attempt: u32,
        /// Backoff before the next attempt, in milliseconds.
        backoff_ms: u64,
    },
    /// Connected and delivering data.
    Connected,
}

/// A request for historical bars over `[from, to)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarRequest {
    instrument: InstrumentId,
    spec: BarSpecification,
    from: Option<UnixNanos>,
    to: Option<UnixNanos>,
}

impl BarRequest {
    /// Creates a request, rejecting a `from` at or after `to`.
    pub fn new(
        instrument: InstrumentId,
        spec: BarSpecification,
        from: Option<UnixNanos>,
        to: Option<UnixNanos>,
    ) -> PortResult<Self> {
        if let (Some(from), Some(to)) = (from, to) {
            if from >= to {
                return Err(PortError::Unsupported(
                    "bar request `from` must be before `to`".into(),
                ));
            }
        }
        Ok(Self {
            instrument,
            spec,
            from,
            to,
        })
    }

    /// The requested instrument.
    pub fn instrument(&self) -> &InstrumentId {
        &self.instrument
    }

    /// The requested bar specification.
    pub fn spec(&self) -> BarSpecification {
        self.spec
    }

    /// Start of the range, inclusive, or unbounded.
    pub fn from(&self) -> Option<UnixNanos> {
        self.from
    }

    /// End of the range, exclusive, or unbounded.
    pub fn to(&self) -> Option<UnixNanos> {
        self.to
    }
}

/// A request for historical ticks over `[from, to)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickRequest {
    instrument: InstrumentId,
    from: Option<UnixNanos>,
    to: Option<UnixNanos>,
}

impl TickRequest {
    /// Creates a request, rejecting a `from` at or after `to`.
    pub fn new(
        instrument: InstrumentId,
        from: Option<UnixNanos>,
        to: Option<UnixNanos>,
    ) -> PortResult<Self> {
        if let (Some(from), Some(to)) = (from, to) {
            if from >= to {
                return Err(PortError::Unsupported(
                    "tick request `from` must be before `to`".into(),
                ));
            }
        }
        Ok(Self { instrument, from, to })
    }

    /// The requested instrument.
    pub fn instrument(&self) -> &InstrumentId {
        &self.instrument
    }

    /// Start of the range, inclusive, or unbounded.
    pub fn from(&self) -> Option<UnixNanos> {
        self.from
    }

    /// End of the range, exclusive, or unbounded.
    pub fn to(&self) -> Option<UnixNanos> {
        self.to
    }
}

/// A requested set of instruments to subscribe to (empty means "all").
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SubscriptionParams {
    symbols: Vec<InstrumentId>,
}

impl SubscriptionParams {
    /// Creates a subscription request for `symbols` (empty means "all").
    pub fn new(symbols: Vec<InstrumentId>) -> Self {
        Self { symbols }
    }

    /// The requested instruments.
    pub fn symbols(&self) -> &[InstrumentId] {
        &self.symbols
    }
}

/// The instruments a [`MarketDataConnector::subscribe`] call actually subscribed to.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Subscription {
    symbols: Vec<InstrumentId>,
}

impl Subscription {
    /// Creates a subscription confirmation for `symbols`.
    pub fn new(symbols: Vec<InstrumentId>) -> Self {
        Self { symbols }
    }

    /// The subscribed instruments.
    pub fn symbols(&self) -> &[InstrumentId] {
        &self.symbols
    }
}

/// One market data event a [`MarketDataConnector`] can deliver.
#[derive(Debug, Clone, PartialEq)]
pub enum MarketEvent {
    /// A bar closed.
    Bar(Bar),
    /// A quote updated.
    QuoteTick(QuoteTick),
    /// A trade printed.
    TradeTick(TradeTick),
    /// No data arrived for `instrument` since `since`; an in-band gap notice,
    /// not a failure.
    GapMarker {
        /// The instrument with no recent data.
        instrument: InstrumentId,
        /// When data was last seen for it.
        since: UnixNanos,
    },
}

/// An empty order-book snapshot shape for sources that hold no book (a bar
/// or last-price-only store always answers [`PortError::Unsupported`]
/// instead of constructing one of these, so the exact shape here matters
/// only to future depth-capable sources).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DepthSnapshot {
    /// Bid levels, best first, as (price, size) pairs.
    pub bids: Vec<(f64, f64)>,
    /// Ask levels, best first, as (price, size) pairs.
    pub asks: Vec<(f64, f64)>,
}

/// An instrument's reference data as of a given effective date range.
#[derive(Debug, Clone, PartialEq)]
pub struct DatedInstrument {
    /// The instrument record.
    pub instrument: Instrument,
    /// Exchange single-order freeze quantity in force for this record.
    pub freeze_qty: f64,
    /// Date this record took effect.
    pub effective_from: NaiveDate,
    /// Date this record stopped being in force, or `None` if still current.
    pub effective_to: Option<NaiveDate>,
}

/// Serves historical bars for an instrument and specification.
#[async_trait::async_trait]
pub trait BarReader: Send + Sync {
    /// Reads the bars matching `request`.
    async fn read_bars(&self, request: &BarRequest) -> PortResult<Vec<Bar>>;
}

/// Serves historical quote and trade ticks.
#[async_trait::async_trait]
pub trait TickReader: Send + Sync {
    /// Reads the quotes matching `request`.
    async fn read_quotes(&self, request: &TickRequest) -> PortResult<Vec<QuoteTick>>;
    /// Reads the trades matching `request`.
    async fn read_trades(&self, request: &TickRequest) -> PortResult<Vec<TradeTick>>;
}

/// Serves the latest quote for an instrument, optionally as of a past time.
#[async_trait::async_trait]
pub trait QuoteReader: Send + Sync {
    /// Reads the latest quote for `id` at or before `as_of`, or the live
    /// latest quote when `as_of` is `None`.
    async fn read_quote(
        &self,
        id: &InstrumentId,
        as_of: Option<UnixNanos>,
    ) -> PortResult<Option<QuoteTick>>;
}

/// Serves an order-book depth snapshot for an instrument.
#[async_trait::async_trait]
pub trait DepthReader: Send + Sync {
    /// Reads up to `levels` of depth for `id`.
    async fn read_depth(&self, id: &InstrumentId, levels: usize) -> PortResult<DepthSnapshot>;
}

/// Serves instrument reference data, optionally as of a backtest date.
#[async_trait::async_trait]
pub trait InstrumentMaster: Send + Sync {
    /// Looks up the current record for `id`.
    async fn get_instrument(&self, id: &InstrumentId) -> PortResult<Option<Instrument>>;
    /// Lists every instrument this master knows about.
    async fn list_instruments(&self) -> PortResult<Vec<Instrument>>;
    /// A dated snapshot source may override these to serve historical
    /// catalog states; the default errors, since most masters only know
    /// the current state.
    async fn snapshot(&self, _as_of: NaiveDate) -> PortResult<Vec<DatedInstrument>> {
        Err(PortError::Unsupported(
            "this instrument master holds no dated history".into(),
        ))
    }
    /// Resolves `id` as of a backtest date; the default errors, since most
    /// masters only know the current state.
    async fn resolve_as_of(
        &self,
        _id: &InstrumentId,
        _as_of: NaiveDate,
    ) -> PortResult<Option<Instrument>> {
        Err(PortError::Unsupported(
            "this instrument master holds no dated history".into(),
        ))
    }
}

/// A live or replayed source of [`MarketEvent`]s for a subscribed instrument set.
#[async_trait::async_trait]
pub trait MarketDataConnector: Send + Sync {
    /// Subscribes to `params`, returning the instruments actually subscribed to.
    async fn subscribe(&mut self, params: SubscriptionParams) -> PortResult<Subscription>;
    /// Pulls the next event, or `None` when the source is exhausted (not an error).
    async fn next_event(&mut self) -> PortResult<Option<MarketEvent>>;
    /// The connector's current connection state.
    fn state(&self) -> ConnectorState;
}
