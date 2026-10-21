#![deny(unsafe_code)]

//! Generic market contracts and pluggable market packs for the Honba platform.
//!
//! Provides the generic traits and types for market calendars, transaction cost schedules,
//! instrument trading rules, symbol grammars, derivative expiry conventions, margin models,
//! and unified market profiles.
//!
//! # Architecture
//!
//! - **Generic Contracts**:
//!   - [`MarketCalendar`]: Trading sessions, holidays, and settlement calendar arithmetic.
//!   - [`CostSchedule`]: Itemized transaction fees and tax calculations returning named [`Charge`]s.
//!   - [`InstrumentRules`]: Lot sizes, tick sizes, freeze quantities, and price bands.
//!   - [`SymbolGrammar`]: Ticker parsing, normalization, and grammar validation.
//!   - [`ExpiryRules`]: Derivative expiration schedules and settlement dates.
//!   - [`MarginModel`]: Initial and maintenance margin requirements.
//!   - [`SettlementRules`]: Clearing cycles (e.g. T+2 for India equities, T+1, T+0) and
//!     cash/physical delivery.
//!   - [`MarketProfile`]: Bundled facade exposing all market rules for a specific exchange/jurisdiction.
//!   - [`MarketRegistry`]: Thread-safe discovery and resolution of market profiles.
//!
//! - **Market Packs**:
//!   - `null`: Zero-dependency, offline test pack (`NullCalendar`, `NullCostSchedule`, `NullMarketProfile`).
//!   - `india` (`#[cfg(feature = "india")]`): Real-world Indian market pack (NSE/BSE, STT, NIFTY 50).

pub mod aggregator;
pub mod calendar;
pub mod costs;
pub mod error;
pub mod expiry;
pub mod null;
pub mod profile;
pub mod rules;
pub mod settlement;
pub mod universes;

#[cfg(feature = "india")]
pub mod india;

// Re-export core generic contracts at crate root
pub use aggregator::SessionBarAggregator;
pub use calendar::{HolidaySource, MarketCalendar, Session, TradingCalendar};
pub use costs::{
    Charge, CostModelSource, CostSchedule, DatedCostScheduleSource, FeeBreakdown, MarketSegment,
    edge_after_costs,
};
pub use error::{MarketError, Result};
pub use expiry::{ExpiryRules, LastThursdayExpiry};
pub use null::{
    NullCalendar, NullCostSchedule, NullExpiryRules, NullInstrumentRulesProvider, NullMarginModel,
    NullMarketProfile, NullSymbolGrammar,
};
pub use profile::{MarketProfile, MarketRegistry};
pub use rules::{
    InstrumentRules, InstrumentRulesProvider, PriceBand, PriceViolation, QuantityViolation,
    SymbolGrammar,
};
pub use settlement::{
    MarginModel, MarginRequirement, SettlementRules, SettlementSchedule, SettlementType,
    StandardRollingSettlement,
};
pub use universes::{StaticUniverse, Universe, UniverseSource};

// Backward-compatible re-exports for India types when the feature is enabled
#[cfg(feature = "india")]
pub use india::calendar::nse::NseCalendar;
#[cfg(feature = "india")]
pub use india::costs::model::{CostBreakdown, CostModel, Segment};
#[cfg(feature = "india")]
pub use india::costs::stt::SttRates;
#[cfg(feature = "india")]
pub use india::error::IndiaError;
#[cfg(feature = "india")]
pub use india::fno::{
    FnoContract, FnoExpiry, FnoParseError, default_fno_lot_size, format_fno_symbol,
    parse_fno_symbol,
};
#[cfg(feature = "india")]
pub use india::profile::IndiaMarketProfile;
#[cfg(feature = "india")]
pub use india::universes::nifty50::{NIFTY50_SIZE, Nifty50};

#[cfg(test)]
mod tests;
