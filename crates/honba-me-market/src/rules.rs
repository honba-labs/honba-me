//! Generic instrument trading rules, price bands, and symbol grammar.

use honba_entities::Instrument;
use honba_messages::InstrumentId;
use serde::{Deserialize, Serialize};

use crate::{MarketError, Result};

/// Price band (circuit limits) for an instrument on a trading day.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PriceBand {
    /// Lower circuit / floor limit price.
    pub lower: f64,
    /// Upper circuit / ceiling limit price.
    pub upper: f64,
}

impl PriceBand {
    /// Creates a new price band.
    pub fn new(lower: f64, upper: f64) -> Self {
        debug_assert!(upper >= lower, "upper band must be >= lower band");
        Self { lower, upper }
    }

    /// Checks if a price is within the band.
    pub fn contains(&self, price: f64) -> bool {
        price >= self.lower && price <= self.upper
    }
}

/// Why an order quantity breaks an instrument's rules (ADR 0018 decision 4a).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QuantityViolation {
    /// The quantity is below the minimum order quantity.
    BelowMin {
        /// The offending quantity.
        quantity: f64,
        /// The minimum allowed quantity.
        min: f64,
    },
    /// The quantity exceeds the freeze (maximum single order) quantity.
    OverFreeze {
        /// The offending quantity.
        quantity: f64,
        /// The maximum allowed quantity.
        max: f64,
    },
    /// The quantity is not an exact multiple of the lot size.
    NotLotMultiple {
        /// The offending quantity.
        quantity: f64,
        /// The lot size it must be a multiple of.
        lot: f64,
    },
}

impl std::fmt::Display for QuantityViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BelowMin { quantity, min } => {
                write!(f, "quantity {quantity} below minimum {min}")
            }
            Self::OverFreeze { quantity, max } => {
                write!(f, "quantity {quantity} exceeds maximum freeze limit {max}")
            }
            Self::NotLotMultiple { quantity, lot } => {
                write!(
                    f,
                    "quantity {quantity} is not an exact multiple of lot size {lot}"
                )
            }
        }
    }
}

impl std::error::Error for QuantityViolation {}

impl From<QuantityViolation> for MarketError {
    fn from(v: QuantityViolation) -> Self {
        MarketError::RuleViolation(v.to_string())
    }
}

/// Ticks of float noise tolerated around a tick boundary; equals `honba-entities`'
/// `LOT_TICK_TOLERANCE` so `Instrument::is_on_tick` and the rules agree.
const PRICE_TICK_TOLERANCE: f64 = 1e-6;

/// Why an order price breaks an instrument's rules (ADR 0018 decision 4a).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PriceViolation {
    /// The price is zero, negative or not a number.
    NonPositive {
        /// The offending price.
        price: f64,
    },
    /// The price is not on a multiple of the tick size.
    OffTick {
        /// The offending price.
        price: f64,
        /// The tick size it must be a multiple of.
        tick: f64,
    },
}

impl std::fmt::Display for PriceViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonPositive { price } => write!(f, "price {price} must be positive"),
            Self::OffTick { price, tick } => {
                write!(f, "price {price} does not conform to tick size {tick}")
            }
        }
    }
}

impl std::error::Error for PriceViolation {}

impl From<PriceViolation> for MarketError {
    fn from(v: PriceViolation) -> Self {
        MarketError::RuleViolation(v.to_string())
    }
}

/// Trading rules governing lot sizes, tick sizes, freeze quantities, and price limits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InstrumentRules {
    /// Minimum tradable lot size.
    pub lot_size: f64,
    /// Minimum price increment.
    pub tick_size: f64,
    /// Minimum allowed order quantity.
    pub min_order_quantity: f64,
    /// Maximum allowed single order quantity (freeze / max clip size). None if unlimited.
    pub max_order_quantity: Option<f64>,
}

impl InstrumentRules {
    /// Creates basic instrument rules from lot size and tick size.
    pub fn new(lot_size: f64, tick_size: f64) -> Self {
        Self {
            lot_size,
            tick_size,
            min_order_quantity: lot_size,
            max_order_quantity: None,
        }
    }

    /// Sets the maximum single order quantity.
    pub fn with_max_quantity(mut self, max_quantity: f64) -> Self {
        self.max_order_quantity = Some(max_quantity);
        self
    }

    /// Validates whether an order quantity complies with lot size and quantity limits.
    ///
    /// Checks run in order: minimum, freeze quantity, lot multiple; the first
    /// failure is returned with the numbers it needs.
    pub fn validate_quantity(&self, quantity: f64) -> std::result::Result<(), QuantityViolation> {
        if quantity < self.min_order_quantity {
            return Err(QuantityViolation::BelowMin {
                quantity,
                min: self.min_order_quantity,
            });
        }

        if let Some(max_qty) = self.max_order_quantity {
            if quantity > max_qty {
                return Err(QuantityViolation::OverFreeze {
                    quantity,
                    max: max_qty,
                });
            }
        }

        let rem = (quantity / self.lot_size).fract();
        if rem.abs() > 1e-6 && (1.0 - rem.abs()) > 1e-6 {
            return Err(QuantityViolation::NotLotMultiple {
                quantity,
                lot: self.lot_size,
            });
        }

        Ok(())
    }

    /// Validates whether an order price is positive and on the tick grid.
    ///
    /// The tick tolerance (1e-6 ticks) matches `Instrument::is_on_tick` in
    /// `honba-entities`, so the two agree on every price.
    pub fn validate_price(&self, price: f64) -> std::result::Result<(), PriceViolation> {
        if price.is_nan() || price <= 0.0 {
            return Err(PriceViolation::NonPositive { price });
        }

        let ticks = price / self.tick_size;
        if !price.is_finite() || (ticks - ticks.round()).abs() >= PRICE_TICK_TOLERANCE {
            return Err(PriceViolation::OffTick {
                price,
                tick: self.tick_size,
            });
        }

        Ok(())
    }
}

/// Generic provider of instrument rules for symbols.
pub trait InstrumentRulesProvider: Send + Sync {
    /// Returns the trading rules for a given instrument.
    fn rules_for(&self, instrument: &Instrument) -> InstrumentRules {
        InstrumentRules::new(instrument.lot_size(), instrument.tick_size())
    }

    /// Returns the daily price band (circuits) if applicable.
    fn price_band(&self, _id: &InstrumentId) -> Option<PriceBand> {
        None
    }
}

/// Symbol grammar defining naming, normalization, and ticker parsing.
pub trait SymbolGrammar: Send + Sync {
    /// Normalizes an input symbol to the exchange canonical representation.
    fn normalize(&self, raw: &str) -> String {
        raw.trim().to_uppercase()
    }

    /// Validates whether a raw symbol conforms to market grammar.
    fn validate(&self, raw: &str) -> Result<()> {
        if raw.trim().is_empty() {
            Err(MarketError::InvalidSymbol {
                symbol: raw.to_string(),
                reason: "symbol cannot be empty".to_string(),
            })
        } else {
            Ok(())
        }
    }

    /// Resolves a raw symbol into an [`Instrument`] with appropriate metadata, if supported by the grammar.
    fn resolve_instrument(&self, raw: &str) -> Result<Instrument> {
        let _ = raw;
        Err(MarketError::InvalidSymbol {
            symbol: raw.to_string(),
            reason: "instrument resolution not supported by this grammar".to_string(),
        })
    }
}
