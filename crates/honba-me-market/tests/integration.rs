#![cfg(feature = "india")]
//! Integration tests for the India crate.

use chrono::{NaiveDate, NaiveTime};

use honba_entities::InstrumentKind;
use honba_market::calendar::{HolidaySource, Session, TradingCalendar};
use honba_market::india::calendar::NseCalendar;
use honba_market::india::costs::{CostModel, CostModelSource, Segment, SttRates};
use honba_market::india::profile::IndiaMarketProfile;
use honba_market::india::universes::{Nifty50, NIFTY50_SIZE};
use honba_market::profile::MarketProfile;
use honba_market::universes::{Universe, UniverseSource};
use honba_market::Result;
use honba_messages::OrderSide;

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

struct FixedHolidays(Vec<NaiveDate>);
impl HolidaySource for FixedHolidays {
    fn holidays(&self) -> Result<Vec<NaiveDate>> {
        Ok(self.0.clone())
    }
}

fn test_calendar() -> NseCalendar {
    NseCalendar::from_holidays([date(2025, 1, 26), date(2025, 10, 21), date(2025, 12, 25)])
}

#[test]
fn weekends_are_not_trading_days() {
    let cal = test_calendar();
    assert!(!cal.is_trading_day(date(2025, 1, 4)));
    assert!(!cal.is_trading_day(date(2025, 1, 5)));
}

#[test]
fn regular_weekday_is_trading_day() {
    let cal = test_calendar();
    assert!(cal.is_trading_day(date(2025, 1, 6)));
}

#[test]
fn declared_holiday_is_not_trading_day() {
    let cal = test_calendar();
    assert!(cal.is_holiday(date(2025, 10, 21)));
    assert!(!cal.is_trading_day(date(2025, 10, 21)));
}

#[test]
fn from_source_matches_from_holidays() {
    let src = FixedHolidays(vec![date(2025, 1, 26)]);
    let cal = NseCalendar::from_source(&src).unwrap();
    assert!(cal.is_holiday(date(2025, 1, 26)));
}

#[test]
fn next_trading_day_skips_weekend() {
    assert_eq!(
        test_calendar().next_trading_day(date(2025, 1, 3)),
        date(2025, 1, 6)
    );
}

#[test]
fn prev_trading_day_skips_weekend() {
    assert_eq!(
        test_calendar().prev_trading_day(date(2025, 1, 6)),
        date(2025, 1, 3)
    );
}

#[test]
fn trading_days_between_january_2025() {
    assert_eq!(
        test_calendar().trading_days_between(date(2025, 1, 1), date(2025, 1, 31)),
        23
    );
}

#[test]
fn session_contains_regular_hours() {
    let s = Session::regular();
    assert!(s.contains(NaiveTime::from_hms_opt(9, 15, 0).unwrap()));
    assert!(s.contains(NaiveTime::from_hms_opt(12, 0, 0).unwrap()));
    assert!(!s.contains(NaiveTime::from_hms_opt(15, 30, 0).unwrap()));
    assert!(!s.contains(NaiveTime::from_hms_opt(9, 14, 59).unwrap()));
}

fn test_rates() -> SttRates {
    SttRates::new(0.001, 0.00025, 0.0002, 0.001, 0.00125)
}

fn test_model() -> CostModel {
    CostModel::new(test_rates(), 0.0000325, 0.18, 0.00015, 0.000001, 20.0)
}

#[test]
fn stt_rates_by_side() {
    let r = test_rates();
    assert_eq!(r.for_equity_intraday(OrderSide::Buy), 0.0);
    assert_eq!(r.for_equity_intraday(OrderSide::Sell), 0.00025);
    assert_eq!(r.for_equity_futures(OrderSide::Sell), 0.0002);
}

#[test]
fn cost_model_delivery_buy_has_all_components() {
    let b = test_model().compute(Segment::EquityDelivery, OrderSide::Buy, 100_000.0);
    assert!((b.stt - 100.0).abs() < 1e-9);
    assert!(b.stamp_duty > 0.0);
    assert!(b.brokerage > 0.0);
    assert!(b.gst > 0.0);
    assert!(b.total() > b.stt);
}

#[test]
fn cost_model_delivery_sell_has_no_stamp_duty() {
    let b = test_model().compute(Segment::EquityDelivery, OrderSide::Sell, 100_000.0);
    assert_eq!(b.stamp_duty, 0.0);
}

#[test]
fn cost_model_intraday_buy_has_no_stt() {
    let b = test_model().compute(Segment::EquityIntraday, OrderSide::Buy, 100_000.0);
    assert_eq!(b.stt, 0.0);
}

struct FixedModel(CostModel);
impl CostModelSource for FixedModel {
    fn model_for(&self, _date: NaiveDate) -> honba_market::india::Result<CostModel> {
        Ok(self.0)
    }
}

#[test]
fn cost_model_source_returns_model() {
    let src = FixedModel(test_model());
    let m = src.model_for(date(2025, 1, 1)).unwrap();
    let b = m.compute(Segment::EquityDelivery, OrderSide::Buy, 100_000.0);
    assert!((b.stt - 100.0).abs() < 1e-9);
}

struct FixedUniverse(Vec<String>);
impl UniverseSource for FixedUniverse {
    fn load(&self, _as_of: NaiveDate) -> Result<Vec<String>> {
        Ok(self.0.clone())
    }
}

fn test_universe() -> Nifty50 {
    let symbols: Vec<String> = (0..NIFTY50_SIZE).map(|i| format!("SYM{i}")).collect();
    Nifty50::new(date(2025, 1, 1), symbols).unwrap()
}

#[test]
fn universe_has_50_symbols() {
    let u = test_universe();
    assert_eq!(u.len(), 50);
    assert!(!u.is_empty());
}

#[test]
fn universe_contains_and_position() {
    let u = test_universe();
    assert!(u.contains("SYM0"));
    assert_eq!(u.position("SYM0"), Some(0));
    assert!(!u.contains("NOTASYMBOL"));
    assert_eq!(u.position("NOTASYMBOL"), None);
}

#[test]
fn universe_as_of_date() {
    assert_eq!(test_universe().as_of(), date(2025, 1, 1));
}

#[test]
fn nifty50_rejects_wrong_size() {
    let too_few = vec!["A".to_string(); 49];
    assert!(Nifty50::new(date(2025, 1, 1), too_few).is_err());
}

#[test]
fn nifty50_from_source() {
    let src = FixedUniverse((0..50).map(|i| format!("SYM{i}")).collect());
    let u = Nifty50::from_source(date(2025, 1, 1), &src).unwrap();
    assert_eq!(u.len(), 50);
}

#[test]
fn null_calendar_treats_every_day_as_trading() {
    use honba_market::null::NullCalendar;
    let cal = NullCalendar;
    assert!(cal.is_trading_day(date(2025, 1, 1)));
    assert!(cal.is_trading_day(date(2025, 1, 26)));
    assert!(!cal.is_holiday(date(2025, 1, 26)));
}

#[test]
fn india_equity_settlement_is_t_plus_one_from_2023_01_27() {
    let profile = IndiaMarketProfile::new(
        NseCalendar::from_holidays([date(2025, 1, 27)]),
        test_model(),
    );
    let rules = profile.settlement_rules();
    assert_eq!(rules.settlement_days(InstrumentKind::Equity), 1);
    // Fri 24th -> weekend, holiday Mon 27th -> Tue 28th (1).
    assert_eq!(
        rules.settlement_date(
            date(2025, 1, 24),
            InstrumentKind::Equity,
            profile.calendar()
        ),
        date(2025, 1, 28)
    );
}

#[test]
fn india_equity_settlement_is_t_plus_two_before_2023_01_27() {
    let profile = IndiaMarketProfile::new(NseCalendar::from_holidays([]), test_model());
    let rules = profile.settlement_rules();
    // Thu 26 Jan 2023 -> Fri 27th (1), Mon 30th (2).
    assert_eq!(
        rules.settlement_date(
            date(2023, 1, 26),
            InstrumentKind::Equity,
            profile.calendar()
        ),
        date(2023, 1, 30)
    );
    assert_eq!(
        rules.settlement_days_as_of(InstrumentKind::Equity, date(2019, 5, 1)),
        2
    );
}

#[test]
fn india_profile_reports_equity_settlement_days() {
    assert_eq!(IndiaMarketProfile::equity_settlement_days(), 1);
}

#[test]
fn nse_cash_equity_schedules_work_as_boxed_cost_schedules() {
    use honba_market::india::costs::NseCashEquitySchedule;
    use honba_market::{CostSchedule, MarketSegment};

    let schedules: Vec<Box<dyn CostSchedule>> = vec![
        Box::new(NseCashEquitySchedule::delivery()),
        Box::new(NseCashEquitySchedule::intraday()),
    ];
    let totals: Vec<f64> = schedules
        .iter()
        .map(|s| {
            s.compute_costs(&MarketSegment::from("equity"), OrderSide::Sell, 29_500.0)
                .total()
        })
        .collect();
    assert!(totals[0] > totals[1], "delivery STT is higher: {totals:?}");
}
