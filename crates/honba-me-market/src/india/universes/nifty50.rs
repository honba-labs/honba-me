//! The NIFTY 50 index universe.

use chrono::NaiveDate;

use super::super::error::{IndiaError, Result};

use super::source::{Universe, UniverseSource};

/// Number of constituents in the NIFTY 50.
pub const NIFTY50_SIZE: usize = 50;

/// The NIFTY 50 constituent set at a point in time.
#[derive(Clone, Debug)]
pub struct Nifty50 {
    as_of: NaiveDate,
    symbols: Vec<String>,
}

impl Nifty50 {
    /// Creates a universe from an effective date and 50 symbols.
    pub fn new(as_of: NaiveDate, symbols: Vec<String>) -> Result<Self> {
        if symbols.len() != NIFTY50_SIZE {
            return Err(IndiaError::InvalidDate(format!(
                "NIFTY 50 requires {} constituents, got {}",
                NIFTY50_SIZE,
                symbols.len()
            )));
        }
        Ok(Self { as_of, symbols })
    }

    /// Loads a universe via a [`UniverseSource`].
    pub fn from_source<S: UniverseSource + ?Sized>(as_of: NaiveDate, source: &S) -> Result<Self> {
        Self::new(as_of, source.load(as_of)?)
    }
}

impl Universe for Nifty50 {
    fn as_of(&self) -> NaiveDate {
        self.as_of
    }
    fn symbols(&self) -> &[String] {
        &self.symbols
    }
}
