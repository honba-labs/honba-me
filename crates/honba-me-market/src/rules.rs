//! Generic instrument trading rules, price bands, and symbol grammar.
//!
//! All prices and quantities use canonical [`honba_core`] types (`Price`, `Quantity`)
//! with exact [`rust_decimal::Decimal`] arithmetic — no floating point.

use honba_core::{InstrumentId, Price, Quantity};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

use crate::{MarketError, Result};

/// Price band (circuit limits) for an instrument on a trading day.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PriceBand {
    /// Lower circuit / floor limit price.
    pub lower: Price,
    /// Upper circuit / ceiling limit price.
    pub upper: Price,
    /// Reference price (previous close) used for band calculation.
    pub reference_price: Price,
}

impl PriceBand {
    /// Creates a new price band.
    pub fn new(lower: Price, upper: Price, reference_price: Price) -> Self {
        debug_assert!(upper >= lower, "upper band must be >= lower band");
        Self {
            lower,
            upper,
            reference_price,
        }
    }

    /// Checks if a price is within the band.
    pub fn contains(&self, price: Price) -> bool {
        price >= self.lower && price <= self.upper
    }
}

/// Why an order quantity breaks an instrument's rules (ADR 0018 decision 4a).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QuantityViolation {
    /// The quantity is below the minimum order quantity.
    BelowMin {
        /// The offending quantity.
        quantity: Quantity,
        /// The minimum allowed quantity.
        min: Quantity,
    },
    /// The quantity exceeds the freeze (maximum single order) quantity.
    OverFreeze {
        /// The offending quantity.
        quantity: Quantity,
        /// The maximum allowed quantity.
        max: Quantity,
    },
    /// The quantity is not an exact multiple of the lot size.
    NotLotMultiple {
        /// The offending quantity.
        quantity: Quantity,
        /// The lot size it must be a multiple of.
        lot: Quantity,
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

/// Why an order price breaks an instrument's rules (ADR 0018 decision 4a).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PriceViolation {
    /// The price is zero, negative or not a number.
    NonPositive {
        /// The offending price.
        price: Price,
    },
    /// The price is not on a multiple of the tick size.
    OffTick {
        /// The offending price.
        price: Price,
        /// The tick size it must be a multiple of.
        tick: Price,
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
    pub lot_size: Quantity,
    /// Minimum price increment.
    pub tick_size: Price,
    /// Minimum allowed order quantity.
    pub min_order_quantity: Quantity,
    /// Maximum allowed single order quantity (freeze / max clip size). None if unlimited.
    pub max_order_quantity: Option<Quantity>,
}

impl InstrumentRules {
    /// Creates basic instrument rules from lot size and tick size.
    pub fn new(lot_size: Quantity, tick_size: Price) -> Self {
        Self {
            lot_size,
            tick_size,
            min_order_quantity: lot_size,
            max_order_quantity: None,
        }
    }

    /// Sets the maximum single order quantity.
    pub fn with_max_quantity(mut self, max_quantity: Quantity) -> Self {
        self.max_order_quantity = Some(max_quantity);
        self
    }

    /// Validates whether an order quantity complies with lot size and quantity limits.
    ///
    /// Checks run in order: minimum, freeze quantity, lot multiple; the first
    /// failure is returned with the numbers it needs.
    pub fn validate_quantity(
        &self,
        quantity: Quantity,
    ) -> std::result::Result<(), QuantityViolation> {
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

        // Check lot multiple: quantity / lot_size should be an integer
        let ratio = quantity.0 / self.lot_size.0;
        let fract = ratio - ratio.trunc();
        let tolerance = Decimal::from_str("1e-6").unwrap();
        if fract.abs() > tolerance && (Decimal::ONE - fract.abs()) > tolerance {
            return Err(QuantityViolation::NotLotMultiple {
                quantity,
                lot: self.lot_size,
            });
        }

        Ok(())
    }

    /// Validates whether an order price is positive and on the tick grid.
    pub fn validate_price(&self, price: Price) -> std::result::Result<(), PriceViolation> {
        if !price.is_positive() {
            return Err(PriceViolation::NonPositive { price });
        }

        let ticks = price.0 / self.tick_size.0;
        let rounded = ticks.round();
        let diff = (ticks - rounded).abs();
        let tolerance = Decimal::from_str("1e-6").unwrap();
        if diff >= tolerance {
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
    fn rules_for(&self, instrument: &InstrumentId) -> InstrumentRules {
        let _ = instrument;
        InstrumentRules::new(Quantity::ZERO, Price::ZERO)
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

    /// Resolves a raw symbol into canonical instrument metadata.
    fn resolve_instrument(&self, raw: &str) -> Result<InstrumentId> {
        let _ = raw;
        Err(MarketError::InvalidSymbol {
            symbol: raw.to_string(),
            reason: "instrument resolution not supported by this grammar".to_string(),
        })
    }
}
