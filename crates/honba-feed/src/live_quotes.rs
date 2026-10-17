//! A writable latest-quote store layered over a [`DatasetReader`].
//!
//! [`LiveQuoteStore`] is a [`QuoteReader`] whose "latest" answer can be refreshed from a live
//! feed ([`LiveQuoteStore::put_quote`], [`LiveQuoteStore::put_bar`]). A query with `as_of`
//! keeps replay semantics and is answered by the fallback dataset; only the latest-quote query
//! (`as_of = None`) sees live data, and falls back to the dataset until the first write.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use honba_messages::{Bar, InstrumentId, QuoteTick, UnixNanos};
use honba_ports::{PortResult, QuoteReader};

use crate::DatasetReader;

/// Latest live quotes per instrument, with a [`DatasetReader`] fallback.
#[derive(Debug)]
pub struct LiveQuoteStore {
    live: RwLock<HashMap<InstrumentId, QuoteTick>>,
    fallback: Arc<DatasetReader>,
}

impl LiveQuoteStore {
    /// Creates an empty store answering from `fallback` until quotes are put.
    pub fn new(fallback: Arc<DatasetReader>) -> Self {
        Self {
            live: RwLock::new(HashMap::new()),
            fallback,
        }
    }

    /// Records `quote` unless the held quote for its instrument has a later `ts_event`.
    /// An equal `ts_event` replaces (the newer write wins).
    pub fn put_quote(&self, quote: QuoteTick) {
        let mut live = self.live.write().unwrap_or_else(|e| e.into_inner());
        match live.get(quote.instrument_id()) {
            Some(held) if held.ts_event() > quote.ts_event() => {}
            _ => {
                live.insert(quote.instrument_id().clone(), quote);
            }
        }
    }

    /// Records the quote a bar implies: bid and ask are the close, sizes zero, both
    /// timestamps the bar's `ts_event` (the same derivation [`DatasetReader`] uses).
    pub fn put_bar(&self, bar: &Bar) {
        let close = bar.close();
        self.put_quote(QuoteTick::new(
            bar.bar_type().instrument_id().clone(),
            close,
            close,
            0.0,
            0.0,
            bar.ts_event(),
            bar.ts_event(),
        ));
    }
}

#[async_trait]
impl QuoteReader for LiveQuoteStore {
    async fn read_quote(
        &self,
        id: &InstrumentId,
        as_of: Option<UnixNanos>,
    ) -> PortResult<Option<QuoteTick>> {
        if as_of.is_none() {
            let held = self
                .live
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .get(id)
                .cloned();
            if held.is_some() {
                return Ok(held);
            }
        }
        self.fallback.read_quote(id, as_of).await
    }
}
