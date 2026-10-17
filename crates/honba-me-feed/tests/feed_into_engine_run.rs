//! Integration test: the E1-S6 feed connector contract drives an engine run.
//!
//! `crates/honba-ports/tests/connector_contract.rs` proves the connector state
//! machine on its own (reconnect, backoff, gap markers, terminal stops). This
//! test proves the contract is usable where it will actually be used: a real
//! [`Engine`] run. Every data event the connector yields is injected as a kernel
//! [`Message`] and dispatched to a handler, a [`MarketEvent::GapMarker`]
//! surfaces in-band without breaking the run, and a [`PortError::Terminal`]
//! stops the loop for good. The clock is a hand-moved fake, so nothing here
//! touches wall time and the assertion values are exact.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use honba_engine::{AuditKind, AuditRecord, Engine, EngineOutput, Handler, Result};
use honba_messages::{
    Bar, BarAggregation, BarSpecification, BarType, Event, Exchange, InstrumentId, Message,
    PriceType, UnixNanos,
};
use honba_ports::{
    Clock, ConnectorState, ExponentialBackoff, FeedWatchdog, MarketDataConnector, MarketEvent,
    PortError, PortResult, Subscription, SubscriptionParams,
};

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
    InstrumentId::new("RELIANCE", Exchange::new("NSE"))
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

fn event_ts(event: &MarketEvent) -> Option<u64> {
    match event {
        MarketEvent::Bar(bar) => Some(bar.ts_event().as_u64()),
        MarketEvent::QuoteTick(tick) => Some(tick.ts_event().as_u64()),
        MarketEvent::TradeTick(tick) => Some(tick.ts_event().as_u64()),
        _ => None,
    }
}

/// One scheduled step of a connector's stream.
enum Step {
    /// A data event delivered normally.
    Data(MarketEvent),
    /// Silence for `nanos`, then the watchdog trips and a gap marker is emitted.
    Drop(u64),
    /// A permanent failure that ends the session for good.
    Terminal(String),
}

/// A [`MarketDataConnector`] whose whole stream is a fixed script.
struct ScriptedConnector {
    steps: VecDeque<Step>,
    state: ConnectorState,
    backoff: ExponentialBackoff,
    attempts: u32,
    clock: Arc<dyn Clock>,
    watchdog: FeedWatchdog,
    last_seen: UnixNanos,
    stopped: Option<PortError>,
}

impl ScriptedConnector {
    fn new(clock: Arc<dyn Clock>, steps: Vec<Step>) -> Self {
        Self {
            steps: steps.into(),
            state: ConnectorState::Disconnected {
                reason: "not subscribed".into(),
            },
            backoff: ExponentialBackoff::new(100, 1_000, 0.0, 7),
            attempts: 0,
            watchdog: FeedWatchdog::new(1_000, Arc::clone(&clock)),
            last_seen: UnixNanos::from_u64(0),
            stopped: None,
            clock,
        }
    }
}

#[async_trait]
impl MarketDataConnector for ScriptedConnector {
    async fn subscribe(&mut self, params: SubscriptionParams) -> PortResult<Subscription> {
        if let Some(err) = &self.stopped {
            return Err(err.clone());
        }
        self.last_seen = self.clock.now().await;
        self.state = ConnectorState::Connected;
        Ok(Subscription::new(params.symbols().to_vec()))
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
                if let Some(ts) = event_ts(&event) {
                    self.last_seen = UnixNanos::from_u64(ts);
                }
                Ok(Some(event))
            }
            Step::Drop(nanos) => {
                self.clock.sleep(Duration::from_nanos(nanos)).await?;
                match self.watchdog.check(&reliance(), self.last_seen).await {
                    Some(marker) => {
                        self.attempts += 1;
                        let backoff_ms = self.backoff.next(self.attempts).as_millis() as u64;
                        self.state = ConnectorState::Reconnecting {
                            attempt: self.attempts,
                            backoff_ms,
                        };
                        Ok(Some(marker))
                    }
                    None => {
                        self.state = ConnectorState::Connected;
                        Ok(None)
                    }
                }
            }
            Step::Terminal(reason) => {
                let err = PortError::Terminal(reason);
                self.state = ConnectorState::Disconnected {
                    reason: err.to_string(),
                };
                self.stopped = Some(err.clone());
                Err(err)
            }
        }
    }

    fn state(&self) -> ConnectorState {
        self.state.clone()
    }
}

/// A handler that records the `ts_event` of every bar it sees.
#[derive(Clone, Default)]
struct Bars(Arc<Mutex<Vec<u64>>>);

impl Bars {
    fn seen(&self) -> Vec<u64> {
        self.0.lock().expect("bars lock").clone()
    }
}

/// A [`Handler`] that reads bars into a shared [`Bars`] log.
struct BarRecorder {
    bars: Bars,
}

impl Handler for BarRecorder {
    fn on_event(&mut self, event: &Event, _ts_init: UnixNanos) -> Result<EngineOutput> {
        if let Event::Bar(bar) = event {
            self.bars
                .0
                .lock()
                .expect("bars lock")
                .push(bar.ts_event().as_u64());
        }
        Ok(EngineOutput::None)
    }
}

fn dispatched(audit: &[AuditRecord]) -> Vec<u64> {
    audit
        .iter()
        .filter_map(|record| match record.kind {
            AuditKind::EventDispatched { ts_event } => Some(ts_event),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn feed_into_engine_run() {
    let clock: Arc<dyn Clock> = Arc::new(FakeClock::new(0));
    let mut connector = ScriptedConnector::new(
        clock,
        vec![
            Step::Data(MarketEvent::Bar(bar(100.0, 100))),
            Step::Data(MarketEvent::Bar(bar(101.0, 200))),
            Step::Drop(5_000),
            Step::Data(MarketEvent::Bar(bar(102.0, 6_000))),
            Step::Terminal("session revoked".into()),
        ],
    );

    let bars = Bars::default();
    let mut engine = Engine::new();
    engine.add_handler(BarRecorder { bars: bars.clone() });

    connector
        .subscribe(SubscriptionParams::new(vec![reliance()]))
        .await
        .expect("subscribed");
    engine.start().expect("engine started");

    let mut gaps = 0usize;
    let mut stop = None;
    loop {
        match connector.next_event().await {
            Ok(Some(event)) => match event {
                MarketEvent::Bar(bar) => {
                    engine.inject(Message::new(Event::Bar(bar), UnixNanos::from_u64(0)));
                    while engine.pump().expect("pump") {}
                }
                MarketEvent::GapMarker { .. } => gaps += 1,
                _ => {}
            },
            Ok(None) => break,
            Err(err) => {
                stop = Some(err);
                break;
            }
        }
    }
    engine.finish().expect("engine finished");

    assert_eq!(
        bars.seen(),
        vec![100, 200, 6_000],
        "every data event the connector delivered reached a handler"
    );
    assert_eq!(gaps, 1, "the drop surfaced as exactly one gap marker");
    assert_eq!(
        stop,
        Some(PortError::Terminal("session revoked".into())),
        "the terminal error stopped the loop"
    );
    assert!(
        !stop.as_ref().expect("stopped").is_retryable(),
        "a terminal stop is never retried"
    );
    assert_eq!(
        dispatched(engine.audit()),
        vec![100, 200, 6_000],
        "the engine dispatched exactly the bars the connector delivered, in order"
    );
}
