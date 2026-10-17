//! Zerodha market data adapter.
use async_trait::async_trait;
use futures::stream::BoxStream;
use honba_me_core::{MarketDataFeed, MdError, Subscription, Tick};

pub struct ZerodhaFeed {
    pub api_key: String,
    pub access_token: String,
}

impl ZerodhaFeed {
    pub fn new(k: impl Into<String>, t: impl Into<String>) -> Self {
        Self { api_key: k.into(), access_token: t.into() }
    }
}

#[async_trait]
impl MarketDataFeed for ZerodhaFeed {
    async fn subscribe(
        &self,
        _subs: Vec<Subscription>,
    ) -> Result<BoxStream<'_, Tick>, MdError> {
        Err(MdError::Transport("not implemented".into()))
    }
}
