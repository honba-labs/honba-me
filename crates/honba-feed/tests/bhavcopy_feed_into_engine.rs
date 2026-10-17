//! Integration test: the bhavcopy connector drives an engine run (E10-S8).
//!
//! This reuses the adapter model of
//! `crates/honba-feed/tests/feed_into_engine_run.rs`: each `MarketEvent::Bar`
//! the connector yields is wrapped as a kernel `Message` and dispatched to a
//! [`Handler`]. Unlike that harness the stream is a real bhavcopy, not a
//! script: two subscribed instruments yield exactly the two bars the file
//! contains, the run terminates cleanly at EOF, and the `ConnectorState`
//! progresses `Disconnected -> Connected` and then stays `Connected`.

use std::io::BufReader;
use std::sync::{Arc, Mutex};

use honba_engine::{Engine, EngineOutput, Handler, Result};
use honba_messages::{Event, Exchange, InstrumentId, Message, UnixNanos};
use honba_ports::{ConnectorState, MarketDataConnector, MarketEvent, SubscriptionParams};

use honba_feed::bhavcopy::BhavcopyConnector;

const FIXTURE: &str = "TradDt,BizDt,Sgmt,Src,FinInstrmTp,FinInstrmId,ISIN,TckrSymb,SctySrs,XpryDt,FininstrmActlXpryDt,StrkPric,OptnTp,FinInstrmNm,OpnPric,HghPric,LwPric,ClsPric,LastPric,PrvsClsgPric,UndrlygPric,SttlmPric,OpnIntrst,ChngInOpnIntrst,TtlTradgVol,TtlTrfVal,TtlNbOfTxsExctd,SsnId,NewBrdLotQty,Rmks,Rsvd1,Rsvd2,Rsvd3,Rsvd4\n\
2024-10-01,2024-10-01,CM,NSE,STK,1,INE002A01018,RELIANCE,EQ,,,,,,2900.00,2950.00,2890.00,2940.00,2940.00,2895.00,,,,,5000000,14700000000,120000,1,1,,,,\n\
2024-10-01,2024-10-01,CM,NSE,STK,2,INE467B01029,TCS,EQ,,,,,,4200.00,4250.00,4180.00,4230.00,4230.00,4190.00,,,,,3000000,12690000000,90000,1,1,,,,\n";

fn id(symbol: &str) -> InstrumentId {
    InstrumentId::new(symbol, Exchange::new("NSE"))
}

/// A handler that records the close of every bar it sees.
#[derive(Clone, Default)]
struct Closes(Arc<Mutex<Vec<f64>>>);

struct CloseRecorder {
    closes: Closes,
}

impl Handler for CloseRecorder {
    fn on_event(&mut self, event: &Event, _ts_init: UnixNanos) -> Result<EngineOutput> {
        if let Event::Bar(bar) = event {
            self.closes.0.lock().expect("closes lock").push(bar.close());
        }
        Ok(EngineOutput::None)
    }
}

#[tokio::test]
async fn bhavcopy_feed_into_engine_run() {
    let reliance = id("RELIANCE");
    let tcs = id("TCS");

    let mut connector =
        BhavcopyConnector::from_csv(BufReader::new(FIXTURE.as_bytes()), Exchange::new("NSE"))
            .expect("fixture parses");

    assert_eq!(
        connector.state(),
        ConnectorState::Disconnected {
            reason: "not subscribed".into()
        },
        "an unsubscribed connector is disconnected"
    );

    connector
        .subscribe(SubscriptionParams::new(vec![reliance, tcs]))
        .await
        .expect("subscribed");
    assert_eq!(connector.state(), ConnectorState::Connected);

    let closes = Closes::default();
    let mut engine = Engine::new();
    engine.add_handler(CloseRecorder {
        closes: closes.clone(),
    });
    engine.start().expect("engine started");

    let mut states = Vec::new();
    loop {
        match connector.next_event().await.expect("infallible") {
            Some(MarketEvent::Bar(bar)) => {
                engine.inject(Message::new(Event::Bar(bar), UnixNanos::from_u64(0)));
                while engine.pump().expect("pump") {}
            }
            Some(_) => {}
            None => break,
        }
        states.push(connector.state());
    }
    engine.finish().expect("engine finished");

    let seen = closes.0.lock().expect("closes lock").clone();
    assert_eq!(seen.len(), 2, "the run saw exactly the two subscribed bars");
    assert_eq!(seen[0], 2940.0, "RELIANCE arrives first, in file order");
    assert_eq!(seen[1], 4230.0, "TCS follows");
    assert!(
        states.iter().all(|s| *s == ConnectorState::Connected),
        "the connector stays connected for the whole run: {states:?}"
    );
    assert_eq!(connector.state(), ConnectorState::Connected);
}
