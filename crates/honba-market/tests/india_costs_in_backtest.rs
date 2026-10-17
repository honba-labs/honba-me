#![cfg(feature = "india")]
//! Golden parity: an actual simulator fill charged from the Rust India delivery schedule must
//! match the Python reference cost model.
//!
//! The golden minor-unit totals live in `tests/fixtures/india_delivery_fill_cost_golden.json`,
//! recorded from `python/src/honba/markets/india/costs.py`'s `nse_equity_delivery_fill_cost`
//! (integer paise; see the fixture's `how`). Each case drives a real [`NextOpenSim`] fill with
//! [`NextOpenSim::with_schedule`] and asserts the emitted `Trade.costs` and the running fees are
//! that golden. Deterministic and offline: no clock, no network.

use std::path::PathBuf;
use std::sync::Arc;

use honba_engine::ExecutionEngine;
use honba_entities::{Currency, Money};
use honba_market::india::costs::NseCashEquitySchedule;
use honba_market::MarketSegment;
use honba_messages::{
    Bar, BarAggregation, BarSpecification, BarType, Exchange, InstrumentId, Order, OrderId,
    OrderSide, OrderType, PriceType, TimeInForce, UnixNanos,
};
use honba_sim::NextOpenSim;

#[derive(Debug, serde::Deserialize)]
struct Case {
    side: String,
    quantity: f64,
    price: f64,
    costs_minor: i64,
}

#[derive(serde::Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}

fn instrument() -> InstrumentId {
    InstrumentId::new("RELIANCE", Exchange::new("NSE"))
}

fn bar(ts: u64, open: f64) -> Bar {
    let t = UnixNanos::from_u64(ts);
    Bar::new(
        BarType::new(
            instrument(),
            BarSpecification::new(1, BarAggregation::Day, PriceType::Last),
        ),
        open,
        open,
        open,
        open,
        1.0,
        t,
        t,
    )
}

fn market(id: &str, side: OrderSide, qty: f64, ts: u64) -> Order {
    let t = UnixNanos::from_u64(ts);
    Order::new(
        OrderId::new(id),
        instrument(),
        side,
        OrderType::Market,
        qty,
        None,
        TimeInForce::Day,
        t,
        t,
    )
}

fn fixture() -> Fixture {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/india_delivery_fill_cost_golden.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn india_costs_in_backtest_match_python_costmodel() {
    let fixture = fixture();
    assert!(!fixture.cases.is_empty());
    for case in &fixture.cases {
        let side = match case.side.as_str() {
            "buy" => OrderSide::Buy,
            "sell" => OrderSide::Sell,
            other => panic!("unknown side {other:?}"),
        };
        let mut sim = NextOpenSim::new(Money::new(100_000_000, Currency::Inr))
            .unwrap()
            .with_schedule(
                Arc::new(NseCashEquitySchedule::delivery()),
                MarketSegment::from("equity_delivery"),
            );
        // A sell needs a holding before it can fill (long-only).
        if side == OrderSide::Sell {
            sim.set_position(&instrument(), case.quantity).unwrap();
        }
        sim.on_bar(&bar(1, case.price)).unwrap();
        sim.submit(market("o-1", side, case.quantity, 1)).unwrap();
        sim.on_bar(&bar(2, case.price)).unwrap();

        let fills = sim.drain_fills().unwrap();
        assert_eq!(fills.len(), 1, "no fill for case {case:?}");
        assert_eq!(
            fills[0].costs().minor(),
            case.costs_minor,
            "fill cost mismatch for {case:?}"
        );
        assert_eq!(sim.fees().minor(), case.costs_minor, "fees for {case:?}");
    }
}
