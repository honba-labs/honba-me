//! An in-memory [`TickReader`]: quote/trade ticks held in a `Vec`, for tests and for runs
//! whose ticks the caller already has (ADR 0030).

use async_trait::async_trait;
use honba_messages::{QuoteTick, TradeTick, UnixNanos};
use honba_ports::{PortError, PortResult, TickReader, TickRequest};

/// Quote and trade ticks held in memory, sorted by `ts_event` (stable for ties).
///
/// A store built with no ticks at all answers [`PortError::Unsupported`]: it holds no tick data,
/// which is different from holding some for other instruments and none for this one.
#[derive(Clone, Debug, Default)]
pub struct InMemoryTickReader {
    quotes: Vec<QuoteTick>,
    trades: Vec<TradeTick>,
}

impl InMemoryTickReader {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds quotes (any instrument, any order).
    #[must_use]
    pub fn with_quotes(mut self, quotes: Vec<QuoteTick>) -> Self {
        self.quotes.extend(quotes);
        self.quotes.sort_by_key(QuoteTick::ts_event);
        self
    }

    /// Adds trades (any instrument, any order).
    #[must_use]
    pub fn with_trades(mut self, trades: Vec<TradeTick>) -> Self {
        self.trades.extend(trades);
        self.trades.sort_by_key(TradeTick::ts_event);
        self
    }

    fn check_held(&self) -> PortResult<()> {
        if self.quotes.is_empty() && self.trades.is_empty() {
            return Err(PortError::Unsupported(
                "this reader holds no tick data".into(),
            ));
        }
        Ok(())
    }

    /// The quotes held, in `ts_event` order (as written), for export and inspection.
    pub fn quotes(&self) -> &[QuoteTick] {
        &self.quotes
    }

    /// The trades held, in `ts_event` order (as written), for export and inspection.
    pub fn trades(&self) -> &[TradeTick] {
        &self.trades
    }
}

fn in_range(t: UnixNanos, request: &TickRequest) -> bool {
    request.from().map_or(true, |f| t >= f) && request.to().map_or(true, |e| t < e)
}

#[async_trait]
impl TickReader for InMemoryTickReader {
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
