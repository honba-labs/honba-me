//! E6-S5 render contract: every catalog indicator declares how its outputs are drawn.

use std::collections::BTreeSet;

use crate::render::{self, ColorRole, Pane, Plot, PlotKind, Presentation, RenderError, Zone};
use crate::{Atr, BollingerBands, Ema, Indicator, Macd, Rsi, Sma, Stochastic, WilliamsR};

/// The declared outputs of each catalog indicator (mirrors the wasm catalog).
fn outputs_of(name: &str) -> Vec<&'static str> {
    match name {
        "sma" | "ema" | "rsi" | "atr" | "williams_r" => vec!["value"],
        "stochastic" => vec!["k", "d"],
        "macd" => vec!["macd", "signal", "histogram"],
        "bollinger" => vec!["middle", "upper", "lower"],
        other => panic!("unknown catalog indicator {other}"),
    }
}

/// One output series per catalog indicator, in the wasm `indicator_series` shape: `NaN` while
/// warming up, one point per input. ATR is fed an h=l=c bar so its true range is zero.
fn series_of(name: &str, inputs: &[f64]) -> Vec<f64> {
    let nan = |o: Option<f64>| o.unwrap_or(f64::NAN);
    match name {
        "sma" => {
            let mut ind = Sma::new(20);
            inputs.iter().map(|&x| nan(ind.update(x))).collect()
        }
        "ema" => {
            let mut ind = Ema::new(20);
            inputs.iter().map(|&x| nan(ind.update(x))).collect()
        }
        "rsi" => {
            let mut ind = Rsi::new(14);
            inputs.iter().map(|&x| nan(ind.update(x))).collect()
        }
        "macd" => {
            let mut ind = Macd::new(12, 26, 9);
            inputs
                .iter()
                .map(|&x| ind.update(x).map_or(f64::NAN, |v| v.macd))
                .collect()
        }
        "bollinger" => {
            let mut ind = BollingerBands::new(20, 2.0);
            inputs
                .iter()
                .map(|&x| ind.update(x).map_or(f64::NAN, |v| v.middle))
                .collect()
        }
        "atr" => {
            let mut ind = Atr::new(14);
            inputs
                .iter()
                .map(|&x| nan(ind.update_hlc(x, x, x)))
                .collect()
        }
        "williams_r" => {
            let mut ind = WilliamsR::new(14);
            inputs
                .iter()
                .map(|&x| nan(ind.update_hlc(x + 1.0, x - 1.0, x)))
                .collect()
        }
        "stochastic" => {
            let mut ind = Stochastic::new(14, 3);
            inputs
                .iter()
                .map(|&x| {
                    ind.update_hlc(x + 1.0, x - 1.0, x)
                        .map_or(f64::NAN, |v| v.k)
                })
                .collect()
        }
        other => panic!("unknown catalog indicator {other}"),
    }
}

#[test]
fn every_catalog_indicator_declares_a_valid_presentation() {
    for &name in render::catalog_names() {
        let presentation = render::presentation(name).unwrap_or_else(|e| panic!("{name}: {e}"));
        render::validate(&presentation, &outputs_of(name))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
    }
}

#[test]
fn presentation_covers_every_output_exactly_once() {
    for &name in render::catalog_names() {
        let presentation = render::presentation(name).unwrap();
        let outputs = outputs_of(name);
        assert_eq!(
            presentation.plots.len(),
            outputs.len(),
            "{name}: one plot per output"
        );
        let plotted: BTreeSet<&str> = presentation
            .plots
            .iter()
            .map(|p| p.output.as_str())
            .collect();
        assert_eq!(plotted, outputs.iter().copied().collect(), "{name}");
    }
}

#[test]
fn output_len_matches_input() {
    let inputs: Vec<f64> = (0..96).map(|i| 100.0 + f64::from(i) * 0.5).collect();
    for &name in render::catalog_names() {
        let series = series_of(name, &inputs);
        assert_eq!(series.len(), inputs.len(), "{name}: output length vs input");
        assert!(
            series.iter().any(|v| !v.is_nan()),
            "{name}: never warmed up over {} inputs",
            inputs.len()
        );
    }
}

#[test]
fn no_look_ahead_prefix_and_full_runs_agree() {
    let inputs: Vec<f64> = (0..96)
        .map(|i| 100.0 + f64::from(i) * 0.5 + (f64::from(i) * 0.3).sin())
        .collect();
    for &name in render::catalog_names() {
        let full = series_of(name, &inputs);
        for cut in [20usize, 40, 73] {
            let prefix = series_of(name, &inputs[..cut]);
            assert_eq!(prefix.len(), cut, "{name}@{cut}: prefix length");
            for i in 0..cut {
                let (a, b) = (prefix[i], full[i]);
                assert!(
                    (a.is_nan() && b.is_nan()) || (a - b).abs() < 1e-12,
                    "{name}@{cut}[{i}]: prefix {a} != full {b} (look-ahead)"
                );
            }
        }
    }
}

#[test]
fn pane_reflects_the_price_scale() {
    for name in ["sma", "ema", "bollinger"] {
        assert_eq!(
            render::presentation(name).unwrap().pane,
            Pane::Overlay,
            "{name}"
        );
    }
    for name in ["rsi", "macd", "atr"] {
        assert_eq!(
            render::presentation(name).unwrap().pane,
            Pane::Separate,
            "{name}"
        );
    }
}

#[test]
fn color_roles_are_theme_tokens_never_hex() {
    for role in [
        ColorRole::Primary,
        ColorRole::Secondary,
        ColorRole::Bullish,
        ColorRole::Bearish,
        ColorRole::Neutral,
        ColorRole::Warning,
        ColorRole::Signal,
    ] {
        let token = role.token();
        assert!(!token.is_empty(), "empty colour role");
        assert!(!token.contains('#'), "colour role {token} is a hex literal");
    }
    // Every role used by the catalog resolves to a token.
    for &name in render::catalog_names() {
        let p = render::presentation(name).unwrap();
        for plot in &p.plots {
            assert!(!plot.color.token().contains('#'), "{name}");
        }
        for zone in &p.zones {
            assert!(!zone.color.token().contains('#'), "{name}");
        }
    }
}

#[test]
fn oscillator_zones_are_optional_and_ordered() {
    let rsi = render::presentation("rsi").unwrap();
    assert!(
        !rsi.zones.is_empty(),
        "rsi declares overbought/oversold zones"
    );
    for zone in &rsi.zones {
        assert!(zone.from < zone.to, "zone {zone:?} must be ordered");
    }
    assert!(render::presentation("sma").unwrap().zones.is_empty());
}

#[test]
fn signals_are_sparse_markers_not_execution_columns() {
    // A marker plot must carry its flip condition; it is presentation, never an order.
    let marker = Presentation {
        pane: Pane::Separate,
        plots: vec![
            Plot {
                output: "value".into(),
                kind: PlotKind::Line,
                color: ColorRole::Primary,
                label: "Line".into(),
                signal: None,
            },
            Plot {
                output: "signal".into(),
                kind: PlotKind::Marker,
                color: ColorRole::Signal,
                label: "Cross".into(),
                signal: Some(render::SignalKind::Flip),
            },
        ],
        zones: vec![],
    };
    render::validate(&marker, &["value", "signal"]).unwrap();
    for plot in &marker.plots {
        let text = format!("{} {}", plot.output, plot.label).to_lowercase();
        for banned in ["order", "position", "qty", "quantity", "side"] {
            assert!(
                !text.contains(banned),
                "render column {text:?} looks executable"
            );
        }
    }
}

#[test]
fn validate_rejects_a_marker_without_a_signal() {
    let bad = Presentation {
        pane: Pane::Overlay,
        plots: vec![Plot {
            output: "value".into(),
            kind: PlotKind::Marker,
            color: ColorRole::Signal,
            label: "M".into(),
            signal: None,
        }],
        zones: vec![],
    };
    assert!(matches!(
        render::validate(&bad, &["value"]),
        Err(RenderError::MarkerNeedsSignal { .. })
    ));
}

#[test]
fn validate_rejects_an_output_that_does_not_exist() {
    let bad = Presentation {
        pane: Pane::Overlay,
        plots: vec![Plot {
            output: "nope".into(),
            kind: PlotKind::Line,
            color: ColorRole::Primary,
            label: "X".into(),
            signal: None,
        }],
        zones: vec![],
    };
    assert!(matches!(
        render::validate(&bad, &["value"]),
        Err(RenderError::UnknownOutput { .. })
    ));
}

#[test]
fn validate_rejects_an_unplotted_output() {
    let bad = Presentation {
        pane: Pane::Overlay,
        plots: vec![Plot {
            output: "value".into(),
            kind: PlotKind::Line,
            color: ColorRole::Primary,
            label: "X".into(),
            signal: None,
        }],
        zones: vec![],
    };
    assert!(matches!(
        render::validate(&bad, &["value", "extra"]),
        Err(RenderError::UnplottedOutput { .. })
    ));
}

#[test]
fn validate_rejects_duplicate_plots_and_bad_zones() {
    let dup = Presentation {
        pane: Pane::Overlay,
        plots: vec![
            Plot {
                output: "value".into(),
                kind: PlotKind::Line,
                color: ColorRole::Primary,
                label: "a".into(),
                signal: None,
            },
            Plot {
                output: "value".into(),
                kind: PlotKind::Line,
                color: ColorRole::Secondary,
                label: "b".into(),
                signal: None,
            },
        ],
        zones: vec![],
    };
    assert!(matches!(
        render::validate(&dup, &["value"]),
        Err(RenderError::DuplicateOutput { .. })
    ));

    let bad_zone = Presentation {
        pane: Pane::Overlay,
        plots: vec![Plot {
            output: "value".into(),
            kind: PlotKind::Line,
            color: ColorRole::Primary,
            label: "X".into(),
            signal: None,
        }],
        zones: vec![Zone {
            from: 70.0,
            to: 30.0,
            color: ColorRole::Warning,
            label: "backwards".into(),
        }],
    };
    assert!(matches!(
        render::validate(&bad_zone, &["value"]),
        Err(RenderError::InvalidZone { .. })
    ));
}

#[test]
fn unknown_indicator_has_no_presentation() {
    assert!(matches!(
        render::presentation("not_an_indicator"),
        Err(RenderError::UnknownIndicator(_))
    ));
}
