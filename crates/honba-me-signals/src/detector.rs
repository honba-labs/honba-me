//! The detector contract: a family that scans a closed series and reports firings.

use crate::series::Series;

/// One detector firing at one bar, before scoring (ADR 0031 §1).
#[derive(Clone, Debug, PartialEq)]
pub struct Firing {
    /// The frozen reason code that fired (must be in `catalog::REASON_CODES`).
    pub code: &'static str,
    /// The bar index the firing is anchored to.
    pub bar: usize,
    /// One sentence citing the numbers, shown to the user.
    pub detail: String,
    /// Numbers the detail cites, folded into the signal's evidence map.
    pub evidence: Vec<(&'static str, f64)>,
}

/// A detector over a closed bar series.
///
/// `detect` is causal: the firing at bar `i` may read only inputs `<= i`, so truncating the
/// series after `i` cannot change it. Implementors are stateless; the shared [`Series`] is
/// computed once per evaluation.
pub(crate) trait Detector {
    /// The catalogue name (must match a `catalog::DETECTORS` entry).
    fn name(&self) -> &'static str;

    /// Every firing over the series, in bar order.
    fn detect(&self, series: &Series) -> Vec<Firing>;
}
