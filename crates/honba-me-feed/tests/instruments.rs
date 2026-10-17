//! Integration test: a backtest-shaped flow consults the dated instrument
//! catalog, so an expired contract neither resolves nor fills while a live one
//! does. Fixtures are generated in a temp dir; nothing touches the network.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow::array::{ArrayRef, Float64Array, StringArray};
use arrow::record_batch::RecordBatch;
use chrono::NaiveDate;
use honba_engine::{Engine, EngineOutput, ExecutionEngine, Handler, Result as EngineResult};
use honba_entities::{Currency, Trade};
use honba_feed::{snapshot_schema, CatalogInstrumentMaster};
use honba_messages::{
    Event, Exchange, InstrumentId, Message, Order, OrderId, OrderSide, OrderType, TimeInForce,
    UnixNanos,
};
use honba_ports::InstrumentMaster;
use parquet::arrow::ArrowWriter;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "honba_instruments_it_{}_{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Writes `{root}/instruments/{date}.parquet` with the two futures under test.
fn write_futures(root: &Path, date: &str, rows: &[(&str, &str)]) {
    let dir = root.join("instruments");
    std::fs::create_dir_all(&dir).unwrap();
    let strings = |values: Vec<Option<&str>>| -> ArrayRef { Arc::new(StringArray::from(values)) };
    let numbers = |values: Vec<Option<f64>>| -> ArrayRef { Arc::new(Float64Array::from(values)) };
    let columns: Vec<ArrayRef> = vec![
        strings(rows.iter().map(|(symbol, _)| Some(*symbol)).collect()),
        strings(rows.iter().map(|_| Some("NSE")).collect()),
        strings(rows.iter().map(|_| Some("future")).collect()),
        strings(rows.iter().map(|_| Some("INR")).collect()),
        numbers(rows.iter().map(|_| Some(75.0)).collect()),
        numbers(rows.iter().map(|_| Some(0.05)).collect()),
        numbers(rows.iter().map(|_| Some(1500.0)).collect()),
        strings(rows.iter().map(|(_, expiry)| Some(*expiry)).collect()),
        numbers(rows.iter().map(|_| None).collect()),
        strings(rows.iter().map(|_| None).collect()),
        strings(rows.iter().map(|_| Some("NIFTY")).collect()),
    ];
    let schema = Arc::new(snapshot_schema());
    let batch = RecordBatch::try_new(schema.clone(), columns).unwrap();
    let file = File::create(dir.join(format!("{date}.parquet"))).unwrap();
    let mut writer = ArrowWriter::try_new(file, schema, None).unwrap();
    writer.write(&batch).unwrap();
    writer.close().unwrap();
}

/// An execution double that fills every order it receives at 100.0.
#[derive(Default)]
struct FillAtClose {
    fills: Vec<Trade>,
}

impl ExecutionEngine for FillAtClose {
    fn submit(&mut self, order: Order) -> EngineResult<()> {
        let ts = order.ts_event();
        self.fills.push(Trade::new(
            order.order_id().clone(),
            order.instrument_id().clone(),
            order.side(),
            order.quantity(),
            100.0,
            Currency::Inr,
            ts,
            ts,
        ));
        Ok(())
    }

    fn cancel(&mut self, _order_id: &str, _now: UnixNanos) -> EngineResult<()> {
        Ok(())
    }

    fn drain_fills(&mut self) -> EngineResult<Vec<Trade>> {
        Ok(std::mem::take(&mut self.fills))
    }
}

/// The strategy edge: an order that reaches the engine is submitted.
struct SubmitOrders;

impl Handler for SubmitOrders {
    fn on_event(&mut self, event: &Event, _ts_init: UnixNanos) -> EngineResult<EngineOutput> {
        match event {
            Event::Order(order) => Ok(EngineOutput::Orders(vec![order.clone()])),
            _ => Ok(EngineOutput::None),
        }
    }
}

#[tokio::test]
async fn backtest_resolves_expired_contract() {
    let dir = TempDir::new("backtest_expired");
    write_futures(
        &dir.0,
        "2024-01-01",
        &[
            ("NIFTY24OCTFUT", "2024-10-31"),
            ("NIFTY24DECFUT", "2024-12-26"),
        ],
    );
    let master = CatalogInstrumentMaster::open(&dir.0).unwrap();
    let as_of = NaiveDate::from_ymd_opt(2024, 11, 15).unwrap();

    let expired = InstrumentId::new("NIFTY24OCTFUT", Exchange::new("NSE"));
    let live = InstrumentId::new("NIFTY24DECFUT", Exchange::new("NSE"));

    // The dated catalog is the gate: the contract expired on 2024-10-31.
    assert!(master
        .resolve_as_of(&expired, as_of)
        .await
        .unwrap()
        .is_none());
    assert!(master.resolve_as_of(&live, as_of).await.unwrap().is_some());

    let ts = UnixNanos::from_u64(1_700_000_000_000_000_000);
    let mut engine = Engine::new();
    engine.add_handler(SubmitOrders);
    engine.set_execution(Box::new(FillAtClose::default()));

    // Backtest-shaped: resolve the symbol as of the decision date, then submit.
    for (id, order_id) in [(&expired, "EXPIRED-1"), (&live, "LIVE-1")] {
        if master.resolve_as_of(id, as_of).await.unwrap().is_some() {
            let order = Order::new(
                OrderId::new(order_id),
                id.clone(),
                OrderSide::Buy,
                OrderType::Market,
                75.0,
                None,
                TimeInForce::Day,
                ts,
                ts,
            );
            engine.inject(Message::new(Event::Order(order), ts));
        }
    }

    engine.start().unwrap();
    while engine.pump().unwrap() {}
    engine.finish().unwrap();

    assert_eq!(engine.position(&live), 75.0, "live contract filled");
    assert_eq!(
        engine.position(&expired),
        0.0,
        "expired contract did not fill"
    );
    assert!(
        engine.order("EXPIRED-1").is_none(),
        "no order reached the engine"
    );
}
