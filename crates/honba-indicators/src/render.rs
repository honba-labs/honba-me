//! The indicator render contract (ROADMAP E6-S5): how each indicator's computed outputs are
//! presented by a chart.
//!
//! An indicator describes *presentation only*: the pane it is drawn in, one plot per output (a
//! line, a histogram, or a sparse marker that fires only when a condition flips), a theme colour
//! role (never a literal colour) and optional zones. Nothing here is a wire message, and nothing
//! here can place an order: an indicator never emits execution columns, and a signal becomes an
//! order only inside a strategy.
//!
//! [`catalog_names`] is the set of indicators the WASM surface computes; [`presentation`] returns
//! the contract for one of them and [`validate`] checks it against that indicator's declared
//! outputs. The WASM catalog embeds the serialized contract so the chart consumes it without a
//! second source of truth (see `honba-api-wasm::indicators::list_indicators_json`).

use std::collections::BTreeSet;

use serde::Serialize;

/// The indicators the WASM surface computes and therefore declares a render contract for.
pub const CATALOG: [&str; 8] = [
    "sma",
    "ema",
    "rsi",
    "macd",
    "bollinger",
    "atr",
    "williams_r",
    "stochastic",
];

/// The catalog indicator names, in the order the chart should list them.
pub fn catalog_names() -> &'static [&'static str] {
    &CATALOG
}

/// Which charting pane an indicator is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Pane {
    /// Drawn over the price pane, sharing its scale (moving averages, bands).
    Overlay,
    /// Drawn in its own pane below the price, with its own scale (oscillators, volatility).
    Separate,
}

/// How one output series is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlotKind {
    /// A continuous line.
    Line,
    /// One column per bar (e.g. an MACD histogram).
    Histogram,
    /// Sparse markers, emitted only when the condition in [`Plot::signal`] flips.
    Marker,
}

/// A theme colour role, resolved to a palette slot by the renderer.
///
/// Deliberately a role, never an `#rrggbb` literal, so the light theme, the dark theme and the
/// terminal stay consistent. [`ColorRole::token`] returns the wire token.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorRole {
    /// The indicator's main series.
    Primary,
    /// A secondary series (a signal line, a band edge).
    Secondary,
    /// An up/oversold role.
    Bullish,
    /// A down/overbought role.
    Bearish,
    /// A neutral background role.
    Neutral,
    /// A caution role (an overbought zone edge).
    Warning,
    /// A sparse signal marker.
    Signal,
}

impl ColorRole {
    /// The theme token (`"primary"`, `"bearish"`, …) a renderer resolves to a colour.
    pub const fn token(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Secondary => "secondary",
            Self::Bullish => "bullish",
            Self::Bearish => "bearish",
            Self::Neutral => "neutral",
            Self::Warning => "warning",
            Self::Signal => "signal",
        }
    }
}

/// The condition a sparse marker fires on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    /// Every time the output flips sign (`+` to `-`, or `-` to `+`).
    Flip,
}

/// One plotted output.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Plot {
    /// The indicator output this plot draws; must be one of the indicator's declared outputs.
    pub output: String,
    /// How the output is drawn.
    pub kind: PlotKind,
    /// The theme colour role.
    pub color: ColorRole,
    /// A short human label.
    pub label: String,
    /// The flip condition, present exactly for [`PlotKind::Marker`] plots.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<SignalKind>,
}

/// A horizontal band drawn behind the plots (e.g. an oscillator's overbought zone).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Zone {
    /// The band's lower bound.
    pub from: f64,
    /// The band's upper bound; must be greater than `from`.
    pub to: f64,
    /// The theme colour role.
    pub color: ColorRole,
    /// A short human label.
    pub label: String,
}

/// How an indicator is presented by a chart.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Presentation {
    /// The pane the indicator is drawn in.
    pub pane: Pane,
    /// One plot per declared output.
    pub plots: Vec<Plot>,
    /// Optional zones; empty when the indicator declares none.
    pub zones: Vec<Zone>,
}

/// Why a render contract was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum RenderError {
    /// The indicator name is not in the catalog.
    UnknownIndicator(String),
    /// A plot draws an output the indicator never produces.
    UnknownOutput {
        /// The offending output name.
        output: String,
    },
    /// An indicator output has no plot.
    UnplottedOutput {
        /// The output with no plot.
        output: String,
    },
    /// Two plots draw the same output.
    DuplicateOutput {
        /// The duplicated output name.
        output: String,
    },
    /// A [`PlotKind::Marker`] plot has no [`Plot::signal`].
    MarkerNeedsSignal {
        /// The output of the offending plot.
        output: String,
    },
    /// A [`Plot::signal`] was given on a non-marker plot.
    SignalNeedsMarker {
        /// The output of the offending plot.
        output: String,
    },
    /// A zone is empty, non-finite or ordered backwards.
    InvalidZone {
        /// The offending zone's label.
        label: String,
        /// The zone's lower bound.
        from: f64,
        /// The zone's upper bound.
        to: f64,
        /// Why it was refused.
        reason: &'static str,
    },
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownIndicator(n) => write!(f, "unknown indicator `{n}`"),
            Self::UnknownOutput { output } => write!(f, "unknown output `{output}`"),
            Self::UnplottedOutput { output } => write!(f, "output `{output}` has no plot"),
            Self::DuplicateOutput { output } => write!(f, "output `{output}` is plotted twice"),
            Self::MarkerNeedsSignal { output } => {
                write!(f, "marker `{output}` has no signal condition")
            }
            Self::SignalNeedsMarker { output } => {
                write!(f, "output `{output}` has a signal but is not a marker")
            }
            Self::InvalidZone {
                label,
                from,
                to,
                reason,
            } => write!(f, "zone `{label}` [{from}, {to}]: {reason}"),
        }
    }
}

impl std::error::Error for RenderError {}

fn line(output: &str, color: ColorRole, label: &str) -> Plot {
    Plot {
        output: output.into(),
        kind: PlotKind::Line,
        color,
        label: label.into(),
        signal: None,
    }
}

fn histogram(output: &str, color: ColorRole, label: &str) -> Plot {
    Plot {
        output: output.into(),
        kind: PlotKind::Histogram,
        color,
        label: label.into(),
        signal: None,
    }
}

fn zone(from: f64, to: f64, color: ColorRole, label: &str) -> Zone {
    Zone {
        from,
        to,
        color,
        label: label.into(),
    }
}

/// The render contract for catalog indicator `name`.
///
/// # Errors
///
/// [`RenderError::UnknownIndicator`] when `name` is not in [`catalog_names`].
pub fn presentation(name: &str) -> Result<Presentation, RenderError> {
    let (pane, plots, zones) = match name {
        "sma" => (
            Pane::Overlay,
            vec![line("value", ColorRole::Primary, "SMA")],
            vec![],
        ),
        "ema" => (
            Pane::Overlay,
            vec![line("value", ColorRole::Primary, "EMA")],
            vec![],
        ),
        "rsi" => (
            Pane::Separate,
            vec![line("value", ColorRole::Secondary, "RSI")],
            vec![
                zone(0.0, 30.0, ColorRole::Bullish, "Oversold"),
                zone(70.0, 100.0, ColorRole::Bearish, "Overbought"),
            ],
        ),
        "macd" => (
            Pane::Separate,
            vec![
                line("macd", ColorRole::Primary, "MACD"),
                line("signal", ColorRole::Secondary, "Signal"),
                histogram("histogram", ColorRole::Signal, "Histogram"),
            ],
            vec![],
        ),
        "bollinger" => (
            Pane::Overlay,
            vec![
                line("middle", ColorRole::Primary, "Middle"),
                line("upper", ColorRole::Secondary, "Upper"),
                line("lower", ColorRole::Secondary, "Lower"),
            ],
            vec![],
        ),
        "atr" => (
            Pane::Separate,
            vec![line("value", ColorRole::Secondary, "ATR")],
            vec![],
        ),
        "williams_r" => (
            Pane::Separate,
            vec![line("value", ColorRole::Secondary, "Williams %R")],
            vec![
                zone(-100.0, -80.0, ColorRole::Bullish, "Oversold"),
                zone(-20.0, 0.0, ColorRole::Bearish, "Overbought"),
            ],
        ),
        "stochastic" => (
            Pane::Separate,
            vec![
                line("k", ColorRole::Primary, "%K"),
                line("d", ColorRole::Secondary, "%D"),
            ],
            vec![
                zone(0.0, 20.0, ColorRole::Bullish, "Oversold"),
                zone(80.0, 100.0, ColorRole::Bearish, "Overbought"),
            ],
        ),
        other => return Err(RenderError::UnknownIndicator(other.to_string())),
    };
    Ok(Presentation { pane, plots, zones })
}

/// Checks a render contract against the outputs the indicator declares.
///
/// A valid presentation plots every declared output exactly once, references no output the
/// indicator does not produce, carries a signal exactly on marker plots, and draws only finite,
/// ordered zones.
///
/// # Errors
///
/// [`RenderError`] describing the first violation found.
pub fn validate(presentation: &Presentation, outputs: &[&str]) -> Result<(), RenderError> {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for plot in &presentation.plots {
        if !outputs.contains(&plot.output.as_str()) {
            return Err(RenderError::UnknownOutput {
                output: plot.output.clone(),
            });
        }
        if !seen.insert(plot.output.as_str()) {
            return Err(RenderError::DuplicateOutput {
                output: plot.output.clone(),
            });
        }
        match (plot.kind, plot.signal) {
            (PlotKind::Marker, None) => {
                return Err(RenderError::MarkerNeedsSignal {
                    output: plot.output.clone(),
                });
            }
            (kind, Some(_)) if kind != PlotKind::Marker => {
                return Err(RenderError::SignalNeedsMarker {
                    output: plot.output.clone(),
                });
            }
            _ => {}
        }
    }
    for output in outputs {
        if !seen.contains(output) {
            return Err(RenderError::UnplottedOutput {
                output: (*output).to_string(),
            });
        }
    }
    for zone in &presentation.zones {
        if !zone.from.is_finite() || !zone.to.is_finite() || zone.from >= zone.to {
            return Err(RenderError::InvalidZone {
                label: zone.label.clone(),
                from: zone.from,
                to: zone.to,
                reason: "zone must be a finite band with from < to",
            });
        }
    }
    Ok(())
}
