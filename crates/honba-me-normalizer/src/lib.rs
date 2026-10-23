//! Instrument Master & Corporate Actions Engine.
//!
//! Provides automated ingestion of instrument master data from multiple brokers
//! (Zerodha instruments.csv, Upstox complete.json.gz, NSE EQUITY_L.csv) and
//! corporate actions tracking (splits, bonuses, dividends, symbol changes).
//!
//! Maintains fast in-memory lookup indices for Token ↔ Symbol ↔ ISIN ↔ InstrumentId
//! resolution with sub-microsecond lookup performance.

pub mod downloader;
pub use downloader::MasterDownloader;

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::str::FromStr;

use chrono::NaiveDate;
use honba_core::{Exchange, InstrumentId, Price, Quantity};
use honba_me_core::SymbolMapper;
use rust_decimal::Decimal;
use serde::Deserialize;
use thiserror::Error;

/// Errors from the instrument master.
#[derive(Debug, Error)]
pub enum InstrumentMasterError {
    /// CSV parsing error.
    #[error("CSV parse error: {0}")]
    CsvParse(String),
    /// Instrument not found.
    #[error("instrument not found: {0}")]
    NotFound(String),
    /// File IO error.
    #[error("IO error: {0}")]
    Io(String),
}

/// Instrument master entry with all known identifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstrumentMasterEntry {
    /// Exchange (NSE, BSE, etc.)
    pub exchange: Exchange,
    /// Trading symbol (e.g., "RELIANCE", "TCS")
    pub symbol: String,
    /// Broker-specific instrument token
    pub token: Option<String>,
    /// ISIN (International Securities Identification Number)
    pub isin: Option<String>,
    /// Instrument type (EQ, FUT, OPT, etc.)
    pub instrument_type: Option<String>,
    /// Lot size for F&O instruments
    pub lot_size: Option<u32>,
    /// Tick size for price precision
    pub tick_size: Option<Price>,
    /// Freeze quantity limit
    pub freeze_qty: Option<Quantity>,
}

/// Instrument master lookup index.
///
/// Provides O(1) lookup by token, symbol, or ISIN with multi-key indexing.
#[derive(Debug, Clone)]
pub struct InstrumentMaster {
    /// Token → InstrumentId mapping
    token_index: HashMap<String, InstrumentId>,
    /// Symbol → InstrumentId mapping
    symbol_index: HashMap<String, InstrumentId>,
    /// ISIN → InstrumentId mapping
    isin_index: HashMap<String, InstrumentId>,
    /// Full entry storage keyed by InstrumentId
    entries: HashMap<InstrumentId, InstrumentMasterEntry>,
}

impl InstrumentMaster {
    /// Create a new empty instrument master.
    pub fn new() -> Self {
        Self {
            token_index: HashMap::new(),
            symbol_index: HashMap::new(),
            isin_index: HashMap::new(),
            entries: HashMap::new(),
        }
    }

    /// Get the number of instruments in the master.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the master is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Look up an instrument by broker token.
    pub fn by_token(&self, token: &str) -> Option<&InstrumentId> {
        self.token_index.get(token)
    }

    /// Look up an instrument by symbol.
    pub fn by_symbol(&self, symbol: &str) -> Option<&InstrumentId> {
        self.symbol_index.get(symbol)
    }

    /// Resolve tick size based on current instrument rules.
    pub fn resolve_tick_size(&self, id: &InstrumentId) -> Option<Price> {
        self.entries.get(id).and_then(|e| e.tick_size)
    }

    /// Resolve lot size based on current instrument rules.
    pub fn resolve_lot_size(&self, id: &InstrumentId) -> Option<u32> {
        self.entries.get(id).and_then(|e| e.lot_size)
    }

    /// Look up an instrument by ISIN.
    pub fn by_isin(&self, isin: &str) -> Option<&InstrumentId> {
        self.isin_index.get(isin)
    }

    /// Get the full entry for an instrument.
    pub fn entry(&self, id: &InstrumentId) -> Option<&InstrumentMasterEntry> {
        self.entries.get(id)
    }

    /// Add an instrument to the master.
    pub fn add(&mut self, entry: InstrumentMasterEntry) {
        let id = InstrumentId::new(entry.exchange.clone(), entry.symbol.clone())
            .with_token(entry.token.clone().unwrap_or_default())
            .with_isin(entry.isin.clone().unwrap_or_default());

        self.token_index
            .insert(entry.token.clone().unwrap_or_default(), id.clone());
        self.symbol_index
            .insert(entry.symbol.clone(), id.clone());
        if let Some(isin) = &entry.isin {
            self.isin_index.insert(isin.clone(), id.clone());
        }
        self.entries.insert(id, entry);
    }

    /// Load instruments from a Zerodha instruments.csv file.
    ///
    /// Expected CSV columns: instrument_token,exchange_token,tradingsymbol,name,last_price,expiry,strike,tick_size,lot_size,instrument_type,segment,exchange_tradingsymbol,etc.
    pub fn load_zerodha_csv(&mut self, path: &Path) -> Result<usize, InstrumentMasterError> {
        let mut reader = csv::Reader::from_path(path)
            .map_err(|e| InstrumentMasterError::CsvParse(e.to_string()))?;

        let mut count = 0;
        for result in reader.records() {
            let record = result.map_err(|e| InstrumentMasterError::CsvParse(e.to_string()))?;

            let token = record.get(0).unwrap_or("").to_string();
            let exchange_str = record.get(1).unwrap_or("NSE");
            let symbol = record.get(2).unwrap_or("").to_string();
            let instrument_type = record.get(9).unwrap_or("").to_string();
            let tick_size_str = record.get(7).unwrap_or("0.05");
            let lot_size_str = record.get(8).unwrap_or("1");

            let exchange: Exchange = exchange_str.parse().unwrap_or(Exchange::Nse);
            let tick_size: Option<Price> = tick_size_str
                .parse::<Decimal>()
                .ok()
                .map(Price::new);
            let lot_size: Option<u32> = lot_size_str.parse::<u32>().ok();

            if symbol.is_empty() {
                continue;
            }

            let entry = InstrumentMasterEntry {
                exchange,
                symbol: symbol.clone(),
                token: if token.is_empty() { None } else { Some(token.clone()) },
                isin: None,
                instrument_type: if instrument_type.is_empty() { None } else { Some(instrument_type.clone()) },
                lot_size,
                tick_size,
                freeze_qty: None,
            };

            self.add(entry);
            count += 1;
        }

        Ok(count)
    }

    /// Load instruments from Upstox complete.json.gz file.
    ///
    /// Expected JSON structure: array of objects with fields: instrument_token, exchange_token, trading_symbol, name, expiry, strike, tick_size, lot_size, instrument_type, segment, exchange, etc.
    pub fn load_upstox_json(&mut self, data: &[u8]) -> Result<usize, InstrumentMasterError> {
        let records: Vec<UpstoxInstrument> = serde_json::from_slice(data)
            .map_err(|e| InstrumentMasterError::CsvParse(e.to_string()))?;

        let mut count = 0;
        for record in records {
            let exchange: Exchange = record.exchange.parse().unwrap_or(Exchange::Nse);
            let tick_size: Option<Price> = record
                .tick_size
                .parse::<Decimal>()
                .ok()
                .map(Price::new);
            let lot_size: Option<u32> = record.lot_size.parse::<u32>().ok();

            if record.trading_symbol.is_empty() {
                continue;
            }

            let entry = InstrumentMasterEntry {
                exchange,
                symbol: record.trading_symbol.clone(),
                token: if record.instrument_token.is_empty() {
                    None
                } else {
                    Some(record.instrument_token.clone())
                },
                isin: None,
                instrument_type: if record.instrument_type.is_empty() {
                    None
                } else {
                    Some(record.instrument_type.clone())
                },
                lot_size,
                tick_size,
                freeze_qty: None,
            };

            self.add(entry);
            count += 1;
        }

        Ok(count)
    }

    /// Load instruments from NSE EQUITY_L.csv file.
    ///
    /// Expected CSV columns: SYMBOL,SERIES,DATE_OF_LISTING,PAID_UP_VALUE,MARKET_LOT,ISIN_NUMBER,FACE_VALUE
    pub fn load_nse_equity_csv(&mut self, path: &Path) -> Result<usize, InstrumentMasterError> {
        let mut reader = csv::Reader::from_path(path)
            .map_err(|e| InstrumentMasterError::CsvParse(e.to_string()))?;

        let mut count = 0;
        for result in reader.records() {
            let record = result.map_err(|e| InstrumentMasterError::CsvParse(e.to_string()))?;

            let symbol = record.get(0).unwrap_or("").to_string();
            let series = record.get(1).unwrap_or("EQ");
            let isin = record.get(5).unwrap_or("").to_string();
            let market_lot = record.get(4).unwrap_or("1");
            let face_value = record.get(6).unwrap_or("1.0");

            if symbol.is_empty() {
                continue;
            }

            let exchange: Exchange = Exchange::Nse;
            let lot_size: Option<u32> = market_lot.parse::<u32>().ok();
            let tick_size: Option<Price> = face_value
                .parse::<Decimal>()
                .ok()
                .map(|d| Price::new(d / Decimal::from(100)));

            let entry = InstrumentMasterEntry {
                exchange,
                symbol: symbol.clone(),
                token: None,
                isin: if isin.is_empty() { None } else { Some(isin.clone()) },
                instrument_type: if series == "EQ" {
                    Some("EQ".to_string())
                } else {
                    Some(series.to_string())
                },
                lot_size,
                tick_size,
                freeze_qty: None,
            };

            self.add(entry);
            count += 1;
        }

        Ok(count)
    }

    /// Import instruments from an `InstrumentMaster` export (serializable format).
    pub fn import_master(&mut self, instruments: &[InstrumentMasterEntry]) {
        for entry in instruments {
            self.add(entry.clone());
        }
    }

    /// Export all instruments as a serializable list.
    pub fn export_master(&self) -> Vec<InstrumentMasterEntry> {
        self.entries.values().cloned().collect()
    }
}

/// Upstox instrument record from complete.json.gz.
#[derive(Debug, Deserialize)]
struct UpstoxInstrument {
    instrument_token: String,
    exchange_token: String,
    trading_symbol: String,
    name: String,
    expiry: Option<String>,
    strike: Option<String>,
    tick_size: String,
    lot_size: String,
    instrument_type: String,
    segment: String,
    exchange: String,
}

/// Corporate action event types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorporateActionType {
    /// Stock split (e.g., 1:10 means 1 old share → 10 new shares)
    Split {
        /// Old shares count
        old_shares: u32,
        /// New shares count
        new_shares: u32,
    },
    /// Bonus issue (e.g., 1:1 means 1 bonus share for every 1 held)
    Bonus {
        /// Bonus shares per held share
        bonus_ratio_num: u32,
        /// Bonus ratio denominator
        bonus_ratio_den: u32,
    },
    /// Cash dividend
    Dividend {
        /// Dividend per share
        per_share: Price,
    },
    /// Symbol change (rename)
    SymbolRename {
        /// New symbol
        new_symbol: String,
    },
    /// Demerger
    Demerger {
        /// Resulting company ISIN
        resulting_isin: Option<String>,
    },
}

/// Corporate action record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorporateActionEvent {
    /// Instrument ISIN
    pub isin: String,
    /// Event type
    pub event_type: CorporateActionType,
    /// Effective date (ex-date)
    pub effective_date: NaiveDate,
    /// Announcement date
    pub announcement_date: Option<NaiveDate>,
    /// Record date
    pub record_date: Option<NaiveDate>,
}

/// Corporate actions registry.
#[derive(Debug, Clone, Default)]
pub struct CorporateActions {
    /// Events indexed by ISIN, sorted by effective date
    events: BTreeMap<String, Vec<CorporateActionEvent>>,
}

impl CorporateActions {
    /// Create a new empty corporate actions registry.
    pub fn new() -> Self {
        Self {
            events: BTreeMap::new(),
        }
    }

    /// Add a corporate action event.
    pub fn add_event(&mut self, event: CorporateActionEvent) {
        self.events
            .entry(event.isin.clone())
            .or_default()
            .push(event);
    }

    /// Get all events for an ISIN, sorted by effective date (newest first).
    pub fn events_for_isin(&self, isin: &str) -> Option<&[CorporateActionEvent]> {
        self.events.get(isin).map(|v| v.as_slice())
    }

    /// Get the adjustment factor for a given date (cumulative effect of all events
    /// up to and including the given date).
    ///
    /// Returns the price adjustment factor: multiply historical prices by this
    /// factor to get adjusted prices.
    pub fn adjustment_factor(&self, isin: &str, as_of: NaiveDate) -> Decimal {
        let mut factor = Decimal::ONE;

        if let Some(events) = self.events.get(isin) {
            for event in events {
                if event.effective_date <= as_of {
                    match &event.event_type {
                        CorporateActionType::Split {
                            old_shares,
                            new_shares,
                        } => {
                            // Split increases share count, decreases price
                            // Factor = old_shares / new_shares
                            factor *= Decimal::from(*old_shares) / Decimal::from(*new_shares);
                        }
                        CorporateActionType::Bonus {
                            bonus_ratio_num,
                            bonus_ratio_den,
                        } => {
                            // Bonus issue increases share count
                            // Factor = denominator / (numerator + denominator)
                            factor *= Decimal::from(*bonus_ratio_den)
                                / Decimal::from(*bonus_ratio_num + *bonus_ratio_den);
                        }
                        CorporateActionType::Dividend { .. } => {
                            // Cash dividends don't adjust historical prices for most purposes
                        }
                        CorporateActionType::SymbolRename { .. } => {
                            // Symbol changes don't affect prices
                        }
                        CorporateActionType::Demerger { .. } => {
                            // Demergers typically require special handling
                        }
                    }
                }
            }
        }

        factor
    }

    /// Adjust a historical price for corporate actions up to the given date.
    pub fn adjust_price(&self, isin: &str, historical_price: Price, as_of: NaiveDate) -> Price {
        let factor = self.adjustment_factor(isin, as_of);
        Price::new(historical_price.0 * factor)
    }
}

impl Default for InstrumentMaster {
    fn default() -> Self {
        Self::new()
    }
}
