//! India-bound exchanges and instrument-text parsing.
//!
//! `honba_messages::Exchange` is an opaque code; this module owns which codes
//! are valid for Indian markets and how a user-facing instrument string
//! (`INFY`, or TradingView-style `NSE:INFY`) resolves to an `InstrumentId`.
//! Pure: no I/O.

use std::fmt;
use std::str::FromStr;

use honba_messages::{Exchange, InstrumentId};
use thiserror::Error;

/// An Indian exchange that Honba can load and trade instruments for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IndiaExchange {
    /// National Stock Exchange (default).
    Nse,
    /// BSE (Bombay Stock Exchange).
    Bse,
}

impl IndiaExchange {
    /// Every supported exchange, in display order.
    pub const ALL: [IndiaExchange; 2] = [IndiaExchange::Nse, IndiaExchange::Bse];

    /// The upper-case exchange code.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Nse => "NSE",
            Self::Bse => "BSE",
        }
    }
}

impl fmt::Display for IndiaExchange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<IndiaExchange> for Exchange {
    fn from(value: IndiaExchange) -> Self {
        Exchange::new(value.as_str())
    }
}

/// Errors from parsing an exchange or instrument string.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum InstrumentParseError {
    /// The exchange code is not a supported India exchange.
    #[error("unknown exchange {0:?} (supported: NSE, BSE)")]
    UnknownExchange(String),
    /// The symbol part is empty or malformed.
    #[error("invalid symbol {0:?} (expected SYMBOL or EXCHANGE:SYMBOL)")]
    InvalidSymbol(String),
    /// The symbol's exchange qualifier disagrees with the explicit exchange.
    #[error("symbol qualifier {qualified} conflicts with exchange {explicit}")]
    Conflict {
        /// Exchange taken from the `EXCHANGE:SYMBOL` qualifier.
        qualified: IndiaExchange,
        /// Exchange given explicitly (flag).
        explicit: IndiaExchange,
    },
}

impl FromStr for IndiaExchange {
    type Err = InstrumentParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let code = s.trim();
        Self::ALL
            .into_iter()
            .find(|e| e.as_str().eq_ignore_ascii_case(code))
            .ok_or_else(|| InstrumentParseError::UnknownExchange(code.to_owned()))
    }
}

/// Resolves `text` (`SYMBOL` or `EXCHANGE:SYMBOL`) and an optional explicit
/// exchange into an [`InstrumentId`]. Without either, the exchange is NSE.
///
/// ```
/// use honba_market::india::exchange::resolve_instrument;
///
/// let id = resolve_instrument("BSE:INFY", None).unwrap();
/// assert_eq!(id.to_string(), "INFY.BSE");
/// ```
pub fn resolve_instrument(
    text: &str,
    explicit: Option<&str>,
) -> Result<InstrumentId, InstrumentParseError> {
    let invalid = || InstrumentParseError::InvalidSymbol(text.to_owned());
    let (qualified, symbol) = match text.split_once(':') {
        Some((ex, _)) if ex.trim().is_empty() => return Err(invalid()),
        Some((ex, sym)) => (Some(ex.parse::<IndiaExchange>()?), sym),
        None => (None, text),
    };
    let symbol = symbol.trim();
    if symbol.is_empty() || symbol.contains(':') || symbol.chars().any(char::is_whitespace) {
        return Err(invalid());
    }
    let explicit = explicit.map(str::parse::<IndiaExchange>).transpose()?;
    let exchange = match (qualified, explicit) {
        (Some(q), Some(e)) if q != e => {
            return Err(InstrumentParseError::Conflict {
                qualified: q,
                explicit: e,
            })
        }
        (Some(e), _) | (None, Some(e)) => e,
        (None, None) => IndiaExchange::Nse,
    };
    Ok(InstrumentId::new(symbol, exchange.into()))
}
