//! MarketProfile bundle and MarketRegistry for pluggable market packs.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use honba_entities::{Currency, Instrument};

use crate::calendar::MarketCalendar;
use crate::costs::CostSchedule;
use crate::expiry::ExpiryRules;
use crate::rules::{InstrumentRulesProvider, SymbolGrammar};
use crate::settlement::{MarginModel, SettlementRules};
use crate::{MarketError, Result};

/// A comprehensive market profile bundling calendar, rules, costs, margin, and settlement.
pub trait MarketProfile: Send + Sync {
    /// Canonical market identifier code (e.g., "nse_bse", "null").
    fn market_code(&self) -> &str;

    /// Primary trading and settlement currency.
    fn currency(&self) -> Currency;

    /// The trading calendar.
    fn calendar(&self) -> &dyn MarketCalendar;

    /// Transaction cost schedule.
    fn cost_schedule(&self) -> &dyn CostSchedule;

    /// Instrument rules provider (lot sizes, tick sizes, circuit bands).
    fn instrument_rules(&self) -> &dyn InstrumentRulesProvider;

    /// Symbol grammar and ticker normalization.
    fn symbol_grammar(&self) -> &dyn SymbolGrammar;

    /// Derivative expiry rules.
    fn expiry_rules(&self) -> &dyn ExpiryRules;

    /// Margin calculation model.
    fn margin_model(&self) -> &dyn MarginModel;

    /// Settlement rules and cycle timing.
    fn settlement_rules(&self) -> &dyn SettlementRules;

    /// Resolves an instrument symbol using this market's symbol grammar.
    fn resolve_instrument(&self, symbol: &str) -> Result<Instrument> {
        self.symbol_grammar().resolve_instrument(symbol)
    }
}

/// Thread-safe registry mapping market codes to `MarketProfile` instances.
#[derive(Default)]
pub struct MarketRegistry {
    profiles: RwLock<HashMap<String, Arc<dyn MarketProfile>>>,
}

impl MarketRegistry {
    /// Creates a new empty market registry.
    pub fn new() -> Self {
        Self {
            profiles: RwLock::new(HashMap::new()),
        }
    }

    /// Registers a market profile under its `market_code`.
    pub fn register(&self, profile: Arc<dyn MarketProfile>) {
        let mut map = self
            .profiles
            .write()
            .expect("market registry write lock poisoned");
        map.insert(profile.market_code().to_string(), profile);
    }

    /// Retrieves a market profile by code.
    pub fn get(&self, market_code: &str) -> Result<Arc<dyn MarketProfile>> {
        let map = self
            .profiles
            .read()
            .expect("market registry read lock poisoned");
        map.get(market_code)
            .cloned()
            .ok_or_else(|| MarketError::MarketNotFound(market_code.to_string()))
    }

    /// Returns a list of all registered market codes.
    pub fn available_markets(&self) -> Vec<String> {
        let map = self
            .profiles
            .read()
            .expect("market registry read lock poisoned");
        let mut keys: Vec<String> = map.keys().cloned().collect();
        keys.sort();
        keys
    }

    /// Creates a default pre-configured registry containing the `null` pack and enabled feature packs.
    pub fn default_registry() -> Self {
        let reg = Self::new();
        // Register null market pack
        reg.register(Arc::new(crate::null::NullMarketProfile::default()));

        #[cfg(feature = "india")]
        {
            reg.register(Arc::new(
                crate::india::profile::IndiaMarketProfile::default(),
            ));
        }

        reg
    }
}
