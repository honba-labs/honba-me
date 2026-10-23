//! Zerodha Kite Ticker WebSocket feed adapter and binary packet parser.

use async_trait::async_trait;
use chrono::Utc;
use futures::stream::BoxStream;
use flate2::read::ZlibDecoder;
use rust_decimal::Decimal;
use std::io::Read;
use std::sync::Arc;
use tokio::sync::RwLock;

use honba_core::{DepthLevel, Exchange, InstrumentId, Price, Quantity, Tick};
use honba_me_core::{
    CoalescingTickBuffer, FeedHealth, MarketDataFeed, MdError, ReconnectionManager, Subscription,
    SymbolMapper, WsClient, WsConfig,
};

/// Zerodha Kite Ticker feed adapter.
pub struct ZerodhaFeed {
    /// Zerodha API key
    pub api_key: String,
    /// Access token
    pub access_token: String,
    /// WebSocket client
    pub ws_client: WsClient,
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
        let ws_url = ws_url.into();
        Self {
            api_key: api_key.into(),
            access_token: access_token.into(),
            ws_client: WsClient::new(ws_url),
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

    /// Parse a 44-byte Kite Full Equity Quote binary packet into canonical [`Tick::Quote`].
    ///
    /// Layout: token(4), ltp(4), last_traded_qty(4), avg_traded_price(4), volume(8),
    /// buy_quantity(4), sell_quantity(4), open(4), high(4), low(4), close(4)
    pub fn parse_binary_quote_44b(bytes: &[u8], mapper: &SymbolMapper) -> Result<Tick, MdError> {
        if bytes.len() < 44 {
            return Err(MdError::Transport("Full quote packet too short".into()));
        }

        let token = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let ltp_raw = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let ltp_dec = Decimal::from(ltp_raw) / Decimal::from(100);
        let last_qty_raw = u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        let avg_price_raw = u32::from_be_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
        let volume_raw = u64::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19], bytes[20], bytes[21], bytes[22], bytes[23]]);
        let buy_qty_raw = u32::from_be_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
        let sell_qty_raw = u32::from_be_bytes([bytes[28], bytes[29], bytes[30], bytes[31]]);
        let open_raw = u32::from_be_bytes([bytes[32], bytes[33], bytes[34], bytes[35]]);
        let high_raw = u32::from_be_bytes([bytes[36], bytes[37], bytes[38], bytes[39]]);
        let low_raw = u32::from_be_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]);

        let token_str = token.to_string();
        let inst = mapper.to_canonical(&token_str).unwrap_or_else(|| {
            InstrumentId::new(Exchange::Nse, format!("TOKEN-{}", token)).with_token(token_str)
        });

        let now = Utc::now();
        Ok(Tick::Quote {
            instrument: inst,
            bid: Price::new(Decimal::from(low_raw) / Decimal::from(100)),
            ask: Price::new(Decimal::from(high_raw) / Decimal::from(100)),
            bid_qty: Quantity::new(Decimal::from(buy_qty_raw)),
            ask_qty: Quantity::new(Decimal::from(sell_qty_raw)),
            last_price: Some(Price::new(ltp_dec)),
            exchange_ts: now,
            receive_ts: now,
            band: None,
        })
    }

    /// Parse a 184-byte Kite Full Market Depth binary packet into canonical [`Tick::Depth`].
    ///
    /// Layout: quote fields (44 bytes), OI(4), day_high(4), day_low(4),
    /// 5 bid levels × 12 bytes (price(4), qty(4), orders(4)),
    /// 5 ask levels × 12 bytes (price(4), qty(4), orders(4))
    pub fn parse_binary_depth_184b(bytes: &[u8], mapper: &SymbolMapper) -> Result<Tick, MdError> {
        if bytes.len() < 184 {
            return Err(MdError::Transport("Depth packet too short".into()));
        }

        // Parse quote portion (first 44 bytes)
        let token = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let ltp_raw = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let ltp_dec = Decimal::from(ltp_raw) / Decimal::from(100);
        let volume_raw = u64::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19], bytes[20], bytes[21], bytes[22], bytes[23]]);

        let token_str = token.to_string();
        let inst = mapper.to_canonical(&token_str).unwrap_or_else(|| {
            InstrumentId::new(Exchange::Nse, format!("TOKEN-{}", token)).with_token(token_str)
        });

        // Parse 5 bid levels (offset 56 after quote header + OI + day_high + day_low)
        let mut bids = Vec::with_capacity(5);
        for i in 0..5 {
            let offset = 56 + i * 12;
            let price_raw = u32::from_be_bytes([bytes[offset], bytes[offset+1], bytes[offset+2], bytes[offset+3]]);
            let qty_raw = u32::from_be_bytes([bytes[offset+4], bytes[offset+5], bytes[offset+6], bytes[offset+7]]);
            let price = Price::new(Decimal::from(price_raw) / Decimal::from(100));
            let qty = Quantity::new(Decimal::from(qty_raw));
            bids.push(DepthLevel { price, quantity: qty, orders_count: None });
        }

        // Parse 5 ask levels
        let mut asks = Vec::with_capacity(5);
        for i in 0..5 {
            let offset = 116 + i * 12;
            let price_raw = u32::from_be_bytes([bytes[offset], bytes[offset+1], bytes[offset+2], bytes[offset+3]]);
            let qty_raw = u32::from_be_bytes([bytes[offset+4], bytes[offset+5], bytes[offset+6], bytes[offset+7]]);
            let price = Price::new(Decimal::from(price_raw) / Decimal::from(100));
            let qty = Quantity::new(Decimal::from(qty_raw));
            asks.push(DepthLevel { price, quantity: qty, orders_count: None });
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

    /// Unwraps Kite Ticker frames (2-byte length prefix).
    pub fn unwrap_frames(data: &[u8]) -> Vec<&[u8]> {
        let mut frames = Vec::new();
        let mut pos = 0;
        while pos + 2 <= data.len() {
            let len = u16::from_be_bytes([data[pos], data[pos+1]]) as usize;
            pos += 2;
            if pos + len <= data.len() {
                frames.push(&data[pos..pos + len]);
                pos += len;
            } else {
                break;
            }
        }
        frames
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
        
        // Spawn a background task to process WebSocket messages
        let (msg_tx, mut msg_rx) = tokio::sync::mpsc::unbounded_channel();
        let mapper = self.mapper.clone();
        
        let buf_tx = buf.clone();

        tokio::spawn(async move {
            while let Some(msg) = msg_rx.recv().await {
                if msg.is_binary {
                    // Zerodha packets might be compressed; check if first byte indicates it
                    let data = if msg.payload.first() == Some(&0x78) { // zlib header
                        match Self::decompress_zlib(&msg.payload) {
                            Ok(d) => d,
                            Err(_) => continue,
                        }
                    } else {
                        msg.payload.clone()
                    };

                    let frames = Self::unwrap_frames(&data);
                    let mapper = mapper.read().await;

                    for frame in frames {
                        let tick = match frame.len() {
                            8 => Self::parse_binary_ltp(frame, &mapper),
                            28 => Self::parse_binary_quote(frame, &mapper),
                            44 => Self::parse_binary_quote_44b(frame, &mapper),
                            184 => Self::parse_binary_depth_184b(frame, &mapper),
                            _ => continue, // Unknown packet size
                        };
                        
                        if let Ok(tick) = tick {
                            let _ = buf_tx.send(tick).await;
                        }
                    }
                }
            }
        });

        self.ws_client.connect(msg_tx).await.map_err(|e| MdError::Transport(e.to_string()))?;
        
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
