//! Indian National Stock Exchange (NSE) Futures & Options (F&O) symbol grammar and contract models.
//!
//! # Format Specifications
//! - **Monthly Futures**: `<UNDERLYING><YY><MMM>FUT` (e.g. `NIFTY24OCTFUT`, `RELIANCE24OCTFUT`)
//! - **Weekly Futures**: `<UNDERLYING><YY><M><DD>FUT` (e.g. `NIFTY24O17FUT`)
//! - **Monthly Options**: `<UNDERLYING><YY><MMM><STRIKE><CE|PE>` (e.g. `NIFTY24OCT24000CE`, `CESC24OCT180CE`)
//! - **Weekly Options**: `<UNDERLYING><YY><M><DD><STRIKE><CE|PE>` (e.g. `NIFTY24O1724000CE`, `FINNIFTY2410921000CE`)
//!
//! Weekly month codes: `1`..`9` (Jan..Sep), `O` (Oct), `N` (Nov), `D` (Dec).

use chrono::NaiveDate;
use honba_core::{Exchange, InstrumentId, Price, Quantity};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::expiry::{ExpiryRules, InstrumentKind, LastThursdayExpiry};

/// Option type (Call or Put).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OptionType {
    Call,
    Put,
}

impl OptionType {
    /// Returns the single-character code ("CE" or "PE").
    pub fn code(&self) -> &'static str {
        match self {
            OptionType::Call => "CE",
            OptionType::Put => "PE",
        }
    }
}

/// Errors encountered while parsing an Indian F&O contract symbol.
#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum FnoParseError {
    /// The input symbol was empty or only whitespace.
    #[error("symbol cannot be empty")]
    EmptySymbol,
    /// The contract format did not conform to the NSE F&O grammar.
    #[error("invalid contract format: {0}")]
    InvalidFormat(String),
    /// The expiry token could not be parsed into a valid monthly or weekly expiry.
    #[error("invalid expiry token {0:?}: {1}")]
    InvalidExpiry(String, String),
    /// The strike price was missing, malformed, or non-positive.
    #[error("invalid strike price: {0}")]
    InvalidStrike(String),
    /// The underlying symbol was invalid or empty.
    #[error("invalid underlying symbol: {0}")]
    InvalidUnderlying(String),
}

/// Represents the expiration date or month of an F&O contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FnoExpiry {
    /// Monthly contract expiry (e.g. `24OCT`).
    Monthly {
        /// Four-digit calendar year (e.g. 2024).
        year: i32,
        /// Month of the year (1..=12).
        month: u32,
    },
    /// Weekly contract expiry (e.g. `24O17`, `24109`).
    Weekly {
        /// Four-digit calendar year (e.g. 2024).
        year: i32,
        /// Month of the year (1..=12).
        month: u32,
        /// Day of the month (1..=31).
        day: u32,
    },
}

impl FnoExpiry {
    /// Creates a monthly contract expiry.
    pub fn monthly(year: i32, month: u32) -> Self {
        Self::Monthly { year, month }
    }

    /// Creates a weekly contract expiry.
    pub fn weekly(year: i32, month: u32, day: u32) -> Self {
        Self::Weekly { year, month, day }
    }

    /// Returns the calendar year.
    pub fn year(&self) -> i32 {
        match *self {
            Self::Monthly { year, .. } | Self::Weekly { year, .. } => year,
        }
    }

    /// Returns the month of the year (1..=12).
    pub fn month(&self) -> u32 {
        match *self {
            Self::Monthly { month, .. } | Self::Weekly { month, .. } => month,
        }
    }

    /// Returns the day of the month if this is a weekly contract.
    pub fn day(&self) -> Option<u32> {
        match *self {
            Self::Monthly { .. } => None,
            Self::Weekly { day, .. } => Some(day),
        }
    }

    /// Returns `true` if this is a weekly contract expiry.
    pub fn is_weekly(&self) -> bool {
        matches!(self, Self::Weekly { .. })
    }

    /// Returns `true` if this is a monthly contract expiry.
    pub fn is_monthly(&self) -> bool {
        matches!(self, Self::Monthly { .. })
    }

    /// Returns the nominal expiration date (last Thursday for monthly contracts).
    pub fn expiry_date(&self) -> Option<NaiveDate> {
        match *self {
            Self::Monthly { year, month } => {
                LastThursdayExpiry.expiry_for_month(year, month, InstrumentKind::Future)
            }
            Self::Weekly { year, month, day } => NaiveDate::from_ymd_opt(year, month, day),
        }
    }

    /// Formats the expiry as a 5-character token (e.g. `24OCT` or `24O17`).
    pub fn format_token(&self) -> String {
        match *self {
            Self::Monthly { year, month } => {
                let yy = (year.rem_euclid(100)) as u32;
                format!("{:02}{}", yy, month_to_str(month))
            }
            Self::Weekly { year, month, day } => {
                let yy = (year.rem_euclid(100)) as u32;
                let m_char = month_to_weekly_char(month).unwrap_or('?');
                format!("{:02}{}{:02}", yy, m_char, day)
            }
        }
    }
}

impl std::fmt::Display for FnoExpiry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.format_token())
    }
}

/// Parsed Indian F&O contract (Future or Option).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FnoContract {
    /// A futures contract.
    Future {
        /// The underlying symbol or index (e.g. `NIFTY`, `RELIANCE`).
        underlying: String,
        /// The contract expiration.
        expiry: FnoExpiry,
    },
    /// An options contract.
    Option {
        /// The underlying symbol or index (e.g. `NIFTY`, `BANKNIFTY`, `CESC`).
        underlying: String,
        /// The contract expiration.
        expiry: FnoExpiry,
        /// The strike price.
        strike: Price,
        /// Call or Put option type.
        option_type: OptionType,
    },
}

impl FnoContract {
    /// Returns the underlying symbol or index.
    pub fn underlying(&self) -> &str {
        match self {
            Self::Future { underlying, .. } | Self::Option { underlying, .. } => underlying,
        }
    }

    /// Returns the contract expiration.
    pub fn expiry(&self) -> &FnoExpiry {
        match self {
            Self::Future { expiry, .. } | Self::Option { expiry, .. } => expiry,
        }
    }

    /// Returns the strike price if this is an option contract.
    pub fn strike(&self) -> Option<Price> {
        match self {
            Self::Future { .. } => None,
            Self::Option { strike, .. } => Some(*strike),
        }
    }

    /// Returns the option type if this is an option contract.
    pub fn option_type(&self) -> Option<OptionType> {
        match self {
            Self::Future { .. } => None,
            Self::Option { option_type, .. } => Some(*option_type),
        }
    }

    /// Returns the instrument kind (`Future` or `Option`).
    pub fn kind(&self) -> InstrumentKind {
        match self {
            Self::Future { .. } => InstrumentKind::Future,
            Self::Option { .. } => InstrumentKind::Option,
        }
    }

    /// Formats this contract into canonical NSE ticker text.
    pub fn format_symbol(&self) -> String {
        format_fno_symbol(self)
    }

    /// Converts this contract into an [`InstrumentId`] with the given exchange and lot/tick parameters.
    pub fn to_instrument_id(
        &self,
        exchange: Exchange,
        lot_size: Quantity,
        tick_size: Price,
    ) -> InstrumentId {
        let symbol = self.format_symbol();
        let expiry_date = self.expiry().expiry_date();
        let _ = expiry_date;
        let _ = lot_size;
        let _ = tick_size;
        InstrumentId::new(exchange, symbol)
    }
}

impl std::fmt::Display for FnoContract {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.format_symbol())
    }
}

/// Converts a 1-based month number into standard 3-letter uppercase abbreviation.
pub fn month_to_str(month: u32) -> &'static str {
    match month {
        1 => "JAN",
        2 => "FEB",
        3 => "MAR",
        4 => "APR",
        5 => "MAY",
        6 => "JUN",
        7 => "JUL",
        8 => "AUG",
        9 => "SEP",
        10 => "OCT",
        11 => "NOV",
        12 => "DEC",
        _ => "UNK",
    }
}

/// Parses a 3-letter month abbreviation into a 1-based month number.
pub fn month_from_str(s: &str) -> Option<u32> {
    match s.to_ascii_uppercase().as_str() {
        "JAN" => Some(1),
        "FEB" => Some(2),
        "MAR" => Some(3),
        "APR" => Some(4),
        "MAY" => Some(5),
        "JUN" => Some(6),
        "JUL" => Some(7),
        "AUG" => Some(8),
        "SEP" => Some(9),
        "OCT" => Some(10),
        "NOV" => Some(11),
        "DEC" => Some(12),
        _ => None,
    }
}

/// Converts a 1-based month number into the NSE weekly single-character code (`1`..`9`, `O`, `N`, `D`).
pub fn month_to_weekly_char(month: u32) -> Option<char> {
    match month {
        1..=9 => char::from_digit(month, 10),
        10 => Some('O'),
        11 => Some('N'),
        12 => Some('D'),
        _ => None,
    }
}

/// Parses an NSE weekly single-character month code (`1`..`9`, `O`, `N`, `D`) into a 1-based month number.
pub fn month_from_weekly_char(c: char) -> Option<u32> {
    match c.to_ascii_uppercase() {
        '1'..='9' => c.to_digit(10),
        'O' => Some(10),
        'N' => Some(11),
        'D' => Some(12),
        _ => None,
    }
}

/// Parses a 5-character expiry token (e.g. `24OCT` or `24O17`).
pub fn parse_expiry_token(token: &str) -> Result<FnoExpiry, FnoParseError> {
    if token.len() != 5 {
        return Err(FnoParseError::InvalidExpiry(
            token.to_string(),
            "token must be exactly 5 characters".to_string(),
        ));
    }

    let yy_str = &token[0..2];
    let yy = yy_str.parse::<i32>().map_err(|_| {
        FnoParseError::InvalidExpiry(
            token.to_string(),
            "first 2 chars must be digits".to_string(),
        )
    })?;
    let year = 2000 + yy;

    let sub = &token[2..5];
    if let Some(month) = month_from_str(sub) {
        return Ok(FnoExpiry::Monthly { year, month });
    }

    // Weekly expiry: 1 char month ('1'..='9', 'O', 'N', 'D') + 2 digit day
    let m_char = token.chars().nth(2).unwrap();
    if let Some(month) = month_from_weekly_char(m_char) {
        let dd_str = &token[3..5];
        if let Ok(day) = dd_str.parse::<u32>() {
            if (1..=31).contains(&day) {
                return Ok(FnoExpiry::Weekly { year, month, day });
            }
        }
    }

    Err(FnoParseError::InvalidExpiry(
        token.to_string(),
        "unrecognized expiry pattern (expected YYMMM or YYMDD)".to_string(),
    ))
}

fn format_strike(strike: Price) -> String {
    if strike.0.fract() == Decimal::ZERO {
        format!("{}", strike.0)
    } else {
        let s = format!("{:.4}", strike.0);
        let s = s.trim_end_matches('0');
        let s = s.trim_end_matches('.');
        s.to_string()
    }
}

fn is_valid_underlying(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '&' || c == '-' || c == '_')
}

/// Parses a raw Indian F&O contract ticker symbol into a structured [`FnoContract`].
///
/// Strips optional exchange prefix (e.g. `NSE:NIFTY24OCTFUT` -> `NIFTY24OCTFUT`).
pub fn parse_fno_symbol(raw: &str) -> Result<FnoContract, FnoParseError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(FnoParseError::EmptySymbol);
    }

    let symbol = match trimmed.split_once(':') {
        Some((_, sym)) => sym.trim(),
        None => trimmed,
    };
    if symbol.is_empty() {
        return Err(FnoParseError::EmptySymbol);
    }

    let upper = symbol.to_ascii_uppercase();

    // 1. Futures: ends with FUT
    if upper.ends_with("FUT") {
        let rem = &upper[..upper.len() - 3];
        if rem.len() < 6 {
            return Err(FnoParseError::InvalidFormat(raw.to_string()));
        }
        let (underlying, expiry_token) = rem.split_at(rem.len() - 5);
        if !is_valid_underlying(underlying) {
            return Err(FnoParseError::InvalidUnderlying(underlying.to_string()));
        }
        let expiry = parse_expiry_token(expiry_token)?;
        return Ok(FnoContract::Future {
            underlying: underlying.to_string(),
            expiry,
        });
    }

    // 2. Options: ends with CE or PE
    if upper.ends_with("CE") || upper.ends_with("PE") {
        let option_type = if upper.ends_with("CE") {
            OptionType::Call
        } else {
            OptionType::Put
        };
        let rem = &upper[..upper.len() - 2];
        if rem.len() < 7 {
            return Err(FnoParseError::InvalidFormat(raw.to_string()));
        }

        // Must end with a digit before the CE/PE suffix (the strike price)
        if !rem.chars().last().is_some_and(|c| c.is_ascii_digit()) {
            return Err(FnoParseError::InvalidStrike(format!(
                "symbol {raw} has no numeric strike price before option type"
            )));
        }

        // Search for the 5-character expiry token `rem[i..i+5]`
        let mut candidates = Vec::new();
        for i in 1..=rem.len().saturating_sub(6) {
            let expiry_slice = &rem[i..i + 5];
            if let Ok(exp) = parse_expiry_token(expiry_slice) {
                let strike_str = &rem[i + 5..];
                if let Ok(strike_val) = strike_str.parse::<f64>() {
                    if strike_val > 0.0 {
                        let underlying = &rem[..i];
                        if is_valid_underlying(underlying) {
                            let strike_price = Price::from_rupees(strike_val.to_string().as_str()).unwrap_or(Price::ZERO);
                            candidates.push((underlying.to_string(), exp, strike_price));
                        }
                    }
                }
            }
        }

        if candidates.is_empty() {
            return Err(FnoParseError::InvalidFormat(raw.to_string()));
        }

        // If multiple candidates match, prioritize monthly expiry, then the first matching
        let best = if let Some(monthly) = candidates.iter().find(|(_, exp, _)| exp.is_monthly()) {
            monthly.clone()
        } else {
            candidates.into_iter().next().unwrap()
        };

        return Ok(FnoContract::Option {
            underlying: best.0,
            expiry: best.1,
            strike: best.2,
            option_type,
        });
    }

    Err(FnoParseError::InvalidFormat(format!(
        "unrecognized F&O contract suffix in {raw}; expected FUT, CE, or PE"
    )))
}

/// Formats a structured [`FnoContract`] into canonical NSE ticker symbol text.
pub fn format_fno_symbol(contract: &FnoContract) -> String {
    match contract {
        FnoContract::Future { underlying, expiry } => {
            format!("{}{}{}", underlying, expiry.format_token(), "FUT")
        }
        FnoContract::Option {
            underlying,
            expiry,
            strike,
            option_type,
        } => {
            format!(
                "{}{}{}{}",
                underlying,
                expiry.format_token(),
                format_strike(*strike),
                option_type.code()
            )
        }
    }
}

/// Returns standard NSE default lot size for well-known indices or 1 for equities.
pub fn default_fno_lot_size(underlying: &str) -> Quantity {
    match underlying.to_ascii_uppercase().as_str() {
        "NIFTY" => Quantity::from_u64(25),
        "BANKNIFTY" => Quantity::from_u64(15),
        "FINNIFTY" => Quantity::from_u64(25),
        "MIDCPNIFTY" => Quantity::from_u64(50),
        _ => Quantity::from_u64(1),
    }
}
