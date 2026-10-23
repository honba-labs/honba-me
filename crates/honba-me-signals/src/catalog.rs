//! The frozen reason ontology, the detector families, the horizon rulesets and the detector
//! parameters (ADR 0031 §4, §5).
//!
//! Weights and labels live here, in one auditable place: a detector decides only *when* a
//! [`ReasonCode`] fires and cites its numbers; the engine reads the group, polarity and weight
//! from this table. Adding a code is an additive change; renaming or reusing one is not.

use serde::{Deserialize, Serialize};

/// Reason group classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReasonGroup {
    Trend,
    Momentum,
    Volatility,
    Volume,
    MeanReversion,
    PriceAction,
    Level,
}

/// Reason polarity (bullish/bearish/neutral).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReasonPolarity {
    Bullish,
    Bearish,
    Neutral,
}

/// Signal horizon (timeframe classification).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalHorizon {
    Intraday,
    Swing,
    Positional,
}

/// A frozen reason code: the stable identity of one detector firing (ADR 0031 §4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReasonCode {
    /// The stable wire code, e.g. `ema_cross_up`.
    pub code: &'static str,
    /// The detector family.
    pub group: ReasonGroup,
    /// Which way this reason argues.
    pub polarity: ReasonPolarity,
    /// The positive term this code contributes to a signal's `strength`.
    pub weight: f64,
    /// Short human label shown in the reasons panel.
    pub label: &'static str,
}

/// Every reason code the engine can emit, in catalogue order.
pub const REASON_CODES: &[ReasonCode] = &[
    // --- Trend ---
    ReasonCode {
        code: "ema_cross_up",
        group: ReasonGroup::Trend,
        polarity: ReasonPolarity::Bullish,
        weight: 0.45,
        label: "Fast EMA crossed above slow EMA",
    },
    ReasonCode {
        code: "ema_cross_down",
        group: ReasonGroup::Trend,
        polarity: ReasonPolarity::Bearish,
        weight: 0.45,
        label: "Fast EMA crossed below slow EMA",
    },
    // --- Momentum ---
    ReasonCode {
        code: "rsi_oversold_turn",
        group: ReasonGroup::Momentum,
        polarity: ReasonPolarity::Bullish,
        weight: 0.35,
        label: "RSI turned up out of oversold",
    },
    ReasonCode {
        code: "rsi_overbought_turn",
        group: ReasonGroup::Momentum,
        polarity: ReasonPolarity::Bearish,
        weight: 0.35,
        label: "RSI turned down out of overbought",
    },
    ReasonCode {
        code: "macd_cross_up",
        group: ReasonGroup::Momentum,
        polarity: ReasonPolarity::Bullish,
        weight: 0.35,
        label: "MACD crossed above its signal line",
    },
    ReasonCode {
        code: "macd_cross_down",
        group: ReasonGroup::Momentum,
        polarity: ReasonPolarity::Bearish,
        weight: 0.35,
        label: "MACD crossed below its signal line",
    },
    // --- Volume ---
    ReasonCode {
        code: "rel_volume_spike_up",
        group: ReasonGroup::Volume,
        polarity: ReasonPolarity::Bullish,
        weight: 0.25,
        label: "Relative-volume spike on an up bar",
    },
    ReasonCode {
        code: "rel_volume_spike_down",
        group: ReasonGroup::Volume,
        polarity: ReasonPolarity::Bearish,
        weight: 0.25,
        label: "Relative-volume spike on a down bar",
    },
    // --- Price action ---
    ReasonCode {
        code: "bullish_engulfing",
        group: ReasonGroup::PriceAction,
        polarity: ReasonPolarity::Bullish,
        weight: 0.3,
        label: "Bullish engulfing",
    },
    ReasonCode {
        code: "bearish_engulfing",
        group: ReasonGroup::PriceAction,
        polarity: ReasonPolarity::Bearish,
        weight: 0.3,
        label: "Bearish engulfing",
    },
    ReasonCode {
        code: "hammer",
        group: ReasonGroup::PriceAction,
        polarity: ReasonPolarity::Bullish,
        weight: 0.2,
        label: "Hammer",
    },
    ReasonCode {
        code: "shooting_star",
        group: ReasonGroup::PriceAction,
        polarity: ReasonPolarity::Bearish,
        weight: 0.2,
        label: "Shooting star",
    },
    ReasonCode {
        code: "break_of_structure_up",
        group: ReasonGroup::PriceAction,
        polarity: ReasonPolarity::Bullish,
        weight: 0.45,
        label: "Close breaks the prior swing high",
    },
    ReasonCode {
        code: "break_of_structure_down",
        group: ReasonGroup::PriceAction,
        polarity: ReasonPolarity::Bearish,
        weight: 0.45,
        label: "Close breaks the prior swing low",
    },
    ReasonCode {
        code: "pivot_low",
        group: ReasonGroup::PriceAction,
        polarity: ReasonPolarity::Bullish,
        weight: 0.2,
        label: "Confirmed pivot low (support)",
    },
    ReasonCode {
        code: "pivot_high",
        group: ReasonGroup::PriceAction,
        polarity: ReasonPolarity::Bearish,
        weight: 0.2,
        label: "Confirmed pivot high (resistance)",
    },
    // --- Levels ---
    ReasonCode {
        code: "break_prior_day_high",
        group: ReasonGroup::Level,
        polarity: ReasonPolarity::Bullish,
        weight: 0.35,
        label: "Close above the prior day's high",
    },
    ReasonCode {
        code: "break_prior_day_low",
        group: ReasonGroup::Level,
        polarity: ReasonPolarity::Bearish,
        weight: 0.35,
        label: "Close below the prior day's low",
    },
    ReasonCode {
        code: "break_52w_high",
        group: ReasonGroup::Level,
        polarity: ReasonPolarity::Bullish,
        weight: 0.4,
        label: "Close at a 52-week high",
    },
    ReasonCode {
        code: "break_52w_low",
        group: ReasonGroup::Level,
        polarity: ReasonPolarity::Bearish,
        weight: 0.4,
        label: "Close at a 52-week low",
    },
    // --- VWAP Cross ---
    ReasonCode {
        code: "vwap_cross_up",
        group: ReasonGroup::PriceAction,
        polarity: ReasonPolarity::Bullish,
        weight: 0.35,
        label: "Price crossed above session VWAP",
    },
    ReasonCode {
        code: "vwap_cross_down",
        group: ReasonGroup::PriceAction,
        polarity: ReasonPolarity::Bearish,
        weight: 0.35,
        label: "Price crossed below session VWAP",
    },
    // --- CPR Breakout ---
    ReasonCode {
        code: "cpr_breakout_up",
        group: ReasonGroup::Level,
        polarity: ReasonPolarity::Bullish,
        weight: 0.4,
        label: "Price broke above CPR top (TC)",
    },
    ReasonCode {
        code: "cpr_breakdown_down",
        group: ReasonGroup::Level,
        polarity: ReasonPolarity::Bearish,
        weight: 0.4,
        label: "Price broke below CPR bottom (BC)",
    },
    // --- ORB Breakout ---
    ReasonCode {
        code: "orb_breakout_up",
        group: ReasonGroup::Level,
        polarity: ReasonPolarity::Bullish,
        weight: 0.4,
        label: "Price broke above opening range high",
    },
    ReasonCode {
        code: "orb_breakdown_down",
        group: ReasonGroup::Level,
        polarity: ReasonPolarity::Bearish,
        weight: 0.4,
        label: "Price broke below opening range low",
    },
    // --- Volume Surge ---
    ReasonCode {
        code: "volume_surge_up",
        group: ReasonGroup::Volume,
        polarity: ReasonPolarity::Bullish,
        weight: 0.3,
        label: "Volume surge on up move",
    },
    ReasonCode {
        code: "volume_surge_down",
        group: ReasonGroup::Volume,
        polarity: ReasonPolarity::Bearish,
        weight: 0.3,
        label: "Volume surge on down move",
    },
];

/// Looks up a reason code by its wire string.
pub fn reason_code(code: &str) -> Option<&'static ReasonCode> {
    REASON_CODES.iter().find(|c| c.code == code)
}

/// A detector family and the codes it can emit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DetectorSpec {
    /// Catalogue name, e.g. `ema_cross`.
    pub name: &'static str,
    /// The codes this detector may fire.
    pub codes: &'static [&'static str],
}

/// Every detector the engine knows, in catalogue order.
pub const DETECTORS: &[DetectorSpec] = &[
    DetectorSpec {
        name: "ema_cross",
        codes: &["ema_cross_up", "ema_cross_down"],
    },
    DetectorSpec {
        name: "rsi_turn",
        codes: &["rsi_oversold_turn", "rsi_overbought_turn"],
    },
    DetectorSpec {
        name: "macd_cross",
        codes: &["macd_cross_up", "macd_cross_down"],
    },
    DetectorSpec {
        name: "relative_volume",
        codes: &["rel_volume_spike_up", "rel_volume_spike_down"],
    },
    DetectorSpec {
        name: "candlestick",
        codes: &[
            "bullish_engulfing",
            "bearish_engulfing",
            "hammer",
            "shooting_star",
        ],
    },
    DetectorSpec {
        name: "break_of_structure",
        codes: &["break_of_structure_up", "break_of_structure_down"],
    },
    DetectorSpec {
        name: "pivot",
        codes: &["pivot_low", "pivot_high"],
    },
    DetectorSpec {
        name: "levels",
        codes: &[
            "break_prior_day_high",
            "break_prior_day_low",
            "break_52w_high",
            "break_52w_low",
        ],
    },
    DetectorSpec {
        name: "vwap_cross",
        codes: &["vwap_cross_up", "vwap_cross_down"],
    },
    DetectorSpec {
        name: "cpr_breakout",
        codes: &["cpr_breakout_up", "cpr_breakdown_down"],
    },
    DetectorSpec {
        name: "orb_breakout",
        codes: &["orb_breakout_up", "orb_breakdown_down"],
    },
    DetectorSpec {
        name: "volume_surge",
        codes: &["volume_surge_up", "volume_surge_down"],
    },
];

/// A horizon ruleset: which detectors run, and the confluence/cooldown gates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ruleset {
    /// Stated on every signal this ruleset emits, e.g. `signals/day:v1`.
    pub id: &'static str,
    /// The horizon.
    pub horizon: SignalHorizon,
    /// Drop a signal whose winning-side weight is below this (`0..=1`).
    pub min_strength: f64,
    /// Suppress the same side for this many bars after it fires (`0` disables).
    pub cooldown_bars: usize,
    /// The detector names enabled (each must match a [`DETECTORS`] entry).
    pub detectors: &'static [&'static str],
}

/// The shipped rulesets, one per horizon (ADR 0031 §5).
pub const RULESETS: &[Ruleset] = &[
    Ruleset {
        id: "signals/day:v1",
        horizon: SignalHorizon::Intraday,
        min_strength: 0.4,
        cooldown_bars: 3,
        detectors: &[
            "ema_cross",
            "rsi_turn",
            "macd_cross",
            "relative_volume",
            "candlestick",
            "break_of_structure",
            "pivot",
            "levels",
        ],
    },
    Ruleset {
        id: "signals/swing:v1",
        horizon: SignalHorizon::Swing,
        min_strength: 0.4,
        cooldown_bars: 0,
        detectors: &[
            "ema_cross",
            "rsi_turn",
            "macd_cross",
            "relative_volume",
            "candlestick",
            "break_of_structure",
            "pivot",
            "levels",
        ],
    },
];

/// Looks up a ruleset by id.
pub fn ruleset(id: &str) -> Option<&'static Ruleset> {
    RULESETS.iter().find(|r| r.id == id)
}

/// The shipped ruleset for a horizon.
pub fn ruleset_for(horizon: SignalHorizon) -> &'static Ruleset {
    RULESETS
        .iter()
        .find(|r| r.horizon == horizon)
        .expect("every horizon has a ruleset")
}

impl SignalHorizon {
    pub fn ruleset(self) -> &'static Ruleset {
        ruleset_for(self)
    }
}

/// Detector parameters: the indicator windows and thresholds one evaluation uses.
///
/// These differ by horizon ([`Params::for_horizon`]) but the machinery does not: the same
/// detectors run with different numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    /// Fast EMA period for the trend detector.
    pub ema_fast: usize,
    /// Slow EMA period for the trend detector.
    pub ema_slow: usize,
    /// RSI period.
    pub rsi_period: usize,
    /// RSI level the market must leave for an oversold turn.
    pub rsi_oversold: f64,
    /// RSI level the market must leave for an overbought turn.
    pub rsi_overbought: f64,
    /// MACD fast EMA period.
    pub macd_fast: usize,
    /// MACD slow EMA period.
    pub macd_slow: usize,
    /// MACD signal EMA period.
    pub macd_signal: usize,
    /// Volume moving-average window.
    pub vol_window: usize,
    /// Multiple of the average volume that counts as a spike.
    pub rel_volume: f64,
    /// Bars in the swing high/low a break of structure must clear.
    pub breakout_window: usize,
    /// Half-width of the fractal used to confirm a pivot (bar `i` is a pivot when it is the
    /// extreme of `[i-span, i+span]`).
    pub pivot_span: usize,
    /// Bars in the rolling high/low a 52-week break is measured against.
    pub range_52w: usize,
    /// Number of bars defining the opening range for ORB detector.
    pub orb_minutes: usize,
}

impl Params {
    /// The default parameters for a horizon.
    pub fn for_horizon(horizon: SignalHorizon) -> Self {
        match horizon {
            SignalHorizon::Intraday => Self {
                ema_fast: 9,
                ema_slow: 21,
                rsi_period: 14,
                rsi_oversold: 30.0,
                rsi_overbought: 70.0,
                macd_fast: 12,
                macd_slow: 26,
                macd_signal: 9,
                vol_window: 20,
                rel_volume: 1.5,
                breakout_window: 20,
                pivot_span: 2,
                range_52w: 252,
                orb_minutes: 15,
            },
            SignalHorizon::Swing => Self {
                ema_fast: 20,
                ema_slow: 50,
                rsi_period: 14,
                rsi_oversold: 30.0,
                rsi_overbought: 70.0,
                macd_fast: 12,
                macd_slow: 26,
                macd_signal: 9,
                vol_window: 20,
                rel_volume: 1.2,
                breakout_window: 55,
                pivot_span: 2,
                range_52w: 252,
                orb_minutes: 30,
            },
            SignalHorizon::Positional => Self {
                ema_fast: 50,
                ema_slow: 200,
                rsi_period: 14,
                rsi_oversold: 30.0,
                rsi_overbought: 70.0,
                macd_fast: 12,
                macd_slow: 26,
                macd_signal: 9,
                vol_window: 20,
                rel_volume: 1.0,
                breakout_window: 200,
                pivot_span: 2,
                range_52w: 252,
                orb_minutes: 60,
            },
        }
    }
}

impl Default for Params {
    fn default() -> Self {
        Self::for_horizon(SignalHorizon::Swing)
    }
}
