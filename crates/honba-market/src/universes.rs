//! Generic universe definitions and constituent providers.

use chrono::NaiveDate;

use crate::Result;

/// A point-in-time set of instrument symbols or identifiers.
pub trait Universe: Send + Sync {
    /// Returns the effective snapshot date of this constituent set.
    fn as_of(&self) -> NaiveDate;

    /// Returns the constituent symbols.
    fn symbols(&self) -> &[String];

    /// Returns the number of constituents.
    fn len(&self) -> usize {
        self.symbols().len()
    }

    /// Returns `true` if there are no constituents.
    fn is_empty(&self) -> bool {
        self.symbols().is_empty()
    }

    /// Returns `true` if the symbol is in the universe.
    fn contains(&self, symbol: &str) -> bool {
        self.symbols().iter().any(|s| s == symbol)
    }

    /// Returns the index of the symbol, if present.
    fn position(&self, symbol: &str) -> Option<usize> {
        self.symbols().iter().position(|s| s == symbol)
    }
}

/// A source of constituent lists.
pub trait UniverseSource {
    /// Loads the constituent symbols effective on `as_of`.
    fn load(&self, as_of: NaiveDate) -> Result<Vec<String>>;
}

/// In-memory static universe.
#[derive(Clone, Debug)]
pub struct StaticUniverse {
    as_of: NaiveDate,
    symbols: Vec<String>,
}

impl StaticUniverse {
    /// Creates a static universe snapshot.
    pub fn new(as_of: NaiveDate, symbols: Vec<String>) -> Self {
        Self { as_of, symbols }
    }
}

impl Universe for StaticUniverse {
    fn as_of(&self) -> NaiveDate {
        self.as_of
    }

    fn symbols(&self) -> &[String] {
        &self.symbols
    }
}
