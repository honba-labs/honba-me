//! Market data feed traits and normalized tick types.
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::stream::BoxStream;
use honba_kokoro_types::{InstrumentId, Price, Quantity};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct Tick {
    pub instrument: InstrumentId,
    pub ltp: Price,
    pub volume: Quantity,
    pub bid: Option<Price>,
    pub ask: Option<Price>,
    pub exchange_ts: DateTime<Utc>,
    pub receive_ts: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct Subscription {
    pub instrument: InstrumentId,
    pub mode: Mode,
}

#[derive(Debug, Clone, Copy)]
pub enum Mode { Ltp, Quote, Full }

#[derive(Debug, Error)]
pub enum MdError {
    #[error("auth")]
    Auth,
    #[error("transport: {0}")]
    Transport(String),
}

#[async_trait]
pub trait MarketDataFeed: Send + Sync {
    async fn subscribe(
        &self,
        subs: Vec<Subscription>,
    ) -> Result<BoxStream<'_, Tick>, MdError>;
}
