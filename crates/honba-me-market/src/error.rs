//! Error types for market operations.

use thiserror::Error;

/// Convenience alias for market operation results.
pub type Result<T> = std::result::Result<T, MarketError>;

/// Errors that market operations can produce.
#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum MarketError {
    /// A date or timestamp could not be constructed or is invalid for the market.
    #[error("invalid date: {0}")]
    InvalidDate(String),

    /// An operation referenced a market or segment that isn't recognized.
    #[error("unknown segment: {0}")]
    UnknownSegment(String),

    /// A symbol is not found or not recognized.
    #[error("symbol not found: {0}")]
    SymbolNotFound(String),

    /// A symbol failed grammar / format validation.
    #[error("invalid symbol format '{symbol}': {reason}")]
    InvalidSymbol {
        /// The invalid symbol.
        symbol: String,
        /// Reason for failure.
        reason: String,
    },

    /// An order or price violates instrument trading rules.
    #[error("rule violation: {0}")]
    RuleViolation(String),

    /// The requested market pack is not registered.
    #[error("market not found in registry: {0}")]
    MarketNotFound(String),

    /// Market-specific error wrapper.
    #[error("market error: {0}")]
    Custom(String),
}
