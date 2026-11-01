#![cfg(feature = "india")]
//! Golden parity: an actual simulator fill charged from the Rust India delivery schedule must
//! match the Python reference cost model.
//!
//! The golden minor-unit totals live in `tests/fixtures/india_delivery_fill_cost_golden.json`,
//! recorded from `python/src/honba/markets/india/costs.py`'s `nse_equity_delivery_fill_cost`
//! (integer paise; see the fixture's `how`). Deterministic and offline: no clock, no network.

use std::path::PathBuf;

use honba_core::OrderSide;
use honba_market::CostSchedule;
use honba_market::MarketSegment;
use honba_market::india::costs::NseCashEquitySchedule;

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

fn fixture() -> Fixture {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/india_delivery_fill_cost_golden.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn paise(fees: &honba_market::FeeBreakdown) -> i64 {
    fees.charges
        .iter()
        .map(|c| (c.amount * 100.0).round() as i64)
        .sum()
}

#[test]
fn india_costs_in_backtest_match_python_costmodel() {
    let fixture = fixture();
    assert!(!fixture.cases.is_empty());
    let schedule = NseCashEquitySchedule::delivery();
    let seg = MarketSegment::from("equity_delivery");
    for case in &fixture.cases {
        let side = match case.side.as_str() {
            "buy" => OrderSide::Buy,
            "sell" => OrderSide::Sell,
            other => panic!("unknown side {other:?}"),
        };
        let notional = case.quantity * case.price;
        let fees = schedule.compute_costs(&seg, side, notional);
        assert_eq!(
            paise(&fees),
            case.costs_minor,
            "fill cost mismatch for {case:?}"
        );
    }
}
