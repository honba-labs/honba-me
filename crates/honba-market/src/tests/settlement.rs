//! Unit tests for `crate::settlement`.

use honba_entities::{InstrumentKind, PositionSide};

use super::{date, Holidays};
use crate::settlement::PercentageMarginModel;
use crate::{MarginModel, MarginRequirement, MarketProfile, SettlementRules, SettlementType};
use crate::{SettlementSchedule, StandardRollingSettlement};

#[test]
fn t_plus_one_skips_weekends_and_holidays() {
    let cal = Holidays(vec![date(2025, 1, 13)]);
    let s = StandardRollingSettlement::t_plus_1();
    // Fri 10th -> Sat, Sun, holiday Mon 13th -> Tue 14th.
    assert_eq!(
        s.settlement_date(date(2025, 1, 10), InstrumentKind::Equity, &cal),
        date(2025, 1, 14)
    );
}

#[test]
fn t_plus_zero_settles_on_the_trade_date() {
    let cal = Holidays(vec![]);
    let s = StandardRollingSettlement::t_plus_0();
    assert_eq!(
        s.settlement_date(date(2025, 1, 11), InstrumentKind::Equity, &cal),
        date(2025, 1, 11)
    );
}

#[test]
fn t_plus_two_skips_weekends() {
    let cal = Holidays(vec![]);
    let s = StandardRollingSettlement::t_plus_2();
    // Thu 9th -> Fri 10th (1), Sat/Sun skipped -> Mon 13th (2).
    assert_eq!(
        s.settlement_date(date(2025, 1, 9), InstrumentKind::Equity, &cal),
        date(2025, 1, 13)
    );
}

#[test]
fn t_plus_two_skips_weekends_and_holidays() {
    let cal = Holidays(vec![date(2025, 1, 13)]);
    let s = StandardRollingSettlement::t_plus_2();
    // Fri 10th -> Sat, Sun, holiday Mon 13th -> Tue 14th, Wed 15th.
    assert_eq!(
        s.settlement_date(date(2025, 1, 10), InstrumentKind::Equity, &cal),
        date(2025, 1, 15)
    );
}

#[test]
fn t_plus_two_reports_two_days() {
    let s = StandardRollingSettlement::t_plus_2();
    assert_eq!(s.settlement_days(InstrumentKind::Equity), 2);
}

#[test]
fn schedule_picks_the_cycle_in_force_on_a_date() {
    let s = SettlementSchedule::new(2).from(date(2023, 1, 27), 1);
    assert_eq!(
        s.settlement_days_as_of(InstrumentKind::Equity, date(2023, 1, 26)),
        2
    );
    assert_eq!(
        s.settlement_days_as_of(InstrumentKind::Equity, date(2023, 1, 27)),
        1
    );
    assert_eq!(
        s.settlement_days_as_of(InstrumentKind::Equity, date(2020, 1, 1)),
        2
    );
    // Without a date the latest cycle applies.
    assert_eq!(s.settlement_days(InstrumentKind::Equity), 1);
}

#[test]
fn schedule_phases_may_be_added_out_of_order() {
    let s = SettlementSchedule::new(3)
        .from(date(2024, 1, 1), 0)
        .from(date(2020, 1, 1), 2);
    assert_eq!(
        s.settlement_days_as_of(InstrumentKind::Equity, date(2021, 1, 1)),
        2
    );
    assert_eq!(
        s.settlement_days_as_of(InstrumentKind::Equity, date(2024, 6, 1)),
        0
    );
    assert_eq!(
        s.settlement_days_as_of(InstrumentKind::Equity, date(2019, 1, 1)),
        3
    );
}

#[test]
fn settlement_date_uses_the_cycle_of_the_trade_date() {
    let cal = Holidays(vec![]);
    let s = SettlementSchedule::new(2).from(date(2023, 1, 27), 1);
    // Thu 26 Jan 2023 trades settle T+2 (Mon 30th); Fri 27th trades T+1 (Mon 30th).
    assert_eq!(
        s.settlement_date(date(2023, 1, 26), InstrumentKind::Equity, &cal),
        date(2023, 1, 30)
    );
    assert_eq!(
        s.settlement_date(date(2023, 1, 27), InstrumentKind::Equity, &cal),
        date(2023, 1, 30)
    );
    // Wed 25th trades settle T+2 = Fri 27th.
    assert_eq!(
        s.settlement_date(date(2023, 1, 25), InstrumentKind::Equity, &cal),
        date(2023, 1, 27)
    );
}

#[test]
fn fixed_rules_ignore_the_as_of_date() {
    let s = StandardRollingSettlement::t_plus_2();
    assert_eq!(
        s.settlement_days_as_of(InstrumentKind::Equity, date(2030, 1, 1)),
        2
    );
}

#[cfg(feature = "india")]
#[test]
fn india_equity_settles_t_plus_one_now_and_t_plus_two_before_2023_01_27() {
    let profile = crate::IndiaMarketProfile::default();
    let rules = profile.settlement_rules();
    assert_eq!(rules.settlement_days(InstrumentKind::Equity), 1);
    assert_eq!(crate::IndiaMarketProfile::equity_settlement_days(), 1);
    assert_eq!(
        rules.settlement_days_as_of(InstrumentKind::Equity, date(2023, 1, 26)),
        2
    );
    assert_eq!(
        rules.settlement_days_as_of(InstrumentKind::Equity, date(2023, 1, 27)),
        1
    );
    assert_eq!(
        crate::IndiaMarketProfile::equity_settlement_days_as_of(date(2022, 6, 1)),
        2
    );
    assert_eq!(
        crate::IndiaMarketProfile::equity_settlement_days_as_of(date(2024, 6, 1)),
        1
    );
}

#[test]
fn rolling_settlement_is_cash_for_every_kind() {
    let s = StandardRollingSettlement::new(2);
    assert_eq!(s.settlement_days(InstrumentKind::Future), 2);
    assert_eq!(
        s.settlement_type(InstrumentKind::Option),
        SettlementType::Cash
    );
}

#[test]
fn percentage_margin_scales_notional() {
    let m = PercentageMarginModel::new(0.2, 0.1);
    assert_eq!(
        m.calculate_margin(InstrumentKind::Future, PositionSide::Short, 1000.0),
        MarginRequirement::new(200.0, 100.0)
    );
    assert_eq!(MarginRequirement::zero(), MarginRequirement::new(0.0, 0.0));
}
