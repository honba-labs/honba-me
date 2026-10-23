//! Tests for the scoring engine: confluence, gates, causality.

use honba_core::signal::{SignalDirection, SignalHorizon};

use crate::catalog::{self, Params, Ruleset};
use crate::engine::{compute, signals_for_horizon};
use crate::tests::{instrument, wavy};

#[test]
fn empty_bars_produce_no_signals() {
    let id = instrument();
    assert!(signals_for_horizon(&[], &id, "1d", SignalHorizon::Swing).is_empty());
}

#[test]
fn every_signal_is_well_formed() {
    let id = instrument();
    let signals = signals_for_horizon(&wavy(), &id, "5m", SignalHorizon::Intraday);
    assert!(!signals.is_empty(), "the wavy series should fire something");
    for signal in &signals {
        assert_eq!(signal.validate(), Ok(()));
        assert_eq!(signal.ruleset, "signals/day:v1");
        assert_eq!(signal.horizon, SignalHorizon::Intraday);
        assert_eq!(signal.timeframe, "5m");
        assert!(signal.strength >= catalog::ruleset_for(SignalHorizon::Intraday).min_strength);
        assert!(!signal.tags.is_empty());
        // tags are the sorted, de-duplicated reason codes.
        let mut expected: Vec<String> = signal.reasons.iter().map(|r| r.code.clone()).collect();
        expected.sort();
        expected.dedup();
        assert_eq!(signal.tags, expected);
    }
}

#[test]
fn a_signal_never_revises_when_future_bars_are_appended() {
    // ADR 0031 §2 / E4-S4 truncation property: a signal at bar N reads only bars <= N.
    let id = instrument();
    let full_bars = wavy();
    let full = signals_for_horizon(&full_bars, &id, "5m", SignalHorizon::Intraday);

    let cut = 80;
    let prefix_bars = &full_bars[..cut];
    let prefix = signals_for_horizon(prefix_bars, &id, "5m", SignalHorizon::Intraday);

    let last_t = prefix_bars.last().unwrap().ts_event();
    let expected: Vec<_> = full.iter().filter(|s| s.t <= last_t).cloned().collect();
    assert_eq!(expected, prefix, "a truncated run must match the full run");
    assert!(
        !prefix.is_empty(),
        "the prefix should fire at least one signal"
    );
}

#[test]
fn a_higher_strength_gate_drops_weaker_signals() {
    let id = instrument();
    let bars = wavy();
    let loose = Ruleset {
        min_strength: 0.0,
        cooldown_bars: 0,
        ..*catalog::ruleset_for(SignalHorizon::Intraday)
    };
    let strict = Ruleset {
        min_strength: 1.1, // above the maximum possible strength
        ..loose
    };
    let params = Params::for_horizon(SignalHorizon::Intraday);
    let some = compute(&bars, &id, "5m", &loose, &params);
    let none = compute(&bars, &id, "5m", &strict, &params);
    assert!(!some.is_empty());
    assert!(none.is_empty(), "nothing can clear a 1.1 gate");
}

#[test]
fn cooldown_suppresses_the_same_side_in_consecutive_bars() {
    // With a wide cooldown, two same-side signals must be separated by more than the cooldown.
    let id = instrument();
    let bars = wavy();
    let ruleset = Ruleset {
        min_strength: 0.0,
        cooldown_bars: 20,
        ..*catalog::ruleset_for(SignalHorizon::Intraday)
    };
    let params = Params::for_horizon(SignalHorizon::Intraday);
    let signals = compute(&bars, &id, "5m", &ruleset, &params);
    let mut last_buy: Option<honba_messages::UnixNanos> = None;
    for signal in &signals {
        if signal.direction == SignalDirection::Buy {
            if let Some(prev) = last_buy {
                let gap = signal.t.as_u64() - prev.as_u64();
                assert!(gap > 20 * 60_000_000_000, "buy signals only {gap}ns apart");
            }
            last_buy = Some(signal.t);
        }
    }
}

#[test]
fn signals_are_ordered_by_bar() {
    let id = instrument();
    let signals = signals_for_horizon(&wavy(), &id, "5m", SignalHorizon::Intraday);
    let mut previous = 0u64;
    for signal in &signals {
        assert!(signal.t.as_u64() >= previous);
        previous = signal.t.as_u64();
    }
}

#[test]
fn both_horizons_agree_on_the_contract_but_not_the_parameters() {
    let id = instrument();
    let bars = wavy();
    for horizon in [SignalHorizon::Intraday, SignalHorizon::Swing] {
        let signals = signals_for_horizon(&bars, &id, "1d", horizon);
        for signal in &signals {
            assert_eq!(signal.horizon, horizon);
            assert_eq!(signal.ruleset, horizon.ruleset());
        }
    }
}
