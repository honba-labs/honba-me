//! Integration test: the bhavcopy connector drives a data feed run.
//!
//! Each `MarketEvent::Bar` the connector yields is processed as a canonical
//! `Tick` or `Bar` in the new `honba_me_feed` port API. The run terminates cleanly at EOF.

use std::io::BufReader;
use honba_core::{Exchange, InstrumentId};
use honba_feed::bhavcopy::BhavcopyConnector;
use honba_feed::ports::{MarketDataConnector, MarketEvent, SubscriptionParams};

const FIXTURE: &str = "TradDt,BizDt,Sgmt,Src,FinInstrmTp,FinInstrmId,ISIN,TckrSymb,SctySrs,XpryDt,FininstrmActlXpryDt,StrkPric,OptnTp,FinInstrmNm,OpnPric,HghPric,LwPric,ClsPric,LastPric,PrvsClsgPric,UndrlygPric,SttlmPric,OpnIntrst,ChngInOpnIntrst,TtlTradgVol,TtlTrfVal,TtlNbOfTxsExctd,SsnId,NewBrdLotQty,Rmks,Rsvd1,Rsvd2,Rsvd3,Rsvd4\n\
2024-10-01,2024-10-01,CM,NSE,STK,1,INE002A01018,RELIANCE,EQ,,,,,,2900.00,2950.00,2890.00,2940.00,2940.00,2895.00,,,,,5000000,14700000000,120000,1,1,,,,\n\
2024-10-01,2024-10-01,CM,NSE,STK,2,INE467B01029,TCS,EQ,,,,,,4200.00,4250.00,4180.00,4230.00,4230.00,4190.00,,,,,3000000,12690000000,90000,1,1,,,,\n";

fn id(symbol: &str) -> InstrumentId {
    InstrumentId::new(Exchange::Nse, symbol)
}

#[tokio::test]
async fn bhavcopy_feed_run() {
    let reliance = id("RELIANCE");
    let tcs = id("TCS");

    let mut connector =
        BhavcopyConnector::from_csv(BufReader::new(FIXTURE.as_bytes()), Exchange::Nse)
            .expect("fixture parses");

    connector
        .subscribe(SubscriptionParams::new(vec![reliance, tcs]))
        .await
        .expect("subscribed");

    let mut seen_closes = Vec::new();
    loop {
        match connector.next_event().await.expect("infallible") {
            Some(MarketEvent::Bar(bar)) => {
                seen_closes.push(bar.close());
            }
            Some(_) => {}
            None => break,
        }
    }

    assert_eq!(seen_closes.len(), 2, "the run saw exactly the two subscribed bars");
    // Values from the CSV fixture
    assert_eq!(seen_closes[0], 2940.0, "RELIANCE arrives first, in file order");
    assert_eq!(seen_closes[1], 4230.0, "TCS follows");
}
