//! Runs `schema/conformance/signals_day.json` and `schema/conformance/signals_swing.json`
//! against the signal engine.
//!
//! There is no Python twin for signals yet, so the expected signals were captured once from this
//! engine. The vectors pin behaviour: a change to a detector, a weight, a gate or the indicator
//! windows must update them deliberately. Each case supplies a synthetic series and the signals it
//! must produce, matched **by index** — direction, the codes that must be present (extra codes are
//! allowed), and the count — plus the per-signal invariants (valid, correct ruleset).

use honba_entities::SignalDirection;
use honba_messages::{
    Bar, BarAggregation, BarSpecification, BarType, Exchange, InstrumentId, PriceType, UnixNanos,
};
use honba_signals::{catalog, compute, Params};
use serde_json::Value;

const DAY: &str = include_str!("../../../schema/conformance/signals_day.json");
const SWING: &str = include_str!("../../../schema/conformance/signals_swing.json");

fn floats(v: &Value) -> Vec<f64> {
    v.as_array()
        .expect("array")
        .iter()
        .map(|x| x.as_f64().expect("number"))
        .collect()
}

/// Builds bars from a series at one-second granularity of `step_seconds`. `open` defaults to
/// `close` and the highs/lows to the bar's range, so a case only has to state what matters to it.
fn bars(series: &Value, step_seconds: u64) -> Vec<Bar> {
    let close = floats(&series["close"]);
    let volume = floats(&series["volume"]);
    let open = series
        .get("open")
        .map(floats)
        .unwrap_or_else(|| close.clone());
    let high: Vec<f64> = series
        .get("high")
        .map(floats)
        .unwrap_or_else(|| (0..close.len()).map(|i| open[i].max(close[i])).collect());
    let low: Vec<f64> = series
        .get("low")
        .map(floats)
        .unwrap_or_else(|| (0..close.len()).map(|i| open[i].min(close[i])).collect());

    let id = InstrumentId::new("X", Exchange::new("NSE"));
    let spec = BarSpecification::new(1, BarAggregation::Minute, PriceType::Last);
    (0..close.len())
        .map(|i| {
            let ts = UnixNanos::from_u64((i as u64 + 1) * step_seconds * 1_000_000_000);
            Bar::new(
                BarType::new(id.clone(), spec),
                open[i],
                high[i],
                low[i],
                close[i],
                volume[i],
                ts,
                ts,
            )
        })
        .collect()
}

fn usize_or(value: Option<&Value>, fallback: usize) -> usize {
    value
        .and_then(Value::as_u64)
        .map(|v| v as usize)
        .unwrap_or(fallback)
}

fn f64_or(value: Option<&Value>, fallback: f64) -> f64 {
    value.and_then(Value::as_f64).unwrap_or(fallback)
}

/// Horizon defaults, overridden by the case's `params`.
fn params(horizon: honba_entities::SignalHorizon, case: &Value) -> Params {
    let base = Params::for_horizon(horizon);
    let Some(overrides) = case.get("params") else {
        return base;
    };
    Params {
        ema_fast: usize_or(overrides.get("emaFast"), base.ema_fast),
        ema_slow: usize_or(overrides.get("emaSlow"), base.ema_slow),
        rsi_period: usize_or(overrides.get("rsiPeriod"), base.rsi_period),
        rsi_oversold: f64_or(overrides.get("rsiOversold"), base.rsi_oversold),
        rsi_overbought: f64_or(overrides.get("rsiOverbought"), base.rsi_overbought),
        macd_fast: usize_or(overrides.get("macdFast"), base.macd_fast),
        macd_slow: usize_or(overrides.get("macdSlow"), base.macd_slow),
        macd_signal: usize_or(overrides.get("macdSignal"), base.macd_signal),
        vol_window: usize_or(overrides.get("volWindow"), base.vol_window),
        rel_volume: f64_or(overrides.get("relVolume"), base.rel_volume),
        breakout_window: usize_or(overrides.get("breakoutWindow"), base.breakout_window),
        pivot_span: usize_or(overrides.get("pivotSpan"), base.pivot_span),
        range_52w: usize_or(overrides.get("range52w"), base.range_52w),
    }
}

/// The horizon's shipped ruleset, overridden by the case's `gate`.
fn ruleset(horizon: honba_entities::SignalHorizon, case: &Value) -> catalog::Ruleset {
    let mut base = *catalog::ruleset_for(horizon);
    if let Some(gate) = case.get("gate") {
        if let Some(v) = gate.get("minStrength") {
            base.min_strength = v.as_f64().expect("minStrength number");
        }
        if let Some(v) = gate.get("cooldownBars") {
            base.cooldown_bars = v.as_u64().expect("cooldownBars number") as usize;
        }
    }
    base
}

fn dir(direction: SignalDirection) -> &'static str {
    match direction {
        SignalDirection::Buy => "buy",
        SignalDirection::Sell => "sell",
    }
}

/// The actual signals, one compact JSON object per line, so a mismatch is pasteable.
fn dump(signals: &[honba_entities::Signal]) -> String {
    signals
        .iter()
        .map(|s| {
            let codes: Vec<String> = s
                .reason_codes()
                .iter()
                .map(|c| format!("\"{c}\""))
                .collect();
            format!(
                "  {{\"direction\":\"{}\",\"codes\":[{}]}}",
                dir(s.direction),
                codes.join(",")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn run_file(text: &str, horizon: honba_entities::SignalHorizon) {
    let document: Value = serde_json::from_str(text).expect("vector parses");
    let instrument = InstrumentId::new("X", Exchange::new("NSE"));
    let cases = document["cases"].as_array().expect("cases array");
    assert!(!cases.is_empty());

    for case in cases {
        let name = case["name"].as_str().expect("case name");
        let timeframe = case["timeframe"].as_str().expect("timeframe");
        let bars = bars(
            &case["series"],
            usize_or(case.get("barStepSeconds"), 60) as u64,
        );
        let params = params(horizon, case);
        let ruleset = ruleset(horizon, case);

        let actual = compute(&bars, &instrument, timeframe, &ruleset, &params);
        let expected: Vec<&Value> = case["expect"]["signals"]
            .as_array()
            .map(|a| a.iter().collect())
            .unwrap_or_default();

        let mut problems = Vec::new();
        if expected.len() != actual.len() {
            problems.push(format!(
                "expected {} signals, got {}",
                expected.len(),
                actual.len()
            ));
        }
        for (index, (want, got)) in expected.iter().zip(actual.iter()).enumerate() {
            let want_dir = want["direction"].as_str().expect("direction");
            if want_dir != dir(got.direction) {
                problems.push(format!(
                    "signal {index}: direction {want_dir} != {}",
                    dir(got.direction)
                ));
            }
            if got.validate().is_err() {
                problems.push(format!("signal {index}: not well formed"));
            }
            if got.ruleset != ruleset.id {
                problems.push(format!(
                    "signal {index}: ruleset {} != {}",
                    got.ruleset, ruleset.id
                ));
            }
            let codes = want["codes"].as_array().expect("codes array");
            for code in codes {
                let code = code.as_str().expect("code string");
                if !got.tags.iter().any(|t| t == code) {
                    problems.push(format!(
                        "signal {index}: missing code {code} (got {:?})",
                        got.tags
                    ));
                }
            }
        }

        assert!(
            problems.is_empty(),
            "case `{name}`:\n  {}\nactual:\n{}",
            problems.join("\n  "),
            dump(&actual)
        );
    }
}

#[test]
fn signals_day_matches_the_vector() {
    run_file(DAY, honba_entities::SignalHorizon::Intraday);
}

#[test]
fn signals_swing_matches_the_vector() {
    run_file(SWING, honba_entities::SignalHorizon::Swing);
}

/// Regenerates the committed vectors from the engine (run explicitly):
/// `cargo test -p honba-signals --test signals_conformance -- --ignored regenerate_vectors`.
///
/// There is no Python twin for signals yet, so the expected signals are pinned from this engine:
/// a change to a detector, a weight, a gate or an indicator window must refresh the vectors
/// deliberately.
#[test]
#[ignore = "regenerates schema/conformance/signals_*.json"]
fn regenerate_vectors() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema/conformance");
    let instrument = InstrumentId::new("X", Exchange::new("NSE"));
    for (file, horizon) in [
        ("signals_day.json", honba_entities::SignalHorizon::Intraday),
        ("signals_swing.json", honba_entities::SignalHorizon::Swing),
    ] {
        let path = root.join(file);
        let text = std::fs::read_to_string(&path).expect("read vector");
        let mut document: Value = serde_json::from_str(&text).expect("parse vector");
        for case in document["cases"].as_array_mut().expect("cases array") {
            let step = usize_or(case.get("barStepSeconds"), 60) as u64;
            let bars = bars(&case["series"], step);
            let params = params(horizon, case);
            let ruleset = ruleset(horizon, case);
            let timeframe = case["timeframe"].as_str().expect("timeframe");
            let signals = compute(&bars, &instrument, timeframe, &ruleset, &params);
            let expected: Vec<Value> = signals
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "direction": dir(s.direction),
                        "codes": s.reason_codes(),
                    })
                })
                .collect();
            case["expect"] = serde_json::json!({ "signals": expected });
        }
        let out = serde_json::to_string_pretty(&document).expect("serialize") + "\n";
        std::fs::write(&path, out).expect("write vector");
        println!("regenerated {file}");
    }
}
