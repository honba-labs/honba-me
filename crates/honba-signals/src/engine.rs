//! The confluence scorer: turns detector firings into [`Signal`]s (ADR 0031 §1, §2, §5).

use std::collections::BTreeMap;

use honba_entities::{Reason, ReasonPolarity, Signal, SignalDirection, SignalHorizon};
use honba_messages::{Bar, InstrumentId};

use crate::catalog::{self, Params, Ruleset};
use crate::detector::Firing;
use crate::detectors;
use crate::series::Series;

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

    let mut by_bar: BTreeMap<usize, Vec<Firing>> = BTreeMap::new();
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
        };
        if ruleset.cooldown_bars > 0 {
            if let Some(previous) = last {
                if bar.saturating_sub(previous) <= ruleset.cooldown_bars {
                    continue;
                }
            }
        }

        let (reasons, tags, evidence) = assemble(&firings);
        let signal = Signal {
            instrument: instrument.clone(),
            timeframe: timeframe.to_string(),
            horizon: ruleset.horizon,
            t: bars[bar].ts_event(),
            direction,
            strength,
            anchor_price: bars[bar].close(),
            ruleset: ruleset.id.to_string(),
            reasons,
            tags,
            evidence,
        };
        signal.validate().expect("the engine builds valid signals");
        match direction {
            SignalDirection::Buy => last_buy = Some(bar),
            SignalDirection::Sell => last_sell = Some(bar),
        }
        signals.push(signal);
    }

    signals
}

/// Sums the fired weights by polarity.
fn weigh(firings: &[Firing]) -> (f64, f64) {
    let mut bull = 0.0;
    let mut bear = 0.0;
    for firing in firings {
        let code = catalog::reason_code(firing.code).expect("detectors emit catalogued codes");
        match code.polarity {
            ReasonPolarity::Bullish => bull += code.weight,
            ReasonPolarity::Bearish => bear += code.weight,
        }
    }
    (bull, bear)
}

/// Builds the reasons, the code tags and the evidence map from the firings at one bar.
fn assemble(firings: &[Firing]) -> (Vec<Reason>, Vec<String>, BTreeMap<String, f64>) {
    let mut reasons = Vec::with_capacity(firings.len());
    let mut evidence = BTreeMap::new();
    for firing in firings {
        let code = catalog::reason_code(firing.code).expect("detectors emit catalogued codes");
        reasons.push(Reason {
            code: code.code.to_string(),
            group: code.group,
            polarity: code.polarity,
            weight: code.weight,
            label: code.label.to_string(),
            detail: firing.detail.clone(),
        });
        for (key, value) in &firing.evidence {
            evidence.insert((*key).to_string(), *value);
        }
    }
    let mut tags: Vec<String> = reasons.iter().map(|r| r.code.clone()).collect();
    tags.sort();
    tags.dedup();
    (reasons, tags, evidence)
}
