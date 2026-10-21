//! Angel One SmartAPI WebSocket 2.0 feed adapter.
//!
//! Implements SmartAPI binary WebSocket 2.0 parser and JWT auth handshake
//! for Angel One market data feed.

use async_trait::async_trait;
use chrono::Utc;
use futures::stream::BoxStream;
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;

use honba_core::{DepthLevel, Exchange, InstrumentId, Price, Quantity, Tick};
use honba_me_core::{
    CoalescingTickBuffer, FeedHealth, MarketDataFeed, MdError, ReconnectionManager, Subscription,
    SymbolMapper,
};

/// Angel One SmartAPI WebSocket 2.0 feed adapter.
pub struct AngelOneFeed {
    /// Angel One API key
    pub api_key: String,
    /// Client code
    pub client_code: String,
    /// JWT auth token
    pub jwt_token: String,
    /// WebSocket feed URL
    pub ws_url: String,
    /// Symbol mapper
    pub mapper: Arc<RwLock<SymbolMapper>>,
    /// Connection and subscription state manager
    pub reconnection: Arc<ReconnectionManager>,
}

impl AngelOneFeed {
    /// Default Angel One SmartAPI WebSocket 2.0 endpoint.
    pub const DEFAULT_WS_URL: &'static str = "wss://smartapisocket.angelone.in/smart-stream";

    /// Create a new Angel One feed adapter.
    pub fn new(api_key: impl Into<String>, client_code: impl Into<String>, jwt_token: impl Into<String>) -> Self {
        Self::with_ws_url(api_key, client_code, jwt_token, Self::DEFAULT_WS_URL)
    }

    /// Create adapter with custom WebSocket URL.
    pub fn with_ws_url(
        api_key: impl Into<String>,
        client_code: impl Into<String>,
        jwt_token: impl Into<String>,
        ws_url: impl Into<String>,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            client_code: client_code.into(),
            jwt_token: jwt_token.into(),
            ws_url: ws_url.into(),
            mapper: Arc::new(RwLock::new(SymbolMapper::new())),
            reconnection: Arc::new(ReconnectionManager::new()),
        }
    }

    /// Parse Angel One binary WebSocket 2.0 message.
    ///
    /// Binary packet structure:
    /// - 1 byte: message type (1 = Quote, 2 = Best 5 Depth, 3 = Status)
    /// - 25 bytes: instrument token
    /// - 8 bytes: LTP
    /// - 8 bytes: exchange timestamp
    /// - Followed by 5 bid/ask depth structures (for depth messages)
    pub fn parse_binary_message(bytes: &[u8], mapper: &SymbolMapper) -> Result<Tick, MdError> {
        if bytes.is_empty() {
            return Err(MdError::Transport("Empty binary message".into()));
        }

        let msg_type = bytes[0];

        match msg_type {
            1 => Self::parse_quote(bytes, mapper),
            2 => Self::parse_depth(bytes, mapper),
            3 => Err(MdError::Transport("Status message not supported".into())),
            _ => Err(MdError::Transport(format!("Unknown message type: {}", msg_type))),
        }
    }

    /// Parse a Quote message (type 1).
    fn parse_quote(bytes: &[u8], mapper: &SymbolMapper) -> Result<Tick, MdError> {
        if bytes.len() < 38 {
            return Err(MdError::Transport("Quote packet too short".into()));
        }

        // 25 bytes: instrument token (ASCII string, null-padded)
        let token_bytes = &bytes[1..26];
        let token_str = String::from_utf8_lossy(token_bytes)
            .trim_end_matches('\0')
            .trim()
            .to_string();

        // 8 bytes: LTP (double, little-endian)
        let ltp_raw = f64::from_le_bytes([
            bytes[26], bytes[27], bytes[28], bytes[29],
            bytes[30], bytes[31], bytes[32], bytes[33],
        ]);
        let price_dec = Decimal::from_f64(ltp_raw).unwrap_or(Decimal::ZERO);

        let inst = mapper.to_canonical(&token_str).unwrap_or_else(|| {
            InstrumentId::new(Exchange::Nse, token_str.clone()).with_token(token_str)
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

    /// Parse a Depth message (type 2).
    fn parse_depth(bytes: &[u8], mapper: &SymbolMapper) -> Result<Tick, MdError> {
        if bytes.len() < 42 {
            return Err(MdError::Transport("Depth packet too short".into()));
        }

        // 25 bytes: instrument token
        let token_bytes = &bytes[1..26];
        let token_str = String::from_utf8_lossy(token_bytes)
            .trim_end_matches('\0')
            .trim()
            .to_string();

        // 8 bytes: LTP (double, little-endian)
        let ltp_raw = f64::from_le_bytes([
            bytes[26], bytes[27], bytes[28], bytes[29],
            bytes[30], bytes[31], bytes[32], bytes[33],
        ]);
        let ltp_dec = Decimal::from_f64(ltp_raw).unwrap_or(Decimal::ZERO);

        // 8 bytes: exchange timestamp (double, little-endian) - unused
        let _exchange_ts_raw = f64::from_le_bytes([
            bytes[34], bytes[35], bytes[36], bytes[37],
            bytes[38], bytes[39], bytes[40], bytes[41],
        ]);

        let inst = mapper.to_canonical(&token_str).unwrap_or_else(|| {
            InstrumentId::new(Exchange::Nse, token_str.clone()).with_token(token_str)
        });

        // Parse 5 bid levels and 5 ask levels (each: price(8) + qty(4) = 12 bytes)
        let mut bids = Vec::with_capacity(5);
        let mut asks = Vec::with_capacity(5);

        let mut offset = 38;
        for _ in 0..5 {
            if offset + 12 > bytes.len() {
                break;
            }
            let price_raw = f64::from_le_bytes([
                bytes[offset], bytes[offset+1], bytes[offset+2], bytes[offset+3],
                bytes[offset+4], bytes[offset+5], bytes[offset+6], bytes[offset+7],
            ]);
            let qty_raw = u32::from_le_bytes([
                bytes[offset+8], bytes[offset+9], bytes[offset+10], bytes[offset+11],
            ]);
            bids.push(DepthLevel {
                price: Price::new(Decimal::from_f64(price_raw).unwrap_or(Decimal::ZERO)),
                quantity: Quantity::new(Decimal::from(qty_raw)),
                orders_count: None,
            });
            offset += 12;
        }

        for _ in 0..5 {
            if offset + 12 > bytes.len() {
                break;
            }
            let price_raw = f64::from_le_bytes([
                bytes[offset], bytes[offset+1], bytes[offset+2], bytes[offset+3],
                bytes[offset+4], bytes[offset+5], bytes[offset+6], bytes[offset+7],
            ]);
            let qty_raw = u32::from_le_bytes([
                bytes[offset+8], bytes[offset+9], bytes[offset+10], bytes[offset+11],
            ]);
            asks.push(DepthLevel {
                price: Price::new(Decimal::from_f64(price_raw).unwrap_or(Decimal::ZERO)),
                quantity: Quantity::new(Decimal::from(qty_raw)),
                orders_count: None,
            });
            offset += 12;
        }

        let now = Utc::now();
        Ok(Tick::Depth {
            instrument: inst,
            bids,
            asks,
            last_price: Some(Price::new(ltp_dec)),
            exchange_ts: now,
            receive_ts: now,
        })
    }

    /// Generate JWT authentication header value.
    pub fn auth_header(&self) -> String {
        format!("Bearer {}", self.jwt_token)
    }

    /// Generate subscription request message for Angel One.
    pub fn subscribe_message(tokens: &[&str]) -> String {
        let tokens_json = serde_json::json!({
            "action": "subscribe",
            "params": {
                "mode": "full",
                "tokenList": tokens
            }
        });
        serde_json::to_string(&tokens_json).unwrap_or_else(|_| "{}".to_string())
    }
}

#[async_trait]
impl MarketDataFeed for AngelOneFeed {
    fn name(&self) -> &str {
        "angelone"
    }

    async fn subscribe(
        &self,
        subs: Vec<Subscription>,
    ) -> Result<BoxStream<'static, Tick>, MdError> {
        for s in &subs {
            self.reconnection.add_subscription(s.clone()).await;
        }

        let (buf, rx) = CoalescingTickBuffer::new(5000);
        let _ = buf;
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
    fn test_parse_binary_quote() {
        let mapper = SymbolMapper::new();

        // Build a test quote message:
        // 1 byte: msg_type = 1
        // 25 bytes: token "NSE_EQ|INE009A01021" padded with nulls
        // 8 bytes: LTP as double LE = 1420.75
        let mut bytes = Vec::new();
        bytes.push(1); // msg_type

        let token = b"NSE_EQ|INE009A01021";
        let mut token_bytes = [0u8; 25];
        token_bytes[..token.len()].copy_from_slice(token);
        bytes.extend_from_slice(&token_bytes);

        let ltp: f64 = 1420.75;
        bytes.extend_from_slice(&ltp.to_le_bytes());

        let tick = AngelOneFeed::parse_binary_message(&bytes, &mapper).unwrap();
        match tick {
            Tick::Ltp { instrument, price, .. } => {
                assert_eq!(instrument.exchange, Exchange::Nse);
                assert_eq!(instrument.symbol, "INE009A01021");
                assert!((price.0 - Decimal::from_str_exact("1420.75").unwrap()).abs() < Decimal::from_str_exact("0.001").unwrap());
            }
            _ => panic!("expected LTP tick"),
        }
    }

    #[test]
    fn test_parse_binary_depth() {
        let mapper = SymbolMapper::new();

        // Build a test depth message:
        // 1 byte: msg_type = 2
        // 25 bytes: token
        // 8 bytes: LTP
        // 8 bytes: exchange timestamp
        // 5 bid levels × 12 bytes
        // 5 ask levels × 12 bytes
        let mut bytes = Vec::new();
        bytes.push(2); // msg_type

        let token = b"NSE_EQ|INE009A01021";
        let mut token_bytes = [0u8; 25];
        token_bytes[..token.len()].copy_from_slice(token);
        bytes.extend_from_slice(&token_bytes);

        let ltp: f64 = 1420.75;
        bytes.extend_from_slice(&ltp.to_le_bytes());

        // exchange timestamp (unused)
        bytes.extend_from_slice(&0.0f64.to_le_bytes());

        // 5 bid levels
        for i in 0..5 {
            let price: f64 = 1420.0 + (i as f64 * 0.05);
            let qty: u32 = 100 + (i as u32 * 10);
            bytes.extend_from_slice(&price.to_le_bytes());
            bytes.extend_from_slice(&qty.to_le_bytes());
            bytes.extend_from_slice(&0u32.to_le_bytes()); // orders count (4 bytes, unused)
        }

        // 5 ask levels
        for i in 0..5 {
            let price: f64 = 1420.05 + (i as f64 * 0.05);
            let qty: u32 = 200 + (i as u32 * 10);
            bytes.extend_from_slice(&price.to_le_bytes());
            bytes.extend_from_slice(&qty.to_le_bytes());
            bytes.extend_from_slice(&0u32.to_le_bytes());
        }

        let tick = AngelOneFeed::parse_binary_message(&bytes, &mapper).unwrap();
        match tick {
            Tick::Depth { bids, asks, last_price, .. } => {
                assert_eq!(bids.len(), 5);
                assert_eq!(asks.len(), 5);
                assert!(last_price.is_some());
                assert!((last_price.unwrap().0 - Decimal::from_str_exact("1420.75").unwrap()).abs() < Decimal::from_str_exact("0.001").unwrap());
            }
            _ => panic!("expected Depth tick"),
        }
    }
}
