//! Unit tests for `LiveQuoteStore`: a writable latest-quote overlay on a `DatasetReader`.

use std::sync::Arc;

use honba_core::{Exchange, InstrumentId};

use crate::ports::QuoteReader;
use crate::wire::{Bar, BarAggregation, BarSpecification, BarType, PriceType, QuoteTick, UnixNanos};

use crate::{ColumnarSliceBuilder, Dataset, DatasetReader, LiveQuoteStore};

fn id() -> InstrumentId {
    InstrumentId::new(Exchange::Nse, "TCS")
}

fn ts(n: u64) -> UnixNanos {
    UnixNanos::from_u64(n)
}

fn spec() -> BarSpecification {
    BarSpecification::new(1, BarAggregation::Minute, PriceType::Last)
}

fn store() -> LiveQuoteStore {
    let mut b = ColumnarSliceBuilder::new(id(), spec());
    b.push(ts(10), 100.0, 100.0, 100.0, 100.0, 1.0).unwrap();
    b.push(ts(20), 110.0, 110.0, 110.0, 110.0, 1.0).unwrap();
    let dataset = Dataset::from_slices(vec![b.finish().unwrap()]).unwrap();
    LiveQuoteStore::new(Arc::new(DatasetReader::new(dataset, vec![])))
}

fn quote(px: f64, t: u64) -> QuoteTick {
    QuoteTick::new(id(), px, px, 0.0, 0.0, ts(t), ts(t))
}

#[tokio::test]
async fn empty_store_falls_back_to_the_dataset() {
    let q = store().read_quote(&id(), None).await.unwrap().unwrap();
    assert_eq!(q.bid_price(), 110.0);
    assert_eq!(q.ts_event(), ts(20));
}

#[tokio::test]
async fn latest_put_wins_and_older_is_ignored() {
    let s = store();
    s.put_quote(quote(150.0, 50));
    s.put_quote(quote(120.0, 30));
    let q = s.read_quote(&id(), None).await.unwrap().unwrap();
    assert_eq!((q.bid_price(), q.ts_event()), (150.0, ts(50)));
    s.put_quote(quote(160.0, 60));
    let q = s.read_quote(&id(), None).await.unwrap().unwrap();
    assert_eq!(q.bid_price(), 160.0);
}

#[tokio::test]
async fn as_of_delegates_to_the_dataset() {
    let s = store();
    s.put_quote(quote(150.0, 50));
    let q = s.read_quote(&id(), Some(ts(15))).await.unwrap().unwrap();
    assert_eq!((q.bid_price(), q.ts_event()), (100.0, ts(10)));
}

#[tokio::test]
async fn put_bar_stores_a_close_quote_with_zero_sizes() {
    let s = store();
    let t = ts(90);
    let bt = BarType::new(id(), spec());
    s.put_bar(&Bar::new(bt, 1.0, 9.0, 1.0, 7.5, 5.0, t, t));
    let q = s.read_quote(&id(), None).await.unwrap().unwrap();
    assert_eq!((q.bid_price(), q.ask_price()), (7.5, 7.5));
    assert_eq!((q.bid_size(), q.ask_size()), (0.0, 0.0));
    assert_eq!((q.ts_event(), q.ts_init()), (t, t));
}
