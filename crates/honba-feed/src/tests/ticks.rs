use honba_messages::{
    AggressorSide, Exchange, InstrumentId, QuoteTick, TradeId, TradeTick, UnixNanos,
};
use honba_ports::{PortError, TickReader, TickRequest};

use crate::InMemoryTickReader;

fn id(s: &str) -> InstrumentId {
    InstrumentId::new(s, Exchange::new("NSE"))
}

fn ts(n: u64) -> UnixNanos {
    UnixNanos::from_u64(n)
}

fn q(s: &str, bid: f64, t: u64) -> QuoteTick {
    QuoteTick::new(id(s), bid, bid + 1.0, 10.0, 10.0, ts(t), ts(t))
}

fn tr(s: &str, px: f64, t: u64) -> TradeTick {
    TradeTick::new(
        id(s),
        px,
        1.0,
        AggressorSide::Buyer,
        TradeId::new(format!("T{t}")),
        ts(t),
        ts(t),
    )
}

fn reader() -> InMemoryTickReader {
    InMemoryTickReader::new()
        .with_quotes(vec![
            q("TCS", 3.0, 30),
            q("TCS", 1.0, 10),
            q("INFY", 9.0, 20),
        ])
        .with_trades(vec![tr("TCS", 5.0, 20), tr("TCS", 6.0, 40)])
}

#[tokio::test]
async fn quotes_are_filtered_by_instrument_and_sorted_by_time() {
    let req = TickRequest::new(id("TCS"), None, None).unwrap();
    let got = reader().read_quotes(&req).await.unwrap();
    let times: Vec<u64> = got.iter().map(|x| x.ts_event().as_u64()).collect();
    assert_eq!(times, [10, 30]);
}

#[tokio::test]
async fn the_range_is_half_open() {
    let req = TickRequest::new(id("TCS"), Some(ts(20)), Some(ts(40))).unwrap();
    let r = reader();
    assert_eq!(r.read_trades(&req).await.unwrap().len(), 1);
    assert_eq!(r.read_quotes(&req).await.unwrap().len(), 1);
}

#[tokio::test]
async fn an_unknown_instrument_is_empty_not_an_error() {
    let req = TickRequest::new(id("WIPRO"), None, None).unwrap();
    assert!(reader().read_quotes(&req).await.unwrap().is_empty());
}

#[tokio::test]
async fn an_empty_store_holds_no_tick_data_and_says_so() {
    let req = TickRequest::new(id("TCS"), None, None).unwrap();
    let err = InMemoryTickReader::new()
        .read_quotes(&req)
        .await
        .unwrap_err();
    assert!(matches!(err, PortError::Unsupported(_)));
}
