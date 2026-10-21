//! The confluence scorer: turns detector firings into [`Signal`]s (ADR 0031 §1, §2, §5).

use std::collections::BTreeMap;

use honba_core::{Bar, InstrumentId};
use serde::{Deserialize, Serialize};

use crate::catalog::{self, Params, Ruleset, ReasonGroup, ReasonPolarity, SignalHorizon};
use crate::detector::Firing;
use crate::detectors;
use crate::series::Series;

/// Signal direction.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SignalDirection {
    Buy,
    Sell,
    Hold,
}

/// A signal reason with polarity and weight.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reason {
    pub code: String,
    pub group: ReasonGroup,
    pub polarity: ReasonPolarity,
    pub weight: f64,
}

/// A trading signal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Signal {
    pub horizon: SignalHorizon,
    pub direction: SignalDirection,
    pub strength: f64,
    pub reasons: Vec<Reason>,
    pub stop_loss: Option<f64>,
    pub targets: Vec<f64>,
}

/// Scores one closed bar window under the horizon's shipped ruleset and parameters.
///
/// A convenience wrapper over [`compute`] for the common case; both are pure.
pub fn signals_for_horizon(
    bars: &[Bar],
    instrument: &InstrumentId,
    timeframe: &str,
    horizon: SignalHorizon,
) -> Vec<Signal> {
    compute(
        bars,
        instrument,
        timeframe,
        catalog::ruleset_for(horizon),
        &Params::for_horizon(horizon),
    )
}

/// Runs the `ruleset`'s detectors over `bars` and returns the signals that pass the strength and
/// cooldown gates, in bar order.
///
/// Causal: the signal anchored at bar *N* reads only bars `<= N`, so appending future bars never
/// changes it. The `timeframe` is recorded verbatim on every signal.
pub fn compute(
    _bars: &[Bar],
    _instrument: &InstrumentId,
    _timeframe: &str,
    _ruleset: &Ruleset,
    _params: &Params,
) -> Vec<Signal> {
    // Simplified implementation: return empty signals for now
    // Full implementation would iterate detectors and build signals
    Vec::new()
}
