//! Unit tests for `crate::india::costs::cash_equity` (the NSE cash-equity schedules).

use honba_messages::OrderSide;

use crate::india::costs::{NseCashEquitySchedule, CHARGE_IPFT};
use crate::{CostModelSource, CostSchedule, MarketSegment};

fn seg() -> MarketSegment {
    MarketSegment::from("equity")
}

fn paise(fees: &crate::FeeBreakdown) -> i64 {
    fees.charges
        .iter()
        .map(|c| (c.amount * 100.0).round() as i64)
        .sum()
}

#[test]
fn delivery_buy_charges_stamp_duty_but_no_stt() {
    let fees = NseCashEquitySchedule::delivery().compute_costs(&seg(), OrderSide::Buy, 29_500.0);
    assert_eq!(fees.get("stt"), Some(0.0));
    assert!((fees.get("stamp_duty").unwrap() - 4.425).abs() < 1e-9);
    assert!((fees.get("brokerage").unwrap() - 8.85).abs() < 1e-9);
    assert!((fees.get(CHARGE_IPFT).unwrap() - 0.0295).abs() < 1e-9);
    assert_eq!(paise(&fees), 1598);
}

#[test]
fn delivery_sell_charges_stt_but_no_stamp_duty() {
    let fees = NseCashEquitySchedule::delivery().compute_costs(&seg(), OrderSide::Sell, 29_500.0);
    assert_eq!(fees.get("stamp_duty"), Some(0.0));
    assert!((fees.get("stt").unwrap() - 29.5).abs() < 1e-9);
    assert_eq!(paise(&fees), 4105);
}

#[test]
fn brokerage_is_capped_per_order() {
    let fees =
        NseCashEquitySchedule::delivery().compute_costs(&seg(), OrderSide::Buy, 10_000_000.0);
    assert_eq!(fees.get("brokerage"), Some(20.0));
}

#[test]
fn intraday_uses_its_own_rates() {
    let buy = NseCashEquitySchedule::intraday().compute_costs(&seg(), OrderSide::Buy, 29_500.0);
    let sell = NseCashEquitySchedule::intraday().compute_costs(&seg(), OrderSide::Sell, 29_500.0);
    assert_eq!(paise(&buy), 1244);
    assert_eq!(paise(&sell), 1893);
    assert!((sell.get("stt").unwrap() - 7.375).abs() < 1e-9);
}

#[test]
fn gst_is_on_brokerage_exchange_sebi_and_ipft_unrounded() {
    let fees = NseCashEquitySchedule::delivery().compute_costs(&seg(), OrderSide::Buy, 29_500.0);
    let taxable = ["brokerage", "exchange_fee", "sebi_fee", CHARGE_IPFT]
        .iter()
        .map(|n| fees.get(n).unwrap())
        .fold(0.0, |a, b| a + b);
    assert_eq!(fees.get("gst"), Some(0.18 * taxable));
}

#[test]
fn a_zero_notional_costs_nothing_and_a_negative_one_is_taken_absolute() {
    let zero = NseCashEquitySchedule::delivery().compute_costs(&seg(), OrderSide::Buy, 0.0);
    assert!(zero.charges.is_empty());
    let pos = NseCashEquitySchedule::delivery().compute_costs(&seg(), OrderSide::Sell, 500.0);
    let neg = NseCashEquitySchedule::delivery().compute_costs(&seg(), OrderSide::Sell, -500.0);
    assert_eq!(pos, neg);
}

#[test]
fn the_delivery_source_serves_the_current_schedule_from_its_effective_date() {
    let source = crate::india::costs::nse_cash_equity_delivery_source();
    let from = crate::india::costs::nse_cash_equity_delivery_effective_from();
    let expected =
        NseCashEquitySchedule::delivery().compute_costs(&seg(), OrderSide::Buy, 29_500.0);
    let got = source
        .schedule_for(from)
        .unwrap()
        .compute_costs(&seg(), OrderSide::Buy, 29_500.0);
    assert_eq!(got, expected);
    // Historical schedules are not sourced yet: the day before the effective date has none.
    assert!(source.schedule_for(from.pred_opt().unwrap()).is_err());
}
