//! Zerodha Kite Ticker WebSocket feed adapter and binary packet parser.

use async_trait::async_trait;
use chrono::Utc;
use futures::stream::BoxStream;
use rust_decimal::Decimal;
use std::sync::Arc;
use tokio::sync::RwLock;

use honba_core::{Exchange, InstrumentId, Price, Quantity, Tick};
use honba_me_core::{
    CoalescingTickBuffer, FeedHealth, MarketDataFeed, MdError, ReconnectionManager, Subscription,
    SymbolMapper,
};

/// Zerodha Kite Ticker feed adapter.
pub struct ZerodhaFeed {
    /// Zerodha API key
    pub api_key: String,
    /// Access token
    pub access_token: String,
    /// WebSocket endpoint
    pub ws_url: String,
    /// Symbol and token mapper
    pub mapper: Arc<RwLock<SymbolMapper>>,
    /// Connection and subscription state manager
    pub reconnection: Arc<ReconnectionManager>,
}

impl ZerodhaFeed {
    /// Default Kite Ticker WebSocket endpoint.
    pub const DEFAULT_WS_URL: &'static str = "wss://ws.kite.trade";

    /// Create a new Zerodha feed adapter.
    pub fn new(api_key: impl Into<String>, access_token: impl Into<String>) -> Self {
        Self::with_ws_url(api_key, access_token, Self::DEFAULT_WS_URL)
    }

    /// Create adapter with custom WebSocket URL.
    pub fn with_ws_url(
        api_key: impl Into<String>,
        access_token: impl Into<String>,
        ws_url: impl Into<String>,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            access_token: access_token.into(),
            ws_url: ws_url.into(),
            mapper: Arc::new(RwLock::new(SymbolMapper::new())),
            reconnection: Arc::new(ReconnectionManager::new()),
        }
    }

    /// Parse an 8-byte Kite LTP binary packet into canonical [`Tick::Ltp`].
    pub fn parse_binary_ltp(bytes: &[u8], mapper: &SymbolMapper) -> Result<Tick, MdError> {
        if bytes.len() < 8 {
            return Err(MdError::Transport("LTP packet too short".into()));
        }

        let token = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let ltp_raw = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let price_dec = Decimal::from(ltp_raw) / Decimal::from(100);

        let token_str = token.to_string();
        let inst = mapper.to_canonical(&token_str).unwrap_or_else(|| {
            InstrumentId::new(Exchange::Nse, format!("TOKEN-{}", token)).with_token(token_str)
        });

        let now = Utc::now();
        Ok(Tick::Ltp {
            instrument: inst,
            price: Price::new(price_dec),
            volume: Quantity::ZERO,
            exchange_ts: now,
            receive_ts: now,
        })
    }

    /// Parse a 28-byte Kite Quote binary packet into canonical [`Tick::Quote`].
    pub fn parse_binary_quote(bytes: &[u8], mapper: &SymbolMapper) -> Result<Tick, MdError> {
        if bytes.len() < 28 {
            return Err(MdError::Transport("Quote packet too short".into()));
        }

        let token = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let ltp_raw = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let ltp_dec = Decimal::from(ltp_raw) / Decimal::from(100);

        let token_str = token.to_string();
        let inst = mapper.to_canonical(&token_str).unwrap_or_else(|| {
            InstrumentId::new(Exchange::Nse, format!("TOKEN-{}", token)).with_token(token_str)
        });

        let now = Utc::now();
        Ok(Tick::Quote {
            instrument: inst,
            bid: Price::new(ltp_dec),
            ask: Price::new(ltp_dec),
            bid_qty: Quantity::ZERO,
            ask_qty: Quantity::ZERO,
            last_price: Some(Price::new(ltp_dec)),
            exchange_ts: now,
            receive_ts: now,
            band: None,
        })
    }
}

#[async_trait]
impl MarketDataFeed for ZerodhaFeed {
    fn name(&self) -> &str {
        "zerodha"
    }

    async fn subscribe(
        &self,
        subs: Vec<Subscription>,
    ) -> Result<BoxStream<'static, Tick>, MdError> {
        for s in &subs {
            self.reconnection.add_subscription(s.clone()).await;
        }

        let (buf, rx) = CoalescingTickBuffer::new(5000);
        let _ = buf; // In live client, the WebSocket reader writes to `buf`
        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Box::pin(stream))
    }

    async fn unsubscribe(&self, instruments: &[InstrumentId]) -> Result<(), MdError> {
        for inst in instruments {
            self.reconnection.remove_subscription(inst).await;
        }
        Ok(())
    }

    async fn fetch_snapshot(
        &self,
        _instruments: &[InstrumentId],
    ) -> Result<Vec<Tick>, MdError> {
        // Stub: REST snapshot fetch would be implemented here
        Ok(Vec::new())
    }

    fn feed_health(&self) -> FeedHealth {
        FeedHealth {
            name: self.name().to_string(),
            connected: false,
            last_heartbeat: None,
            rtt_ms: None,
            dropped_count: 0,
            reconnect_count: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_binary_ltp() {
        let mut mapper = SymbolMapper::new();
        let inst = InstrumentId::new(Exchange::Nse, "INFY").with_token("408065");
        mapper.register("408065", inst.clone());

        // Token 408065 = 0x00063A01, Price 1450.50 * 100 = 145050 = 0x0002369A
        let mut bytes = [0u8; 8];
        bytes[0..4].copy_from_slice(&408065u32.to_be_bytes());
        bytes[4..8].copy_from_slice(&145050u32.to_be_bytes());

        let tick = ZerodhaFeed::parse_binary_ltp(&bytes, &mapper).unwrap();
        match tick {
            Tick::Ltp {
                instrument, price, ..
            } => {
                assert_eq!(instrument.symbol, "INFY");
                assert_eq!(price.0, Decimal::from_str_exact("1450.50").unwrap());
            }
            _ => panic!("expected LTP tick"),
        }
    }
}
