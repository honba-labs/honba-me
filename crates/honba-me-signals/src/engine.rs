//! The confluence scorer: turns detector firings into [`Signal`]s (ADR 0031 §1, §2, §5).

use std::collections::BTreeMap;

use honba_core::{Bar, InstrumentId, Price};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

use crate::catalog::{self, Params, Ruleset, ReasonGroup, ReasonPolarity, SignalHorizon};
use crate::detector::Firing;
use crate::detectors;
use crate::series::Series;

/// Signal direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// A trading signal with trade setup tagging (ATR-based stop-loss, targets).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Signal {
    pub horizon: SignalHorizon,
    pub direction: SignalDirection,
    pub strength: f64,
    pub reasons: Vec<Reason>,
    pub stop_loss: Option<Price>,
    pub targets: Vec<Price>,
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
    bars: &[Bar],
    instrument: &InstrumentId,
    timeframe: &str,
    ruleset: &Ruleset,
    params: &Params,
) -> Vec<Signal> {
    if bars.is_empty() {
        return Vec::new();
    }
    let series = Series::compute(bars, params);

    let mut by_bar: BTreeMap<usize, Vec<crate::detector::Firing>> = BTreeMap::new();
    for detector in detectors::instances(params) {
        if !ruleset.detectors.contains(&detector.name()) {
            continue;
        }
        for firing in detector.detect(&series) {
            by_bar.entry(firing.bar).or_default().push(firing);
        }
    }

    let mut signals = Vec::new();
    let mut last_buy: Option<usize> = None;
    let mut last_sell: Option<usize> = None;

    for (bar, firings) in by_bar {
        let (bull, bear) = weigh(&firings);
        let direction = if bull > bear {
            SignalDirection::Buy
        } else if bear > bull {
            SignalDirection::Sell
        } else {
            // No net direction: the evidence cancels, so there is nothing to act on.
            continue;
        };

        let strength = bull.max(bear).min(1.0);
        if strength < ruleset.min_strength {
            continue;
        }

        let last = match direction {
            SignalDirection::Buy => last_buy,
            SignalDirection::Sell => last_sell,
            SignalDirection::Hold => None,
        };
        if ruleset.cooldown_bars > 0 {
            if let Some(previous) = last {
                if bar.saturating_sub(previous) <= ruleset.cooldown_bars {
                    continue;
                }
            }
        }

        let close_price = series.close[bar];
        let atr = series.atr[bar];
        let (stop_loss, targets) = compute_trade_setup(direction, close_price, atr);

        let signal = Signal {
            horizon: ruleset.horizon,
            direction,
            strength,
            reasons: build_reasons(&firings),
            stop_loss,
            targets,
        };

        match direction {
            SignalDirection::Buy => last_buy = Some(bar),
            SignalDirection::Sell => last_sell = Some(bar),
            SignalDirection::Hold => {}
        }
        signals.push(signal);
    }

    signals
}

/// Sums the fired weights by polarity.
fn weigh(firings: &[crate::detector::Firing]) -> (f64, f64) {
    let mut bull = 0.0;
    let mut bear = 0.0;
    for firing in firings {
        let code = crate::catalog::reason_code(firing.code).expect("detectors emit catalogued codes");
        match code.polarity {
            ReasonPolarity::Bullish => bull += code.weight,
            ReasonPolarity::Bearish => bear += code.weight,
            ReasonPolarity::Neutral => {}
        }
    }
    (bull, bear)
}

/// Builds reasons from firings.
fn build_reasons(firings: &[crate::detector::Firing]) -> Vec<Reason> {
    let mut reasons = Vec::with_capacity(firings.len());
    for firing in firings {
        let code = crate::catalog::reason_code(firing.code).expect("detectors emit catalogued codes");
        reasons.push(Reason {
            code: code.code.to_string(),
            group: code.group,
            polarity: code.polarity,
            weight: code.weight,
        });
    }
    reasons
}

/// Computes trade setup (stop-loss and targets) using ATR-based method.
///
/// For buy: stop below entry by 2*ATR, targets at +1R and +2R
/// For sell: stop above entry by 2*ATR, targets at -1R and -2R
/// For hold: no stop/targets
fn compute_trade_setup(direction: SignalDirection, entry_price: f64, atr: Option<f64>) -> (Option<Price>, Vec<Price>) {
    let entry_decimal = Decimal::from_str_exact(&format!("{entry_price:.2}")).unwrap_or(Decimal::ZERO);
    let atr_val = atr.unwrap_or(entry_decimal.to_f64().unwrap_or(0.0) * Decimal::from_str_exact("0.02").unwrap_or(Decimal::ZERO).to_f64().unwrap_or(0.0));
    let stop_distance = atr_val * 2.0;
    let stop_distance_decimal = Decimal::from_str_exact(&stop_distance.to_string()).unwrap_or(Decimal::ZERO);

    match direction {
        SignalDirection::Buy => {
            let stop = Price(entry_decimal - stop_distance_decimal);
            let targets = vec![
                Price(entry_decimal + stop_distance_decimal),
                Price(entry_decimal + stop_distance_decimal * Decimal::from_str_exact("2.0").unwrap_or(Decimal::ZERO)),
            ];
            (Some(stop), targets)
        }
        SignalDirection::Sell => {
            let stop = Price(entry_decimal + stop_distance_decimal);
            let targets = vec![
                Price(entry_decimal - stop_distance_decimal),
                Price(entry_decimal - stop_distance_decimal * Decimal::from_str_exact("2.0").unwrap_or(Decimal::ZERO)),
            ];
            (Some(stop), targets)
        }
        SignalDirection::Hold => (None, vec![]),
    }
}
