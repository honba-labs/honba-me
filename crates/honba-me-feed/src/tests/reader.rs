//! Unit tests for `DatasetReader`: the two read ports over an in-memory dataset.

use honba_entities::{Currency, Instrument, InstrumentKind};
use honba_messages::{
    BarAggregation, BarSpecification, Exchange, InstrumentId, PriceType, UnixNanos,
};
use honba_ports::{BarReader, BarRequest, DepthReader, InstrumentMaster, PortError, QuoteReader};

use crate::{ColumnarSliceBuilder, Dataset, DatasetReader};

fn id(symbol: &str) -> InstrumentId {
    InstrumentId::new(symbol, Exchange::new("NSE"))
}

fn minute() -> BarSpecification {
    BarSpecification::new(1, BarAggregation::Minute, PriceType::Last)
}

fn daily() -> BarSpecification {
    BarSpecification::new(1, BarAggregation::Day, PriceType::Last)
}

fn ts(n: u64) -> UnixNanos {
    UnixNanos::from_u64(n)
}

fn slice(symbol: &str, stamps: &[u64]) -> crate::ColumnarSlice {
    let mut builder = ColumnarSliceBuilder::new(id(symbol), minute());
    for (i, t) in stamps.iter().enumerate() {
        let px = 100.0 + i as f64;
        builder
            .push(ts(*t), px, px + 1.0, px - 1.0, px, 10.0)
            .unwrap();
    }
    builder.finish().unwrap()
}

fn reader() -> DatasetReader {
    let dataset =
        Dataset::from_slices(vec![slice("TCS", &[10, 20, 30, 40]), slice("INFY", &[10])]).unwrap();
    DatasetReader::from_dataset(dataset)
}

fn request(symbol: &str, from: Option<u64>, to: Option<u64>) -> BarRequest {
    BarRequest::new(id(symbol), minute(), from.map(ts), to.map(ts)).unwrap()
}

fn stamps(bars: &[honba_messages::Bar]) -> Vec<u64> {
    bars.iter().map(|b| b.ts_event().as_u64()).collect()
}

#[tokio::test]
async fn listing_is_complete_and_ordered_by_instrument_id() {
    let listed = reader().list_instruments().await.unwrap();
    let ids: Vec<&str> = listed.iter().map(|i| i.id().symbol()).collect();
    assert_eq!(ids, vec!["INFY", "TCS"]);
}

#[tokio::test]
async fn derived_metadata_is_documented_defaults() {
    let got = reader().get_instrument(&id("TCS")).await.unwrap().unwrap();
    assert_eq!(got.kind(), InstrumentKind::Equity);
    assert_eq!(got.currency(), Currency::Inr);
    assert_eq!(got.lot_size(), 1.0);
}

#[tokio::test]
async fn explicit_metadata_replaces_the_defaults() {
    let nifty = Instrument::new(id("TCS"), InstrumentKind::Future, Currency::Usd, 75.0, 0.5);
    let dataset = Dataset::from_slices(vec![slice("TCS", &[1])]).unwrap();
    let reader = DatasetReader::new(dataset, vec![nifty.clone()]);
    assert_eq!(
        reader.get_instrument(&id("TCS")).await.unwrap(),
        Some(nifty)
    );
}

#[tokio::test]
async fn an_unknown_instrument_is_none_not_an_error() {
    assert_eq!(reader().get_instrument(&id("NOPE")).await.unwrap(), None);
}

#[tokio::test]
async fn an_unbounded_read_returns_every_bar_in_ascending_order() {
    let bars = reader()
        .read_bars(&request("TCS", None, None))
        .await
        .unwrap();
    assert_eq!(stamps(&bars), vec![10, 20, 30, 40]);
}

#[tokio::test]
async fn from_is_inclusive_and_to_is_exclusive() {
    let bars = reader()
        .read_bars(&request("TCS", Some(20), Some(40)))
        .await
        .unwrap();
    assert_eq!(stamps(&bars), vec![20, 30]);
}

#[tokio::test]
async fn a_range_past_the_data_is_empty_not_an_error() {
    let bars = reader()
        .read_bars(&request("TCS", Some(500), None))
        .await
        .unwrap();
    assert!(bars.is_empty());
}

#[tokio::test]
async fn an_unknown_instrument_has_no_bars() {
    let bars = reader()
        .read_bars(&request("NOPE", None, None))
        .await
        .unwrap();
    assert!(bars.is_empty());
}

#[tokio::test]
async fn a_timeframe_the_data_does_not_hold_is_unsupported() {
    let req = BarRequest::new(id("TCS"), daily(), None, None).unwrap();
    let err = reader().read_bars(&req).await.unwrap_err();
    assert!(matches!(err, PortError::Unsupported(_)), "{err:?}");
}

#[tokio::test]
async fn reads_are_repeatable() {
    let r = reader();
    let first = r.read_bars(&request("TCS", None, None)).await.unwrap();
    let second = r.read_bars(&request("TCS", None, None)).await.unwrap();
    assert_eq!(first, second);
}

#[tokio::test]
async fn the_latest_quote_is_the_last_close_with_no_size() {
    let quote = reader()
        .read_quote(&id("TCS"), None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(quote.bid_price(), 103.0);
    assert_eq!(quote.ask_price(), 103.0);
    assert_eq!((quote.bid_size(), quote.ask_size()), (0.0, 0.0));
    assert_eq!(quote.ts_event(), ts(40));
    assert_eq!(quote.ts_init(), ts(40));
}

#[tokio::test]
async fn an_as_of_quote_is_inclusive_and_never_sees_later_bars() {
    let r = reader();
    let tcs = id("TCS");
    let at = |t| r.read_quote(&tcs, Some(ts(t)));
    assert_eq!(at(20).await.unwrap().unwrap().bid_price(), 101.0);
    assert_eq!(at(29).await.unwrap().unwrap().bid_price(), 101.0);
    assert_eq!(at(999).await.unwrap().unwrap().bid_price(), 103.0);
}

#[tokio::test]
async fn no_quote_before_the_first_bar_or_for_an_unknown_instrument() {
    let r = reader();
    assert_eq!(r.read_quote(&id("TCS"), Some(ts(9))).await.unwrap(), None);
    assert_eq!(r.read_quote(&id("NOPE"), None).await.unwrap(), None);
}

#[tokio::test]
async fn a_bar_store_has_no_depth_and_says_so() {
    let err = reader().read_depth(&id("TCS"), 5).await.unwrap_err();
    assert!(matches!(err, PortError::Unsupported(_)), "{err:?}");
}
