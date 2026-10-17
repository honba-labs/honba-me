//! Generic margin models and settlement rules.
//!
//! The clearing cycle is a country + exchange property, and it changes over time: India
//! (NSE/BSE) equities settled T+2 until 2023-01-26 and settle T+1 from 2023-01-27, other
//! markets settle T+1, and same-day instruments settle T+0. [`StandardRollingSettlement`]
//! carries one fixed cycle; [`SettlementSchedule`] carries dated phases and answers
//! [`SettlementRules::settlement_days_as_of`]. The India pack
//! (`crates/honba-market/src/india/profile.rs`) is surfaced to Python via
//! `honba._honba.nse_equity_settlement_days(as_of)`, and Python can override the cycle
//! per run or strategy through `settlement_days`.

use chrono::{Duration, NaiveDate};
use honba_entities::{InstrumentKind, PositionSide};
use serde::{Deserialize, Serialize};

use crate::calendar::MarketCalendar;

/// Margin requirements calculated for an order or position.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MarginRequirement {
    /// Initial margin required to place or hold the position.
    pub initial: f64,
    /// Maintenance margin below which margin call/liquidation triggers.
    pub maintenance: f64,
}

impl MarginRequirement {
    /// Creates a margin requirement.
    pub fn new(initial: f64, maintenance: f64) -> Self {
        Self {
            initial,
            maintenance,
        }
    }

    /// Zero margin (e.g. for delivery cash equity buys where full cash is paid).
    pub fn zero() -> Self {
        Self {
            initial: 0.0,
            maintenance: 0.0,
        }
    }
}

/// Generic margin model contract.
pub trait MarginModel: Send + Sync {
    /// Calculates initial and maintenance margin for a given notional, position side, and kind.
    fn calculate_margin(
        &self,
        kind: InstrumentKind,
        side: PositionSide,
        notional: f64,
    ) -> MarginRequirement;
}

/// Default percentage-based margin model.
#[derive(Clone, Copy, Debug)]
pub struct PercentageMarginModel {
    initial_pct: f64,
    maintenance_pct: f64,
}

impl PercentageMarginModel {
    /// Creates a percentage margin model (e.g. 0.20 for 20%).
    pub fn new(initial_pct: f64, maintenance_pct: f64) -> Self {
        Self {
            initial_pct,
            maintenance_pct,
        }
    }
}

impl MarginModel for PercentageMarginModel {
    fn calculate_margin(
        &self,
        _kind: InstrumentKind,
        _side: PositionSide,
        notional: f64,
    ) -> MarginRequirement {
        MarginRequirement::new(notional * self.initial_pct, notional * self.maintenance_pct)
    }
}

/// Settlement type / mechanism.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettlementType {
    /// Cash settled.
    Cash,
    /// Physical delivery of underlying asset.
    PhysicalDelivery,
}

/// Settlement rules governing clearing cycle (e.g. T+1, T+0) and mechanism.
pub trait SettlementRules: Send + Sync {
    /// Returns the settlement cycle duration in business/settlement days (e.g. 1 for T+1, 0 for T+0).
    ///
    /// For a dated schedule this is the cycle currently in force (the latest phase).
    fn settlement_days(&self, kind: InstrumentKind) -> usize;

    /// Returns the settlement cycle in force on `as_of`. Fixed rules ignore the date.
    fn settlement_days_as_of(&self, kind: InstrumentKind, _as_of: NaiveDate) -> usize {
        self.settlement_days(kind)
    }

    /// Returns the delivery or settlement mechanism.
    fn settlement_type(&self, kind: InstrumentKind) -> SettlementType;

    /// Computes the expected settlement date given a trade execution date and market calendar.
    fn settlement_date(
        &self,
        trade_date: NaiveDate,
        kind: InstrumentKind,
        calendar: &dyn MarketCalendar,
    ) -> NaiveDate {
        let days = self.settlement_days_as_of(kind, trade_date);
        let mut d = trade_date;
        let mut remaining = days;
        while remaining > 0 {
            d += Duration::days(1);
            if calendar.is_settlement_day(d) {
                remaining -= 1;
            }
        }
        d
    }
}

/// Standard T+n rolling settlement rules.
#[derive(Clone, Copy, Debug, Default)]
pub struct StandardRollingSettlement {
    days: usize,
}

impl StandardRollingSettlement {
    /// Creates rolling settlement with `days` duration (e.g. 1 for T+1).
    pub fn new(days: usize) -> Self {
        Self { days }
    }

    /// Standard T+1 settlement.
    pub fn t_plus_1() -> Self {
        Self { days: 1 }
    }

    /// Standard T+2 settlement (India NSE/BSE equity delivery).
    pub fn t_plus_2() -> Self {
        Self { days: 2 }
    }

    /// Standard T+0 instant settlement.
    pub fn t_plus_0() -> Self {
        Self { days: 0 }
    }
}

impl SettlementRules for StandardRollingSettlement {
    fn settlement_days(&self, _kind: InstrumentKind) -> usize {
        self.days
    }

    fn settlement_type(&self, _kind: InstrumentKind) -> SettlementType {
        SettlementType::Cash
    }
}

/// Settlement cycle that changes over time: an initial cycle plus dated phases.
///
/// `SettlementSchedule::new(2).from(2023-01-27, 1)` settles T+2 before 2023-01-27 and T+1
/// from that date on.
#[derive(Clone, Debug)]
pub struct SettlementSchedule {
    initial: usize,
    /// `(effective_from, days)`, sorted by date.
    phases: Vec<(NaiveDate, usize)>,
}

impl SettlementSchedule {
    /// A schedule that settles in `initial_days` until a later phase is added.
    pub fn new(initial_days: usize) -> Self {
        Self {
            initial: initial_days,
            phases: Vec::new(),
        }
    }

    /// Adds a phase: from `effective` (inclusive) trades settle in `days`. Phases may be
    /// added in any order; a second phase for the same date replaces the first.
    pub fn from(mut self, effective: NaiveDate, days: usize) -> Self {
        self.phases.retain(|(d, _)| *d != effective);
        self.phases.push((effective, days));
        self.phases.sort_by_key(|(d, _)| *d);
        self
    }

    fn days_as_of(&self, as_of: NaiveDate) -> usize {
        self.phases
            .iter()
            .rev()
            .find(|(d, _)| *d <= as_of)
            .map_or(self.initial, |(_, days)| *days)
    }
}

impl SettlementRules for SettlementSchedule {
    fn settlement_days(&self, _kind: InstrumentKind) -> usize {
        self.phases.last().map_or(self.initial, |(_, days)| *days)
    }

    fn settlement_days_as_of(&self, _kind: InstrumentKind, as_of: NaiveDate) -> usize {
        self.days_as_of(as_of)
    }

    fn settlement_type(&self, _kind: InstrumentKind) -> SettlementType {
        SettlementType::Cash
    }
}
