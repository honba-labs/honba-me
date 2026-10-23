//! Dhan WebSocket feed adapter.

use async_trait::async_trait;
use futures::stream::BoxStream;
use std::sync::Arc;
use tokio::sync::RwLock;

use honba_core::{InstrumentId, Tick};
use honba_me_core::{
    CoalescingTickBuffer, FeedHealth, MarketDataFeed, MdError, ReconnectionManager, Subscription,
    SymbolMapper, WsClient,
};

/// Dhan Feed adapter.
pub struct DhanFeed {
    pub client_id: String,
    pub access_token: String,
    pub ws_client: WsClient,
    pub mapper: Arc<RwLock<SymbolMapper>>,
    pub reconnection: Arc<ReconnectionManager>,
}

impl DhanFeed {
    pub const DEFAULT_WS_URL: &'static str = "wss://api.dhan.co/feed";

    pub fn new(client_id: impl Into<String>, access_token: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            access_token: access_token.into(),
            ws_client: WsClient::new(Self::DEFAULT_WS_URL),
            mapper: Arc::new(RwLock::new(SymbolMapper::new())),
            reconnection: Arc::new(ReconnectionManager::new()),
        }
    }
}

#[async_trait]
impl MarketDataFeed for DhanFeed {
    fn name(&self) -> &str {
        "dhan"
    }

    async fn subscribe(
        &self,
        subs: Vec<Subscription>,
    ) -> Result<BoxStream<'static, Tick>, MdError> {
        let (_, rx) = CoalescingTickBuffer::new(5000);
        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Box::pin(stream))
    }

    async fn unsubscribe(&self, _instruments: &[InstrumentId]) -> Result<(), MdError> {
        Ok(())
    }

    async fn fetch_snapshot(&self, _instruments: &[InstrumentId]) -> Result<Vec<Tick>, MdError> {
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
