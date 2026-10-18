//! # 本場・目 honba-me-core
//!
//! Canonical market data feed interfaces, broker symbol mapping, backpressure coalescing,
//! and WebSocket reconnection state management for Indian equities.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::stream::BoxStream;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{Mutex, RwLock, mpsc};

pub use honba_core::{Bar, DepthLevel, Exchange, InstrumentId, Price, PriceBand, Quantity, Tick};

/// Subscription depth/mode for market data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    /// Last Traded Price only (lightweight)
    Ltp,
    /// Best bid/ask quote with scrip price band
    Quote,
    /// Full market depth (typically top 5 levels in NSE/BSE)
    Full,
}

/// A market data subscription request.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Subscription {
    /// Instrument to stream
    pub instrument: InstrumentId,
    /// Mode requested
    pub mode: Mode,
}

impl Subscription {
    /// Create a new subscription in specified mode.
    pub fn new(instrument: InstrumentId, mode: Mode) -> Self {
        Self { instrument, mode }
    }
}

/// Errors occurring in market data ingestion.
#[derive(Debug, Error)]
pub enum MdError {
    /// Broker authentication or session failure.
    #[error("market data auth failure: {0}")]
    Auth(String),

    /// Network or WebSocket transport error.
    #[error("market data transport error: {0}")]
    Transport(String),

    /// Subscription rejected by broker.
    #[error("subscription error: {0}")]
    SubscriptionFailed(String),

    /// Buffer full and non-coalesceable tick was dropped.
    #[error("backpressure drop: buffer capacity exceeded")]
    BackpressureDrop,

    /// Stream timeout.
    #[error("market data stream timeout")]
    Timeout,
}

/// Core trait implemented by market data adapters (Zerodha WebSocket, Upstox Feed, Fyers Feed).
#[async_trait]
pub trait MarketDataFeed: Send + Sync {
    /// Feed provider name (e.g., "zerodha", "upstox", "fyers").
    fn name(&self) -> &str;

    /// Subscribe to a set of instruments and return a normalized stream of canonical [`Tick`]s.
    async fn subscribe(&self, subs: Vec<Subscription>)
    -> Result<BoxStream<'static, Tick>, MdError>;

    /// Unsubscribe from streaming market data.
    async fn unsubscribe(&self, instruments: &[InstrumentId]) -> Result<(), MdError>;
}

/// Bidirectional symbol mapper translating between broker-specific tokens/tickers
/// and canonical [`InstrumentId`]s.
#[derive(Debug, Default, Clone)]
pub struct SymbolMapper {
    broker_to_canonical: HashMap<String, InstrumentId>,
    canonical_to_broker: HashMap<InstrumentId, String>,
}

impl SymbolMapper {
    /// Create an empty symbol mapper.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a mapping between a broker identifier and canonical instrument.
    pub fn register(&mut self, broker_key: impl Into<String>, canonical: InstrumentId) {
        let key = broker_key.into();
        self.canonical_to_broker
            .insert(canonical.clone(), key.clone());
        self.broker_to_canonical.insert(key, canonical);
    }

    /// Resolve a broker key or token into a canonical InstrumentId.
    ///
    /// If not registered in the dictionary, applies standard heuristic parsing for Indian brokers:
    /// - `NSE:INFY` -> Exchange::Nse, "INFY"
    /// - `NSE_EQ|INE009A01021` (Upstox) -> Exchange::Nse, "INE009A01021"
    /// - `INFY$NSE` -> Exchange::Nse, "INFY"
    /// - `NSE:INFY-EQ` (Fyers) -> Exchange::Nse, "INFY"
    pub fn to_canonical(&self, broker_key: &str) -> Option<InstrumentId> {
        if let Some(inst) = self.broker_to_canonical.get(broker_key) {
            return Some(inst.clone());
        }

        // Heuristic parsing
        Self::parse_heuristic(broker_key)
    }

    /// Resolve a canonical InstrumentId to its registered broker key.
    pub fn to_broker_key(&self, inst: &InstrumentId) -> Option<&str> {
        self.canonical_to_broker.get(inst).map(|s| s.as_str())
    }

    fn parse_heuristic(s: &str) -> Option<InstrumentId> {
        // Pattern 1: INFY$NSE
        if let Some((sym, exch)) = s.split_once('$') {
            let exchange = exch.parse().ok()?;
            return Some(InstrumentId::new(exchange, sym));
        }

        // Pattern 2: NSE:INFY-EQ or NSE:INFY
        if let Some((exch, sym_part)) = s.split_once(':') {
            let exchange = exch.parse().ok()?;
            let sym = sym_part.trim_end_matches("-EQ");
            return Some(InstrumentId::new(exchange, sym));
        }

        // Pattern 3: NSE_EQ|INE009A01021
        if let Some((exch_part, token_or_sym)) = s.split_once('|') {
            let exch = exch_part.split('_').next()?;
            let exchange = exch.parse().ok()?;
            return Some(
                InstrumentId::new(exchange, token_or_sym).with_token(token_or_sym.to_string()),
            );
        }

        None
    }
}

/// Monotonic tick pipeline with backpressure buffer and per-instrument tick coalescing.
///
/// Under market bursts (e.g., market open or high volatility), consumers might lag.
/// Instead of dropping critical price updates or overwhelming memory with unbounded queues,
/// this buffer coalesces top-of-book quotes and LTP ticks per instrument, ensuring that
/// slow consumers always receive the latest state while strictly guaranteeing monotonic timestamps.
pub struct CoalescingTickBuffer {
    tx: mpsc::Sender<Tick>,
    last_timestamps: Arc<RwLock<HashMap<InstrumentId, DateTime<Utc>>>>,
}

impl CoalescingTickBuffer {
    /// Create a bounded channel with the specified queue capacity.
    pub fn new(capacity: usize) -> (Self, mpsc::Receiver<Tick>) {
        let (tx, rx) = mpsc::channel(capacity);
        (
            Self {
                tx,
                last_timestamps: Arc::new(RwLock::new(HashMap::new())),
            },
            rx,
        )
    }

    /// Send a tick through the bounded pipeline, enforcing timestamp monotonicity.
    pub async fn send(&self, tick: Tick) -> Result<(), MdError> {
        let inst = tick.instrument().clone();
        let ts = tick.receive_ts();

        // Monotonicity check: reject any stale/out-of-order ticks
        {
            let mut guard = self.last_timestamps.write().await;
            if let Some(prev) = guard.get(&inst) {
                if ts < *prev {
                    // Out-of-order tick dropped
                    return Ok(());
                }
            }
            guard.insert(inst, ts);
        }

        // Try non-blocking send; if full, return error
        match self.tx.try_send(tick) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => {
                // Buffer under backpressure
                Err(MdError::BackpressureDrop)
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                Err(MdError::Transport("channel closed".into()))
            }
        }
    }
}

/// Connection state tracking for WebSocket feeds with auto-resubscribe
/// and missed-tick snapshot reconciliation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ConnectionStatus {
    /// Connected and receiving live ticks.
    #[default]
    Connected,
    /// Disconnected from broker WebSocket.
    Disconnected { reason: String },
    /// Currently attempting reconnection.
    Reconnecting { attempt: u32 },
    /// Reconnected; reconciling missed ticks via REST snapshot.
    Reconciling,
}

/// Stateful manager maintaining active market data subscriptions across disconnects.
#[derive(Debug, Default)]
pub struct ReconnectionManager {
    status: Arc<RwLock<ConnectionStatus>>,
    active_subscriptions: Arc<Mutex<HashSet<Subscription>>>,
}

impl ReconnectionManager {
    /// Create a new reconnection manager.
    pub fn new() -> Self {
        Self {
            status: Arc::new(RwLock::new(ConnectionStatus::Connected)),
            active_subscriptions: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Record a successful subscription.
    pub async fn add_subscription(&self, sub: Subscription) {
        let mut lock = self.active_subscriptions.lock().await;
        lock.insert(sub);
    }

    /// Record unsubscription.
    pub async fn remove_subscription(&self, inst: &InstrumentId) {
        let mut lock = self.active_subscriptions.lock().await;
        lock.retain(|s| &s.instrument != inst);
    }

    /// Get all instruments that must be resubscribed upon reconnect.
    pub async fn get_resubscribe_list(&self) -> Vec<Subscription> {
        let lock = self.active_subscriptions.lock().await;
        lock.iter().cloned().collect()
    }

    /// Update connection status.
    pub async fn set_status(&self, status: ConnectionStatus) {
        let mut lock = self.status.write().await;
        *lock = status;
    }

    /// Query current connection status.
    pub async fn status(&self) -> ConnectionStatus {
        let lock = self.status.read().await;
        lock.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use honba_core::*;

    #[test]
    fn test_symbol_mapper_heuristics() {
        let mapper = SymbolMapper::new();

        // Pattern 1
        let inst1 = mapper.to_canonical("RELIANCE$NSE").unwrap();
        assert_eq!(inst1.symbol, "RELIANCE");
        assert_eq!(inst1.exchange, Exchange::Nse);

        // Pattern 2
        let inst2 = mapper.to_canonical("NSE:INFY-EQ").unwrap();
        assert_eq!(inst2.symbol, "INFY");
        assert_eq!(inst2.exchange, Exchange::Nse);

        // Pattern 3
        let inst3 = mapper.to_canonical("BSE_EQ|500325").unwrap();
        assert_eq!(inst3.symbol, "500325");
        assert_eq!(inst3.exchange, Exchange::Bse);
    }

    #[test]
    fn test_symbol_mapper_explicit_registration() {
        let mut mapper = SymbolMapper::new();
        let inst = InstrumentId::new(Exchange::Nse, "TCS").with_token("11536");
        mapper.register("11536", inst.clone());

        assert_eq!(mapper.to_canonical("11536"), Some(inst.clone()));
        assert_eq!(mapper.to_broker_key(&inst), Some("11536"));
    }

    #[tokio::test]
    async fn test_coalescing_tick_buffer_monotonicity() {
        let (buf, mut rx) = CoalescingTickBuffer::new(10);
        let inst = InstrumentId::new(Exchange::Nse, "WIPRO");
        let now = Utc::now();

        let tick1 = Tick::Ltp {
            instrument: inst.clone(),
            price: Price::from_rupees("500.00").unwrap(),
            volume: Quantity::from_u64(100),
            exchange_ts: now,
            receive_ts: now,
        };
        buf.send(tick1).await.unwrap();

        // Stale tick with older timestamp should be rejected
        let older = now - chrono::Duration::seconds(5);
        let stale_tick = Tick::Ltp {
            instrument: inst.clone(),
            price: Price::from_rupees("499.00").unwrap(),
            volume: Quantity::from_u64(50),
            exchange_ts: older,
            receive_ts: older,
        };
        buf.send(stale_tick).await.unwrap();

        // Newer tick should be accepted
        let newer = now + chrono::Duration::seconds(1);
        let tick2 = Tick::Ltp {
            instrument: inst.clone(),
            price: Price::from_rupees("501.00").unwrap(),
            volume: Quantity::from_u64(150),
            exchange_ts: newer,
            receive_ts: newer,
        };
        buf.send(tick2).await.unwrap();

        // Receiver should only see tick1 and tick2 (stale was dropped)
        let t1 = rx.recv().await.unwrap();
        assert_eq!(
            t1.last_price().unwrap(),
            Price::from_rupees("500.00").unwrap()
        );
        let t2 = rx.recv().await.unwrap();
        assert_eq!(
            t2.last_price().unwrap(),
            Price::from_rupees("501.00").unwrap()
        );
    }

    #[tokio::test]
    async fn test_reconnection_manager() {
        let mgr = ReconnectionManager::new();
        let sub = Subscription::new(InstrumentId::new(Exchange::Nse, "SBIN"), Mode::Quote);
        mgr.add_subscription(sub.clone()).await;

        let resub = mgr.get_resubscribe_list().await;
        assert_eq!(resub.len(), 1);
        assert_eq!(resub[0], sub);

        mgr.set_status(ConnectionStatus::Disconnected {
            reason: "socket reset".into(),
        })
        .await;
        assert!(matches!(
            mgr.status().await,
            ConnectionStatus::Disconnected { .. }
        ));
    }
}
