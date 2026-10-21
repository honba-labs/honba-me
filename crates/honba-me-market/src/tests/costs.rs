//! Unit tests for `crate::costs`.

use std::sync::Arc;

use chrono::NaiveDate;
use honba_messages::OrderSide;

use crate::{
    Charge, CostModelSource, CostSchedule, DatedCostScheduleSource, FeeBreakdown, MarketSegment,
    NullCostSchedule, edge_after_costs,
};

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

/// A dummy schedule that charges a fixed flat `amount` per fill, whichever segment or side.
struct Fixed(f64);

impl CostSchedule for Fixed {
    fn compute_costs(
        &self,
        _segment: &MarketSegment,
        _side: OrderSide,
        _notional: f64,
    ) -> FeeBreakdown {
        FeeBreakdown::from_charges(vec![Charge::new("flat", self.0)])
    }
}

#[test]
fn fee_breakdown_totals_and_looks_up_charges_by_name() {
    let mut fees = FeeBreakdown::empty();
    assert_eq!(fees.total(), 0.0);
    fees.add("brokerage", 20.0);
    fees.add("gst", 3.6);
    assert!((fees.total() - 23.6).abs() < 1e-12);
    assert_eq!(fees.get("gst"), Some(3.6));
    assert_eq!(fees.get("stt"), None);
    assert_eq!(
        FeeBreakdown::from_charges(vec![Charge::new("a", 1.0)]),
        FeeBreakdown {
            charges: vec![Charge::new("a", 1.0)]
        }
    );
}

#[test]
fn charge_displays_with_four_decimals() {
    assert_eq!(Charge::new("stt", 1.5).to_string(), "stt: 1.5000");
}

#[test]
fn market_segment_round_trips_through_strings() {
    let a = MarketSegment::from("EQ");
    let b = MarketSegment::from(String::from("EQ"));
    assert_eq!(a, b);
    assert_eq!(a.as_str(), "EQ");
    assert_eq!(a.to_string(), "EQ");
}

#[test]
fn null_cost_schedule_charges_nothing() {
    let fees = NullCostSchedule.compute_costs(&"EQ".into(), OrderSide::Buy, 1_000_000.0);
    assert_eq!(fees.total(), 0.0);
}

#[test]
fn total_bps_scales_a_breakdown_onto_the_notional() {
    let mut fees = FeeBreakdown::empty();
    fees.add("brokerage", 20.0);
    fees.add("stt", 30.0);
    // 50 of cost on a 10_000 notional is 50 basis points of that notional.
    assert!((fees.total_bps(10_000.0) - 50.0).abs() < 1e-9);
    // A zero notional has no denominator: zero bps, never a division by zero.
    assert_eq!(fees.total_bps(0.0), 0.0);
    assert!(FeeBreakdown::empty().total_bps(5_000.0) == 0.0);
}

#[test]
fn edge_after_costs_nets_the_charge_bps_out_of_a_gross_edge() {
    let mut fees = FeeBreakdown::empty();
    fees.add("brokerage", 20.0);
    fees.add("stt", 30.0);
    // 80 bps gross, 50 bps of costs on a 10_000 notional -> 30 bps net.
    assert!((edge_after_costs(80.0, &fees, 10_000.0) - 30.0).abs() < 1e-9);
    // Costs at or beyond the gross edge leave a non-positive net edge.
    assert!(edge_after_costs(40.0, &fees, 10_000.0) <= 0.0);
    // A zero notional means no costs: the gross edge passes through.
    assert_eq!(edge_after_costs(80.0, &fees, 0.0), 80.0);
}

#[test]
fn charges_effective_dated() {
    let early = date(2024, 1, 1);
    let middle = date(2025, 1, 1);
    let late = date(2026, 1, 1);
    let source = DatedCostScheduleSource::new(vec![
        (early, Arc::new(Fixed(1.0))),
        (late, Arc::new(Fixed(2.0))),
    ]);
    let total_on = |d: NaiveDate| -> f64 {
        source
            .schedule_for(d)
            .unwrap()
            .compute_costs(&MarketSegment::from("EQ"), OrderSide::Buy, 100.0)
            .total()
    };
    // Before the first effective date there is no schedule in force.
    assert!(source.schedule_for(date(2023, 12, 31)).is_err());
    // On or after `early`, before `late`: the early schedule.
    assert_eq!(total_on(early), 1.0);
    assert_eq!(total_on(middle), 1.0);
    // The boundary date itself picks the later schedule.
    assert_eq!(total_on(late), 2.0);
    assert_eq!(total_on(date(2027, 1, 1)), 2.0);
    // An empty source has no schedule for any date.
    assert!(
        DatedCostScheduleSource::new(Vec::new())
            .schedule_for(middle)
            .is_err()
    );
}
