//! Upstox v2 Market Data feed adapter and JSON quote parser.

use async_trait::async_trait;
use chrono::Utc;
use futures::stream::BoxStream;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;

use honba_core::{Exchange, InstrumentId, Price, Quantity, Tick};
use honba_me_core::{
    CoalescingTickBuffer, MarketDataFeed, MdError, ReconnectionManager, Subscription, SymbolMapper,
};

/// Upstox market data feed adapter.
pub struct UpstoxFeed {
    /// Upstox API key
    pub api_key: String,
    /// OAuth Bearer token
    pub access_token: String,
    /// WebSocket feed URL
    pub ws_url: String,
    /// Symbol mapper
    pub mapper: Arc<RwLock<SymbolMapper>>,
    /// Reconnection and subscription manager
    pub reconnection: Arc<ReconnectionManager>,
}

impl UpstoxFeed {
    /// Default Upstox v2 WebSocket market data endpoint.
    pub const DEFAULT_WS_URL: &'static str = "wss://api.upstox.com/v2/feed/market-data-feed";

    /// Create a new Upstox feed adapter.
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

    /// Parse Upstox JSON quote message into canonical [`Tick`].
    pub fn parse_quote_json(json_str: &str, mapper: &SymbolMapper) -> Result<Tick, MdError> {
        let parsed: UpstoxFeedMessage = serde_json::from_str(json_str)
            .map_err(|e| MdError::Transport(format!("invalid upstox feed json: {e}")))?;

        let inst = mapper
            .to_canonical(&parsed.instrument_key)
            .unwrap_or_else(|| {
                InstrumentId::new(Exchange::Nse, parsed.instrument_key.clone())
                    .with_token(parsed.instrument_key)
            });

        let price_dec = Decimal::from_str_exact(&parsed.ltp.to_string()).unwrap_or(Decimal::ZERO);
        let vol_dec = Decimal::from_str_exact(&parsed.volume.to_string()).unwrap_or(Decimal::ZERO);

        let now = Utc::now();
        Ok(Tick::Ltp {
            instrument: inst,
            price: Price::new(price_dec),
            volume: Quantity::new(vol_dec),
            exchange_ts: now,
            receive_ts: now,
        })
    }
}

#[derive(Debug, Deserialize)]
struct UpstoxFeedMessage {
    instrument_key: String,
    ltp: f64,
    volume: u64,
}

#[async_trait]
impl MarketDataFeed for UpstoxFeed {
    fn name(&self) -> &str {
        "upstox"
    }

    async fn subscribe(
        &self,
        subs: Vec<Subscription>,
    ) -> Result<BoxStream<'static, Tick>, MdError> {
        for s in &subs {
            self.reconnection.add_subscription(s.clone()).await;
        }

        let (_buf, rx) = CoalescingTickBuffer::new(5000);
        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Box::pin(stream))
    }

    async fn unsubscribe(&self, instruments: &[InstrumentId]) -> Result<(), MdError> {
        for inst in instruments {
            self.reconnection.remove_subscription(inst).await;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_upstox_quote_json() {
        let mapper = SymbolMapper::new();
        let json = r#"{"instrument_key": "NSE_EQ|INE009A01021", "ltp": 1420.75, "volume": 125000}"#;
        let tick = UpstoxFeed::parse_quote_json(json, &mapper).unwrap();

        match tick {
            Tick::Ltp {
                instrument,
                price,
                volume,
                ..
            } => {
                assert_eq!(instrument.exchange, Exchange::Nse);
                assert_eq!(instrument.symbol, "INE009A01021");
                assert_eq!(price.0, Decimal::from_str_exact("1420.75").unwrap());
                assert_eq!(volume.0, Decimal::from(125000));
            }
            _ => panic!("expected LTP tick"),
        }
    }
}
