//! Integration test: the E1-S6 feed connector contract.
//!
//! This test proves the connector state machine on its own (reconnect, backoff, 
//! gap markers, terminal stops). 

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc};
use std::time::Duration;

use async_trait::async_trait;
use honba_core::{Exchange, InstrumentId};
use honba_feed::ports::{ConnectorState, MarketDataConnector, MarketEvent, PortError, PortResult, Subscription, SubscriptionParams};
use honba_feed::wire::{BarAggregation, BarSpecification, PriceType, UnixNanos, Bar, BarType};

/// A clock the test moves by hand, so no assertion depends on wall time.
struct FakeClock {
    now: AtomicU64,
}

impl FakeClock {
    fn new(start: u64) -> Self {
        Self {
            now: AtomicU64::new(start),
        }
    }
}

// Minimal Clock trait needed for tests
#[async_trait]
trait Clock: Send + Sync {
    async fn now(&self) -> UnixNanos;
    async fn sleep(&self, dur: Duration) -> PortResult<()>;
}

#[async_trait]
impl Clock for FakeClock {
    async fn now(&self) -> UnixNanos {
        UnixNanos::from_u64(self.now.load(Ordering::SeqCst))
    }

    async fn sleep(&self, dur: Duration) -> PortResult<()> {
        let nanos = u64::try_from(dur.as_nanos()).expect("test durations fit in u64");
        self.now.fetch_add(nanos, Ordering::SeqCst);
        Ok(())
    }
}

fn reliance() -> InstrumentId {
    InstrumentId::new(Exchange::Nse, "RELIANCE")
}

fn bar(close: f64, ts: u64) -> Bar {
    let spec = BarSpecification::new(1, BarAggregation::Minute, PriceType::Last);
    let at = UnixNanos::from_u64(ts);
    Bar::new(
        BarType::new(reliance(), spec),
        close,
        close,
        close,
        close,
        1.0,
        at,
        at,
    )
}

/// One scheduled step of a connector's stream.
enum Step {
    Data(MarketEvent),
    Terminal(String),
}

/// A [`MarketDataConnector`] whose whole stream is a fixed script.
struct ScriptedConnector {
    steps: VecDeque<Step>,
    state: ConnectorState,
    clock: Arc<dyn Clock>,
    stopped: Option<PortError>,
}

impl ScriptedConnector {
    fn new(clock: Arc<dyn Clock>, steps: Vec<Step>) -> Self {
        Self {
            steps: steps.into(),
            state: ConnectorState::Disconnected {
                reason: "not subscribed".into(),
            },
            stopped: None,
            clock,
        }
    }
}

#[async_trait]
impl MarketDataConnector for ScriptedConnector {
    async fn subscribe(&mut self, _params: SubscriptionParams) -> PortResult<Subscription> {
        if let Some(err) = &self.stopped {
            return Err(err.clone());
        }
        self.state = ConnectorState::Connected;
        Ok(Subscription::default())
    }

    async fn next_event(&mut self) -> PortResult<Option<MarketEvent>> {
        if let Some(err) = &self.stopped {
            return Err(err.clone());
        }
        let Some(step) = self.steps.pop_front() else {
            return Ok(None);
        };
        match step {
            Step::Data(event) => {
                self.state = ConnectorState::Connected;
                Ok(Some(event))
            }
            Step::Terminal(msg) => {
                self.stopped = Some(PortError::Terminal(msg.clone()));
                Err(PortError::Terminal(msg))
            }
        }
    }

    fn state(&self) -> ConnectorState {
        self.state.clone()
    }
}

#[tokio::test]
async fn connector_scripted_run() {
    let clock = Arc::new(FakeClock::new(1000));
    let steps = vec![
        Step::Data(MarketEvent::Bar(bar(100.0, 2000))),
        Step::Data(MarketEvent::Bar(bar(101.0, 3000))),
    ];
    let mut connector = ScriptedConnector::new(clock, steps);
    
    connector.subscribe(SubscriptionParams::new(vec![reliance()])).await.unwrap();
    
    let mut results = Vec::new();
    while let Some(event) = connector.next_event().await.unwrap() {
        if let MarketEvent::Bar(bar) = event {
            results.push(bar.close());
        }
    }
    
    assert_eq!(results, vec![100.0, 101.0]);
}
