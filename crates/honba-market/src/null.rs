//! Null market pack for unit testing, offline simulation, and headless test harnesses.

use chrono::{NaiveDate, NaiveTime};
use honba_entities::{Currency, InstrumentKind, PositionSide};
use honba_messages::OrderSide;

use crate::calendar::{MarketCalendar, Session};
use crate::costs::{CostSchedule, FeeBreakdown, MarketSegment};
use crate::expiry::ExpiryRules;
use crate::profile::MarketProfile;
use crate::rules::{InstrumentRulesProvider, SymbolGrammar};
use crate::settlement::{
    MarginModel, MarginRequirement, SettlementRules, StandardRollingSettlement,
};

/// A simple 24/7 calendar that treats every single day as a trading day.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullCalendar;

impl MarketCalendar for NullCalendar {
    fn session(&self) -> Session {
        Session::new(
            NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
            NaiveTime::from_hms_opt(23, 59, 59).unwrap(),
        )
    }

    fn is_holiday(&self, _date: NaiveDate) -> bool {
        false
    }

    fn is_trading_day(&self, _date: NaiveDate) -> bool {
        true
    }

    fn is_settlement_day(&self, _date: NaiveDate) -> bool {
        true
    }
}

/// Zero-cost schedule for null market tests.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullCostSchedule;

impl CostSchedule for NullCostSchedule {
    fn compute_costs(
        &self,
        _segment: &MarketSegment,
        _side: OrderSide,
        _notional: f64,
    ) -> FeeBreakdown {
        FeeBreakdown::empty()
    }
}

/// Permissive instrument rules provider.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullInstrumentRulesProvider;

impl InstrumentRulesProvider for NullInstrumentRulesProvider {}

/// Pass-through symbol grammar for the null market.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullSymbolGrammar;

impl SymbolGrammar for NullSymbolGrammar {}

/// No-op expiry rules.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullExpiryRules;

impl ExpiryRules for NullExpiryRules {
    fn is_expiry_date(&self, _date: NaiveDate, _kind: InstrumentKind) -> bool {
        false
    }

    fn expiry_for_month(
        &self,
        _year: i32,
        _month: u32,
        _kind: InstrumentKind,
    ) -> Option<NaiveDate> {
        None
    }
}

/// Zero-margin model.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullMarginModel;

impl MarginModel for NullMarginModel {
    fn calculate_margin(
        &self,
        _kind: InstrumentKind,
        _side: PositionSide,
        _notional: f64,
    ) -> MarginRequirement {
        MarginRequirement::zero()
    }
}

/// Comprehensive null market profile for test isolation.
#[derive(Debug, Default, Clone)]
pub struct NullMarketProfile {
    calendar: NullCalendar,
    costs: NullCostSchedule,
    rules: NullInstrumentRulesProvider,
    grammar: NullSymbolGrammar,
    expiry: NullExpiryRules,
    margin: NullMarginModel,
    settlement: StandardRollingSettlement,
}

impl MarketProfile for NullMarketProfile {
    fn market_code(&self) -> &str {
        "null"
    }

    fn currency(&self) -> Currency {
        Currency::Usd
    }

    fn calendar(&self) -> &dyn MarketCalendar {
        &self.calendar
    }

    fn cost_schedule(&self) -> &dyn CostSchedule {
        &self.costs
    }

    fn instrument_rules(&self) -> &dyn InstrumentRulesProvider {
        &self.rules
    }

    fn symbol_grammar(&self) -> &dyn SymbolGrammar {
        &self.grammar
    }

    fn expiry_rules(&self) -> &dyn ExpiryRules {
        &self.expiry
    }

    fn margin_model(&self) -> &dyn MarginModel {
        &self.margin
    }

    fn settlement_rules(&self) -> &dyn SettlementRules {
        &self.settlement
    }
}
