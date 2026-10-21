//! Tests for the frozen ontology, the detector specs and the rulesets.

use honba_entities::SignalHorizon;

use crate::catalog::{self, DETECTORS, Params, REASON_CODES, RULESETS};

#[test]
fn reason_codes_are_unique_and_looked_up() {
    let mut seen = std::collections::BTreeSet::new();
    for code in REASON_CODES {
        assert!(seen.insert(code.code), "duplicate code {}", code.code);
        assert!(
            catalog::reason_code(code.code).is_some(),
            "{} not looked up",
            code.code
        );
    }
    assert!(catalog::reason_code("does_not_exist").is_none());
}

#[test]
fn every_code_has_a_sane_weight() {
    for code in REASON_CODES {
        assert!(
            code.weight > 0.0 && code.weight <= 1.0,
            "{} weight {} out of (0, 1]",
            code.code,
            code.weight
        );
    }
}

#[test]
fn every_detector_code_is_a_reason_code_and_vice_versa() {
    let from_detectors: std::collections::BTreeSet<&str> = DETECTORS
        .iter()
        .flat_map(|d| d.codes.iter().copied())
        .collect();
    let from_catalogue: std::collections::BTreeSet<&str> =
        REASON_CODES.iter().map(|c| c.code).collect();
    assert_eq!(
        from_detectors, from_catalogue,
        "a code is emitted but not catalogued, or catalogued but never emitted"
    );
}

#[test]
fn detector_names_are_unique() {
    let mut seen = std::collections::BTreeSet::new();
    for detector in DETECTORS {
        assert!(seen.insert(detector.name), "duplicate {}", detector.name);
    }
}

#[test]
fn rulesets_cover_every_horizon_and_are_named_after_it() {
    for horizon in [SignalHorizon::Intraday, SignalHorizon::Swing] {
        let ruleset = catalog::ruleset_for(horizon);
        assert_eq!(ruleset.horizon, horizon);
        assert_eq!(ruleset.id, horizon.ruleset());
        assert!(
            (0.0..=1.0).contains(&ruleset.min_strength),
            "{} min_strength {}",
            ruleset.id,
            ruleset.min_strength
        );
    }
    assert_eq!(RULESETS.len(), 2);
    assert!(catalog::ruleset("signals/nope:v1").is_none());
}

#[test]
fn rulesets_name_known_detectors() {
    let known: std::collections::BTreeSet<&str> = DETECTORS.iter().map(|d| d.name).collect();
    for ruleset in RULESETS {
        for name in ruleset.detectors {
            assert!(known.contains(name), "{} names unknown {name}", ruleset.id);
        }
    }
}

#[test]
fn intraday_and_swing_use_different_ema_windows() {
    let intraday = Params::for_horizon(SignalHorizon::Intraday);
    let swing = Params::for_horizon(SignalHorizon::Swing);
    assert_ne!(intraday.ema_fast, swing.ema_fast);
    assert!(intraday.ema_fast < intraday.ema_slow);
    assert!(swing.ema_fast < swing.ema_slow);
}
