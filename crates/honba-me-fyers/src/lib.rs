//! Fyers Data Socket feed adapter and quote packet parser.

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

/// Fyers Data Socket feed adapter.
pub struct FyersFeed {
    /// Fyers app ID
    pub app_id: String,
    /// Access token
    pub access_token: String,
    /// WebSocket feed URL
    pub ws_url: String,
    /// Symbol mapper
    pub mapper: Arc<RwLock<SymbolMapper>>,
    /// Reconnection and subscription manager
    pub reconnection: Arc<ReconnectionManager>,
}

impl FyersFeed {
    /// Default Fyers WebSocket data socket endpoint.
    pub const DEFAULT_WS_URL: &'static str = "wss://socket.fyers.in/data/v3";

    /// Create a new Fyers feed adapter.
    pub fn new(app_id: impl Into<String>, access_token: impl Into<String>) -> Self {
        Self::with_ws_url(app_id, access_token, Self::DEFAULT_WS_URL)
    }

    /// Create adapter with custom WebSocket URL.
    pub fn with_ws_url(
        app_id: impl Into<String>,
        access_token: impl Into<String>,
        ws_url: impl Into<String>,
    ) -> Self {
        Self {
            app_id: app_id.into(),
            access_token: access_token.into(),
            ws_url: ws_url.into(),
            mapper: Arc::new(RwLock::new(SymbolMapper::new())),
            reconnection: Arc::new(ReconnectionManager::new()),
        }
    }

    /// Parse Fyers quote payload into canonical [`Tick`].
    pub fn parse_quote_json(json_str: &str, mapper: &SymbolMapper) -> Result<Tick, MdError> {
        let parsed: FyersQuote = serde_json::from_str(json_str)
            .map_err(|e| MdError::Transport(format!("invalid fyers quote json: {e}")))?;

        let inst = mapper.to_canonical(&parsed.symbol).unwrap_or_else(|| {
            InstrumentId::new(Exchange::Nse, parsed.symbol.trim_end_matches("-EQ"))
        });

        let price_dec = Decimal::from_str_exact(&parsed.ltp.to_string()).unwrap_or(Decimal::ZERO);
        let vol_dec =
            Decimal::from_str_exact(&parsed.vol_traded_today.to_string()).unwrap_or(Decimal::ZERO);

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
struct FyersQuote {
    symbol: String,
    ltp: f64,
    vol_traded_today: u64,
}

#[async_trait]
impl MarketDataFeed for FyersFeed {
    fn name(&self) -> &str {
        "fyers"
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
    fn test_parse_fyers_quote_json() {
        let mapper = SymbolMapper::new();
        let json = r#"{"symbol": "NSE:RELIANCE-EQ", "ltp": 2480.35, "vol_traded_today": 340000}"#;
        let tick = FyersFeed::parse_quote_json(json, &mapper).unwrap();

        match tick {
            Tick::Ltp {
                instrument,
                price,
                volume,
                ..
            } => {
                assert_eq!(instrument.exchange, Exchange::Nse);
                assert_eq!(instrument.symbol, "RELIANCE");
                assert_eq!(price.0, Decimal::from_str_exact("2480.35").unwrap());
                assert_eq!(volume.0, Decimal::from(340000));
            }
            _ => panic!("expected LTP tick"),
        }
    }
}
