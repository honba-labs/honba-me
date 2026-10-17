//! India market profile and default rules provider.

use chrono::NaiveDate;
use honba_entities::{Currency, Instrument, InstrumentKind, PositionSide};

use crate::calendar::MarketCalendar;
use crate::costs::CostSchedule;
use crate::expiry::{ExpiryRules, LastThursdayExpiry};
use crate::profile::MarketProfile;
use crate::rules::{InstrumentRules, InstrumentRulesProvider, SymbolGrammar};
use crate::settlement::{MarginModel, MarginRequirement, SettlementRules, SettlementSchedule};

use super::calendar::NseCalendar;
use super::costs::{CostModel, SttRates};

/// India instrument rules provider (standard NSE lot sizes and tick increments).
#[derive(Debug, Default, Clone, Copy)]
pub struct IndiaInstrumentRulesProvider;

impl InstrumentRulesProvider for IndiaInstrumentRulesProvider {
    fn rules_for(&self, instrument: &Instrument) -> InstrumentRules {
        match instrument.kind() {
            InstrumentKind::Equity => {
                // Cash equity: lot size 1, tick size 0.05
                InstrumentRules::new(1.0, 0.05)
            }
            _ => InstrumentRules::new(instrument.lot_size(), instrument.tick_size()),
        }
    }
}

/// India symbol grammar validator and normalizer.
#[derive(Debug, Default, Clone, Copy)]
pub struct IndiaSymbolGrammar;

impl SymbolGrammar for IndiaSymbolGrammar {
    fn normalize(&self, raw: &str) -> String {
        raw.trim().to_uppercase()
    }

    fn validate(&self, raw: &str) -> crate::Result<()> {
        let norm = self.normalize(raw);
        if norm.is_empty() {
            return Err(crate::MarketError::InvalidSymbol {
                symbol: raw.to_string(),
                reason: "symbol cannot be empty".to_string(),
            });
        }
        Ok(())
    }

    fn resolve_instrument(&self, raw: &str) -> crate::Result<Instrument> {
        let norm = self.normalize(raw);
        let (exchange_code, symbol) = match norm.split_once(':') {
            Some((ex, sym)) => (ex, sym),
            None => ("NSE", norm.as_str()),
        };
        let exchange = honba_messages::Exchange::new(exchange_code);

        // Try F&O contract parsing first
        if let Ok(contract) = crate::india::fno::parse_fno_symbol(symbol) {
            let lot_size = crate::india::fno::default_fno_lot_size(contract.underlying());
            let tick_size = 0.05;
            return Ok(contract.to_instrument(exchange, lot_size, tick_size));
        }

        // Validate basic cash equity symbol
        self.validate(symbol)?;

        let id = honba_messages::InstrumentId::new(symbol, exchange);
        Ok(Instrument::new(
            id,
            InstrumentKind::Equity,
            Currency::Inr,
            1.0,
            0.05,
        ))
    }
}

/// Margin model for Indian markets (SEBI peak margin guidelines).
#[derive(Debug, Default, Clone, Copy)]
pub struct IndiaMarginModel;

impl MarginModel for IndiaMarginModel {
    fn calculate_margin(
        &self,
        kind: InstrumentKind,
        _side: PositionSide,
        notional: f64,
    ) -> MarginRequirement {
        match kind {
            InstrumentKind::Equity => {
                // Intraday VAR+ELM ~ 20%, Cash delivery 100%
                MarginRequirement::new(notional * 0.20, notional * 0.15)
            }
            InstrumentKind::Future => {
                // SPAN + Exposure ~ 25%
                MarginRequirement::new(notional * 0.25, notional * 0.20)
            }
            InstrumentKind::Option => {
                // Option buying 100% premium; Option selling SPAN+Exposure
                MarginRequirement::new(notional * 0.30, notional * 0.25)
            }
            _ => MarginRequirement::zero(),
        }
    }
}

/// First date on which every NSE/BSE equity settles T+1 (T+2 before).
fn india_equity_settlement() -> SettlementSchedule {
    let t_plus_1_from = NaiveDate::from_ymd_opt(2023, 1, 27).expect("valid date");
    SettlementSchedule::new(2).from(t_plus_1_from, 1)
}

/// Comprehensive India market profile (NSE/BSE).
#[derive(Debug, Clone)]
pub struct IndiaMarketProfile {
    calendar: NseCalendar,
    costs: CostModel,
    rules: IndiaInstrumentRulesProvider,
    grammar: IndiaSymbolGrammar,
    expiry: LastThursdayExpiry,
    margin: IndiaMarginModel,
    settlement: SettlementSchedule,
}

impl Default for IndiaMarketProfile {
    fn default() -> Self {
        Self {
            calendar: NseCalendar::from_holidays(Vec::<NaiveDate>::new()),
            costs: CostModel::new(
                SttRates::default(),
                0.0000345, // NSE equity transaction charge ~0.00345%
                0.18,      // 18% GST
                0.00015,   // 0.015% stamp duty on buy
                0.000001,  // SEBI turnover charge
                20.0,      // Flat discount brokerage of ₹20
            ),
            rules: IndiaInstrumentRulesProvider,
            grammar: IndiaSymbolGrammar,
            expiry: LastThursdayExpiry,
            margin: IndiaMarginModel,
            settlement: india_equity_settlement(),
        }
    }
}

impl IndiaMarketProfile {
    /// Creates an India market profile with a custom calendar and cost model.
    pub fn new(calendar: NseCalendar, costs: CostModel) -> Self {
        Self {
            calendar,
            costs,
            rules: IndiaInstrumentRulesProvider,
            grammar: IndiaSymbolGrammar,
            expiry: LastThursdayExpiry,
            margin: IndiaMarginModel,
            settlement: india_equity_settlement(),
        }
    }

    /// Settlement cycle for India equity delivery today (T+1).
    ///
    /// Use [`Self::equity_settlement_days_as_of`] for a historical date. Single source of
    /// truth behind `honba._honba.nse_equity_settlement_days()`; Python can still override
    /// the cycle per run or strategy via `settlement_days`.
    pub fn equity_settlement_days() -> usize {
        Self::default()
            .settlement_rules()
            .settlement_days(InstrumentKind::Equity)
    }

    /// Settlement cycle for India equity delivery on `as_of`: T+2 before 2023-01-27,
    /// T+1 from that date (SEBI's phased T+1 move completed for all equities then).
    pub fn equity_settlement_days_as_of(as_of: NaiveDate) -> usize {
        Self::default()
            .settlement_rules()
            .settlement_days_as_of(InstrumentKind::Equity, as_of)
    }
}

impl MarketProfile for IndiaMarketProfile {
    fn market_code(&self) -> &str {
        "nse_bse"
    }

    fn currency(&self) -> Currency {
        Currency::Inr
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
