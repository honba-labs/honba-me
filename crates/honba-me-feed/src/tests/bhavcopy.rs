//! Unit tests for the NSE bhavcopy connector (E10-S8 / ADR 0028).

use std::io::BufReader;

use honba_core::Exchange;

use crate::ports::{ConnectorState, MarketDataConnector, MarketEvent, SubscriptionParams};
use crate::wire::{BarAggregation, PriceType};

use crate::bhavcopy::BhavcopyConnector;

/// 2024-01-02 09:15 IST == 2024-01-02T03:45:00Z (Python `test_nse_bhavcopy_tz.py`).
const EPOCH_NS_2024_01_02_0915_IST: u64 = 1_704_167_100 * 1_000_000_000;

const UDIFF: &str = "TradDt,BizDt,Sgmt,Src,FinInstrmTp,FinInstrmId,ISIN,TckrSymb,SctySrs,XpryDt,FininstrmActlXpryDt,StrkPric,OptnTp,FinInstrmNm,OpnPric,HghPric,LwPric,ClsPric,LastPric,PrvsClsgPric,UndrlygPric,SttlmPric,OpnIntrst,ChngInOpnIntrst,TtlTradgVol,TtlTrfVal,TtlNbOfTxsExctd,SsnId,NewBrdLotQty,Rmks,Rsvd1,Rsvd2,Rsvd3,Rsvd4\n\
2024-10-01,2024-10-01,CM,NSE,STK,1,INE002A01018,RELIANCE,EQ,,,,,,2900.00,2950.00,2890.00,2940.00,2940.00,2895.00,,,,,5000000,14700000000,120000,1,1,,,,\n\
2024-10-01,2024-10-01,CM,NSE,STK,2,INE467B01029,TCS,EQ,,,,,,4200.00,4250.00,4180.00,4230.00,4230.00,4190.00,,,,,3000000,12690000000,90000,1,1,,,,\n";

const SEC: &str = "SYMBOL, SERIES, DATE1, PREV_CLOSE, OPEN_PRICE, HIGH_PRICE, LOW_PRICE, LAST_PRICE, CLOSE_PRICE, AVG_PRICE, TTL_TRD_QNTY, TURNOVER_LACS, NO_OF_TRADES, DELIV_QTY, DELIV_PER\n\
RELIANCE, EQ, 01-Oct-2024, 2895.00, 2900.00, 2950.00, 2890.00, 2940.00, 2940.00, 2920.00, 5000000, 146000.00, 120000, 2500000, 50.00\n\
TCS, EQ, 01-Oct-2024, 4190.00, 4200.00, 4250.00, 4180.00, 4230.00, 4230.00, 4210.00, 3000000, 126300.00, 90000, 1500000, 50.00\n";

async fn connector(csv: &str) -> BhavcopyConnector {
    BhavcopyConnector::from_csv(BufReader::new(csv.as_bytes()), Exchange::Nse)
        .expect("fixture parses")
}

async fn drain_all(connector: &mut BhavcopyConnector) -> Vec<crate::wire::Bar> {
    connector
        .subscribe(SubscriptionParams::new(vec![]))
        .await
        .expect("subscribed");
    let mut bars = Vec::new();
    while let Some(MarketEvent::Bar(bar)) = connector.next_event().await.expect("no error") {
        bars.push(bar);
    }
    bars
}

#[tokio::test]
async fn bhavcopy_connector_parses_fixture() {
    for csv in [UDIFF, SEC] {
        let mut connector = connector(csv).await;
        let bars = drain_all(&mut connector).await;
        assert_eq!(bars.len(), 2, "one bar per equity row across both shapes");

        let reliance = bars
            .iter()
            .find(|b| b.bar_type().instrument_id().symbol == "RELIANCE")
            .expect("RELIANCE present");
        assert_eq!(reliance.open(), 2900.0);
        assert_eq!(reliance.high(), 2950.0);
        assert_eq!(reliance.low(), 2890.0);
        assert_eq!(reliance.close(), 2940.0);
        assert_eq!(reliance.volume(), 5_000_000.0);

        let tcs = bars
            .iter()
            .find(|b| b.bar_type().instrument_id().symbol == "TCS")
            .expect("TCS present");
        assert_eq!(tcs.open(), 4200.0);
        assert_eq!(tcs.high(), 4250.0);
        assert_eq!(tcs.low(), 4180.0);
        assert_eq!(tcs.close(), 4230.0);
        assert_eq!(tcs.volume(), 3_000_000.0);

        let spec = reliance.bar_type().spec();
        assert_eq!(spec.aggregation(), BarAggregation::Day);
        assert_eq!(spec.price_type(), PriceType::Last);
        assert_eq!(
            reliance.bar_type().instrument_id().exchange.as_str(),
            "NSE"
        );
    }
}

#[tokio::test]
async fn bar_ts_is_ist_open_independent_of_local_tz() {
    const SEC_ONE: &str = "SYMBOL, SERIES, DATE1, OPEN_PRICE, HIGH_PRICE, LOW_PRICE, CLOSE_PRICE, TTL_TRD_QNTY\n\
RELIANCE, EQ, 02-Jan-2024, 100.0, 110.0, 90.0, 105.0, 1000\n";

    let mut connector = connector(SEC_ONE).await;
    assert_eq!(
        connector.state(),
        ConnectorState::Disconnected {
            reason: "not subscribed".into()
        }
    );
    let bars = drain_all(&mut connector).await;
    assert_eq!(bars.len(), 1);
    assert_eq!(bars[0].ts_event().as_u64(), EPOCH_NS_2024_01_02_0915_IST);
}
