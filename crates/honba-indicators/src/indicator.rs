//! The `Indicator` trait shared by every technical indicator.

/// An incremental, single-pass indicator.
///
/// Implementors consume a stream of inputs and produce an output once enough
/// history has accumulated. Before that point, [`Indicator::update`] returns
/// `None`.
///
/// `Input` is a generic associated type so implementations can borrow from
/// the caller — e.g. ATR reads `&Bar` without cloning.
pub trait Indicator {
    /// The type fed to [`Indicator::update`]. May borrow from the caller.
    type Input<'a>;

    /// The type produced once the indicator is primed.
    ///
    /// Must be `Copy` so `update` can return it by value without allocation.
    type Output: Copy;

    /// Feeds one input and returns the indicator's value, if defined.
    fn update(&mut self, input: Self::Input<'_>) -> Option<Self::Output>;

    /// Returns the most recent output without consuming new input.
    ///
    /// Returns `None` before the indicator has produced its first value.
    fn value(&self) -> Option<Self::Output>;

    /// Clears all internal state.
    fn reset(&mut self);

    /// Returns `true` if [`Indicator::value`] would return `Some`.
    fn is_ready(&self) -> bool {
        self.value().is_some()
    }
}
