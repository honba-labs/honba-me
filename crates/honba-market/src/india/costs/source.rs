//! Trait for supplying a [`CostModel`] per date.

use chrono::NaiveDate;

use super::super::error::Result;

use super::model::CostModel;

/// A source of cost models indexed by effective date.
pub trait CostModelSource {
    /// Returns the cost model in force on the given date.
    fn model_for(&self, date: NaiveDate) -> Result<CostModel>;
}
