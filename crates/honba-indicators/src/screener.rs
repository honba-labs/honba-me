//! Screener predicate evaluation over a bar series.
//!
//! This is the Rust twin of the Python reference evaluator
//! (`honba.screener.evaluator`: `_compare`, `evaluate_predicate_on_bars`,
//! `evaluate_group_on_bars`). Both are pinned to each other by the shared golden vectors in
//! `schema/conformance/screener_scan.json`.
//!
//! # Placement
//!
//! It lives here, in `honba-indicators` (L3), because it is the lowest layer that can name both
//! the predicate types (`honba-entities`, L1) and the indicators it computes with. Everything
//! above (the REST handler, the WASM surface) reuses it without a new dependency edge, and it
//! stays pure: no I/O, no clock, no allocation beyond the series it computes.
//!
//! # Metrics
//!
//! Only metrics derivable from bars are supported: `open`, `high`, `low`, `close`, `volume`,
//! `price_52_week_high`, `price_52_week_low`, `sma<N>` and `rsi` (period 14), matched
//! case-insensitively. Any other key, or any `period` dimension, is a
//! [`ScreenerError::UnsupportedMetric`]: where Python silently answers `false` for those, this
//! evaluator refuses, because a bar-only dataset must never pass a fundamental off as a
//! non-match.
//!
//! # Known differences from Python (all pinned by the golden vectors)
//!
//! - Unsupported metrics are an error, not `false` (above).
//! - An ordering operator (`gt`, `gte`, `lt`, `lte`, `between`) against a non-numeric operand is
//!   [`ScreenerError::InvalidOperand`]; Python raises `TypeError`.
//! - The 252-bar invariant of the 52-week metrics is applied to the key case-insensitively;
//!   Python compares it case-sensitively, so `PRICE_52_WEEK_LOW` over a short history is computed
//!   from the short window there.

use std::collections::BTreeMap;
use std::fmt;

use honba_entities::{
    FilterOp, MetricPeriod, MetricRef, ScreenerFilterGroup, ScreenerFilterPredicate, Timeframe,
};
use honba_messages::Bar;
use serde_json::Value;

use crate::{Indicator, Rsi, Sma};

/// The deepest group nesting accepted (the top-level group is depth 1).
pub const MAX_GROUP_DEPTH: usize = 8;

/// The most predicates one filter may contain, across all nested groups.
pub const MAX_PREDICATES: usize = 64;

/// Bars in the 52-week window; fewer bars than this is insufficient data.
const WEEK52_BARS: usize = 252;

/// Why a screener filter cannot be evaluated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScreenerError {
    /// The metric (or a period dimension on it) cannot be computed from bars.
    UnsupportedMetric {
        /// The offending metric key.
        key: String,
    },
    /// The right-hand operand does not fit the operator (Python: `TypeError`).
    InvalidOperand {
        /// Operator being applied.
        op: FilterOp,
        /// What is wrong.
        message: String,
    },
    /// The filter tree itself is malformed, or exceeds the size limits.
    InvalidFilter {
        /// What is wrong.
        message: String,
    },
}

impl ScreenerError {
    /// A stable machine-readable reason code.
    pub fn reason(&self) -> &'static str {
        match self {
            Self::UnsupportedMetric { .. } => "unsupported_metric",
            Self::InvalidOperand { .. } => "invalid_operand",
            Self::InvalidFilter { .. } => "invalid_filter",
        }
    }
}

impl fmt::Display for ScreenerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedMetric { key } => write!(
                f,
                "metric {key:?} cannot be computed from bars (supported: open, high, low, close, \
                 volume, price_52_week_high, price_52_week_low, sma<N>, rsi)"
            ),
            Self::InvalidOperand { op, message } => write!(f, "{op:?}: {message}"),
            Self::InvalidFilter { message } => write!(f, "invalid filter: {message}"),
        }
    }
}

impl std::error::Error for ScreenerError {}

fn invalid_filter(message: impl Into<String>) -> ScreenerError {
    ScreenerError::InvalidFilter {
        message: message.into(),
    }
}

/// A bar-derived metric.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Metric {
    Open,
    High,
    Low,
    Close,
    Volume,
    Week52Low,
    Week52High,
    Sma(usize),
    Rsi14,
}

fn parse_metric(key: &str) -> Result<Metric, ScreenerError> {
    let lower = key.to_ascii_lowercase();
    let metric = match lower.as_str() {
        "open" => Some(Metric::Open),
        "high" => Some(Metric::High),
        "low" => Some(Metric::Low),
        "close" => Some(Metric::Close),
        "volume" => Some(Metric::Volume),
        "price_52_week_low" => Some(Metric::Week52Low),
        "price_52_week_high" => Some(Metric::Week52High),
        "rsi" => Some(Metric::Rsi14),
        other => other
            .strip_prefix("sma")
            .filter(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|digits| digits.parse::<usize>().ok())
            .filter(|&period| period > 0)
            .map(Metric::Sma),
    };
    metric.ok_or_else(|| ScreenerError::UnsupportedMetric {
        key: key.to_owned(),
    })
}

fn series(metric: Metric, bars: &[Bar]) -> Vec<Option<f64>> {
    match metric {
        Metric::Open => bars.iter().map(|b| Some(b.open())).collect(),
        Metric::High => bars.iter().map(|b| Some(b.high())).collect(),
        Metric::Low => bars.iter().map(|b| Some(b.low())).collect(),
        Metric::Close => bars.iter().map(|b| Some(b.close())).collect(),
        Metric::Volume => bars.iter().map(|b| Some(b.volume())).collect(),
        Metric::Week52Low => (0..bars.len())
            .map(|i| {
                bars[(i + 1).saturating_sub(WEEK52_BARS)..=i]
                    .iter()
                    .map(Bar::low)
                    .reduce(f64::min)
            })
            .collect(),
        Metric::Week52High => (0..bars.len())
            .map(|i| {
                bars[(i + 1).saturating_sub(WEEK52_BARS)..=i]
                    .iter()
                    .map(Bar::high)
                    .reduce(f64::max)
            })
            .collect(),
        Metric::Sma(period) => {
            let mut sma = Sma::new(period);
            bars.iter().map(|b| sma.update(b.close())).collect()
        }
        Metric::Rsi14 => {
            let mut rsi = Rsi::new(14);
            bars.iter().map(|b| rsi.update(b.close())).collect()
        }
    }
}

/// Checks that `key` is a metric this evaluator can compute from bars.
pub fn check_metric(key: &str) -> Result<(), ScreenerError> {
    parse_metric(key).map(|_| ())
}

/// The latest value of each metric in `keys` (`None` while it warms up), keyed as asked.
///
/// An empty `bars` gives `None` for every supported key; an unsupported key is an error.
pub fn latest_metrics(
    keys: &[String],
    bars: &[Bar],
) -> Result<BTreeMap<String, Option<f64>>, ScreenerError> {
    let mut out = BTreeMap::new();
    for key in keys {
        let metric = parse_metric(key)?;
        out.insert(key.clone(), series(metric, bars).last().copied().flatten());
    }
    Ok(out)
}

/// Python's `str(float)`, so `like`/`has` see the same text as the reference.
pub fn py_float_repr(x: f64) -> String {
    if x.is_nan() {
        return "nan".to_owned();
    }
    if x.is_infinite() {
        return if x < 0.0 { "-inf" } else { "inf" }.to_owned();
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0.0" } else { "0.0" }.to_owned();
    }
    // `{:e}` yields the shortest round-trip digits, e.g. "-1.5e-7".
    let sci = format!("{x:e}");
    let (mantissa, exp) = sci.split_once('e').unwrap_or((sci.as_str(), "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", mantissa),
    };
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let decpt = exp + 1;
    let n = digits.len() as i32;
    let body = if decpt <= -4 || decpt > 16 {
        let mut s = digits[..1].to_owned();
        if n > 1 {
            s.push('.');
            s.push_str(&digits[1..]);
        }
        let e = decpt - 1;
        format!("{s}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs())
    } else if decpt <= 0 {
        format!("0.{}{digits}", "0".repeat((-decpt) as usize))
    } else if decpt >= n {
        format!("{digits}{}.0", "0".repeat((decpt - n) as usize))
    } else {
        format!(
            "{}.{}",
            &digits[..decpt as usize],
            &digits[decpt as usize..]
        )
    };
    format!("{sign}{body}")
}

/// Python's `str(value)` for the scalar operands `like`/`has` accept.
fn py_str(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Bool(true) => Some("True".to_owned()),
        Value::Bool(false) => Some("False".to_owned()),
        Value::Number(n) => Some(match n.as_i64() {
            Some(i) => i.to_string(),
            None => match n.as_u64() {
                Some(u) => u.to_string(),
                None => py_float_repr(n.as_f64()?),
            },
        }),
        _ => None,
    }
}

/// A JSON value as the number Python would compare it as (`True == 1.0`), if it is one.
fn as_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        _ => None,
    }
}

fn operand(op: FilterOp, value: &Value) -> Result<f64, ScreenerError> {
    as_number(value).ok_or_else(|| ScreenerError::InvalidOperand {
        op,
        message: format!("cannot order a number against {value}"),
    })
}

/// `_compare(lhs, op, rhs)` for a numeric `lhs`.
fn compare(lhs: f64, op: FilterOp, rhs: &Value) -> Result<bool, ScreenerError> {
    if rhs.is_null() {
        return Ok(false);
    }
    Ok(match op {
        FilterOp::Eq => as_number(rhs).is_some_and(|r| lhs == r),
        FilterOp::Neq => as_number(rhs) != Some(lhs),
        FilterOp::Gt => lhs > operand(op, rhs)?,
        FilterOp::Gte => lhs >= operand(op, rhs)?,
        FilterOp::Lt => lhs < operand(op, rhs)?,
        FilterOp::Lte => lhs <= operand(op, rhs)?,
        FilterOp::Between => match rhs.as_array().map(Vec::as_slice) {
            Some([low, high]) => operand(op, low)? <= lhs && lhs <= operand(op, high)?,
            _ => false,
        },
        FilterOp::In => rhs
            .as_array()
            .is_some_and(|items| items.iter().filter_map(as_number).any(|r| lhs == r)),
        FilterOp::NotIn => rhs
            .as_array()
            .is_some_and(|items| !items.iter().filter_map(as_number).any(|r| lhs == r)),
        FilterOp::Like | FilterOp::Has => {
            let needle = py_str(rhs).ok_or_else(|| ScreenerError::InvalidOperand {
                op,
                message: "the operand must be a string, number or boolean".to_owned(),
            })?;
            py_float_repr(lhs)
                .to_lowercase()
                .contains(&needle.to_lowercase())
        }
        // Crossings are resolved by the caller, which has the previous bar.
        FilterOp::CrossesAbove | FilterOp::CrossesBelow => false,
    })
}

fn check_dimensions(key: &str, period: &Option<MetricPeriod>) -> Result<(), ScreenerError> {
    // A bar series has no fiscal periods: a TTM/FY/... dimension would be fabricated.
    match period {
        None | Some(MetricPeriod::Snapshot | MetricPeriod::Current) => Ok(()),
        Some(_) => Err(ScreenerError::UnsupportedMetric {
            key: key.to_owned(),
        }),
    }
}

/// The right-hand metric of `pred`, when its operand is a metric reference.
fn right_metric(pred: &ScreenerFilterPredicate) -> Option<MetricRef> {
    pred.metric_ref()
}

/// Evaluates one predicate against a chronological bar history (latest bar last).
pub fn evaluate_predicate(
    pred: &ScreenerFilterPredicate,
    bars: &[Bar],
) -> Result<bool, ScreenerError> {
    let lhs_metric = parse_metric(&pred.key)?;
    check_dimensions(&pred.key, &pred.period)?;
    let rhs_ref = right_metric(pred);
    let rhs_metric = match &rhs_ref {
        Some(r) => {
            check_dimensions(&r.key, &r.period)?;
            Some(parse_metric(&r.key)?)
        }
        None => None,
    };
    if bars.is_empty() {
        return Ok(false);
    }
    if matches!(lhs_metric, Metric::Week52Low | Metric::Week52High) && bars.len() < WEEK52_BARS {
        return Ok(false);
    }
    let lhs_series = series(lhs_metric, bars);
    let Some(lhs_curr) = lhs_series.last().copied().flatten() else {
        return Ok(false);
    };
    let lhs_prev = lhs_series.len().checked_sub(2).and_then(|i| lhs_series[i]);

    // (current, previous) of the right-hand side; a scalar is its own previous value.
    let (rhs_curr, rhs_prev): (Option<f64>, Option<f64>) = if let Some(metric) = rhs_metric {
        let rhs_series = series(metric, bars);
        let Some(curr) = rhs_series.last().copied().flatten() else {
            return Ok(false);
        };
        let prev = rhs_series.len().checked_sub(2).and_then(|i| rhs_series[i]);
        (Some(curr), prev)
    } else {
        let scalar = as_number(&pred.value);
        (scalar, scalar)
    };

    match pred.op {
        FilterOp::CrossesAbove | FilterOp::CrossesBelow => {
            let (Some(rc), Some(lp), Some(rp)) = (rhs_curr, lhs_prev, rhs_prev) else {
                // Python: a missing previous value is false; a non-numeric scalar cannot cross.
                return if pred.value.is_null()
                    || rhs_metric.is_some()
                    || as_number(&pred.value).is_some()
                {
                    Ok(false)
                } else {
                    Err(ScreenerError::InvalidOperand {
                        op: pred.op,
                        message: "the operand must be a number or a metric".to_owned(),
                    })
                };
            };
            Ok(if pred.op == FilterOp::CrossesAbove {
                lp <= rp && lhs_curr > rc
            } else {
                lp >= rp && lhs_curr < rc
            })
        }
        op if rhs_metric.is_some() => match rhs_curr {
            Some(rc) => compare(lhs_curr, op, &number_value(rc)),
            None => Ok(false),
        },
        // A literal operand is compared as given, so `like 42` and `like 42.0` stay distinct.
        op => compare(lhs_curr, op, &pred.value),
    }
}

fn number_value(x: f64) -> Value {
    serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number)
}

/// One item of a group: a nested group (has `operator`) or a predicate.
enum Item {
    Group(ScreenerFilterGroup),
    Predicate(ScreenerFilterPredicate),
}

fn parse_item(raw: &Value) -> Result<Item, ScreenerError> {
    let object = raw
        .as_object()
        .ok_or_else(|| invalid_filter("a filter item must be a predicate or a group object"))?;
    if object.contains_key("operator") {
        serde_json::from_value(raw.clone())
            .map(Item::Group)
            .map_err(|e| invalid_filter(format!("invalid group: {e}")))
    } else {
        serde_json::from_value(raw.clone())
            .map(Item::Predicate)
            .map_err(|e| invalid_filter(format!("invalid predicate: {e}")))
    }
}

fn is_or(group: &ScreenerFilterGroup) -> Result<bool, ScreenerError> {
    match group.operator.as_str() {
        "AND" => Ok(false),
        "OR" => Ok(true),
        other => Err(invalid_filter(format!(
            "group operator {other:?} is not AND or OR"
        ))),
    }
}

/// Walks a filter tree, calling `visit` on every predicate; enforces the size limits.
fn walk(
    group: &ScreenerFilterGroup,
    depth: usize,
    count: &mut usize,
    visit: &mut dyn FnMut(&ScreenerFilterPredicate) -> Result<(), ScreenerError>,
) -> Result<(), ScreenerError> {
    if depth > MAX_GROUP_DEPTH {
        return Err(invalid_filter(format!(
            "groups are nested deeper than {MAX_GROUP_DEPTH}"
        )));
    }
    is_or(group)?;
    for raw in &group.items {
        match parse_item(raw)? {
            Item::Group(inner) => walk(&inner, depth + 1, count, visit)?,
            Item::Predicate(pred) => {
                *count += 1;
                if *count > MAX_PREDICATES {
                    return Err(invalid_filter(format!(
                        "a filter may hold at most {MAX_PREDICATES} predicates"
                    )));
                }
                visit(&pred)?;
            }
        }
    }
    Ok(())
}

/// Checks a filter without any bars: well-formed, within limits, and every metric supported.
pub fn validate_group(group: &ScreenerFilterGroup) -> Result<(), ScreenerError> {
    walk(group, 1, &mut 0, &mut |pred| {
        check_metric(&pred.key)?;
        check_dimensions(&pred.key, &pred.period)?;
        if let Some(r) = right_metric(pred) {
            check_metric(&r.key)?;
            check_dimensions(&r.key, &r.period)?;
        }
        Ok(())
    })
}

/// Every metric key a filter reads (left side, then right-side reference), first-seen order,
/// without repeats.
pub fn group_metric_keys(group: &ScreenerFilterGroup) -> Result<Vec<String>, ScreenerError> {
    let mut keys: Vec<String> = Vec::new();
    walk(group, 1, &mut 0, &mut |pred| {
        let mut add = |key: &str| {
            if !keys.iter().any(|k| k == key) {
                keys.push(key.to_owned());
            }
        };
        add(&pred.key);
        if let Some(r) = right_metric(pred) {
            add(&r.key);
        }
        Ok(())
    })?;
    Ok(keys)
}

/// Every timeframe dimension a filter names (left or right side), first-seen order, no repeats.
///
/// The evaluator reads whatever bars it is given, so a caller that knows the bars' timeframe
/// uses this to refuse a predicate asking for another one rather than answer from the wrong bars.
pub fn group_timeframes(group: &ScreenerFilterGroup) -> Result<Vec<Timeframe>, ScreenerError> {
    let mut found: Vec<Timeframe> = Vec::new();
    walk(group, 1, &mut 0, &mut |pred| {
        let ref_tf = right_metric(pred).and_then(|r| r.timeframe);
        for tf in pred.timeframe.iter().cloned().chain(ref_tf) {
            if !found.contains(&tf) {
                found.push(tf);
            }
        }
        Ok(())
    })?;
    Ok(found)
}

/// Evaluates a group of predicates (and nested groups) against a bar history.
///
/// Every item is evaluated (no short-circuit), so an unsupported metric is reported even when
/// another branch already decides the result. An empty group is `true`.
pub fn evaluate_group(group: &ScreenerFilterGroup, bars: &[Bar]) -> Result<bool, ScreenerError> {
    validate_group(group)?;
    evaluate_validated(group, bars)
}

fn evaluate_validated(group: &ScreenerFilterGroup, bars: &[Bar]) -> Result<bool, ScreenerError> {
    if group.items.is_empty() {
        return Ok(true);
    }
    let or = is_or(group)?;
    let mut results = Vec::with_capacity(group.items.len());
    for raw in &group.items {
        results.push(match parse_item(raw)? {
            Item::Group(inner) => evaluate_validated(&inner, bars)?,
            Item::Predicate(pred) => evaluate_predicate(&pred, bars)?,
        });
    }
    Ok(if or {
        results.iter().any(|&r| r)
    } else {
        results.iter().all(|&r| r)
    })
}
