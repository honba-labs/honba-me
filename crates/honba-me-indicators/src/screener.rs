//! Screener predicate evaluation over a bar series.
//!
//! Simplified implementation using canonical honba-core types.

use std::collections::BTreeMap;
use std::fmt;

use honba_core::Bar;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Indicator, Rsi, Sma};

/// Filter comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterOp {
    /// Greater than.
    Gt,
    /// Greater than or equal.
    Gte,
    /// Less than.
    Lt,
    /// Less than or equal.
    Lte,
    /// Equal.
    Eq,
    /// Not equal.
    Ne,
    /// Between two values (inclusive).
    Between,
    /// Not equal (alias).
    Neq,
    /// In a set of values.
    In,
    /// Not in a set of values.
    NotIn,
    /// String pattern match.
    Like,
    /// Substring match.
    Has,
    /// Crosses above (resolved by caller).
    CrossesAbove,
    /// Crosses below (resolved by caller).
    CrossesBelow,
}

/// Metric reference identifying a bar-derived metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricRef {
    /// Metric key (e.g. "close", "sma20").
    pub key: String,
    /// Optional time period dimension.
    pub period: Option<MetricPeriod>,
}

/// Metric time period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetricPeriod {
    /// Intraday bar period.
    Intraday,
    /// Daily bar period.
    Daily,
    /// Weekly bar period.
    Weekly,
    /// Monthly bar period.
    Monthly,
    /// Trailing twelve months.
    Ttm,
    /// Snapshot (current bar).
    Snapshot,
    /// Current bar period.
    Current,
}

/// A single filter predicate comparing a metric against a value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreenerFilterPredicate {
    /// Metric to evaluate on the left-hand side.
    pub metric: MetricRef,
    /// Comparison operator.
    pub op: FilterOp,
    /// Right-hand side scalar value.
    pub value: f64,
}

/// A group of filter predicates (AND/OR logic).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ScreenerFilterGroup {
    /// AND: all children must match.
    And(Vec<ScreenerFilterGroup>),
    /// OR: any child must match.
    Or(Vec<ScreenerFilterGroup>),
    /// A single predicate leaf.
    Predicate(ScreenerFilterPredicate),
}

/// Timeframe for data aggregation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Timeframe {
    Intraday1m,
    Intraday5m,
    Intraday15m,
    Daily,
    Weekly,
    Monthly,
}

/// Screener evaluation error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ScreenerError {
    /// The requested metric is not supported.
    UnsupportedMetric { key: String },
    /// The operand type is invalid for the operator.
    InvalidOperand { op: FilterOp, message: String },
    /// The filter definition is invalid.
    InvalidFilter { message: String },
}

impl fmt::Display for ScreenerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedMetric { key } => write!(f, "unsupported metric: {key}"),
            Self::InvalidOperand { op, message } => write!(f, "invalid operand for {op:?}: {message}"),
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

/// Bars in the 52-week window; fewer bars than this is insufficient data.
const WEEK52_BARS: usize = 252;

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

fn parse_metric_inner(key: &str) -> Result<Metric, ScreenerError> {
    let k = key.to_ascii_lowercase();
    match k.as_str() {
        "open" => Ok(Metric::Open),
        "high" => Ok(Metric::High),
        "low" => Ok(Metric::Low),
        "close" => Ok(Metric::Close),
        "volume" => Ok(Metric::Volume),
        "price_52_week_low" => Ok(Metric::Week52Low),
        "price_52_week_high" => Ok(Metric::Week52High),
        s if s.starts_with("sma") => {
            let period: usize = s[3..].parse().map_err(|_| ScreenerError::UnsupportedMetric {
                key: key.to_owned(),
            })?;
            Ok(Metric::Sma(period))
        }
        "rsi14" | "rsi" => Ok(Metric::Rsi14),
        _ => Err(ScreenerError::UnsupportedMetric {
            key: key.to_owned(),
        }),
    }
}

fn series(metric: Metric, bars: &[Bar]) -> Vec<Option<f64>> {
    match metric {
        Metric::Open => bars
            .iter()
            .map(|b| Some(b.open.0.to_string().parse::<f64>().unwrap_or(0.0)))
            .collect(),
        Metric::High => bars
            .iter()
            .map(|b| Some(b.high.0.to_string().parse::<f64>().unwrap_or(0.0)))
            .collect(),
        Metric::Low => bars
            .iter()
            .map(|b| Some(b.low.0.to_string().parse::<f64>().unwrap_or(0.0)))
            .collect(),
        Metric::Close => bars
            .iter()
            .map(|b| Some(b.close.0.to_string().parse::<f64>().unwrap_or(0.0)))
            .collect(),
        Metric::Volume => bars
            .iter()
            .map(|b| Some(b.volume.0.to_string().parse::<f64>().unwrap_or(0.0)))
            .collect(),
        Metric::Week52Low => (0..bars.len())
            .map(|i| {
                let start = (i + 1usize).saturating_sub(WEEK52_BARS);
                bars[start..=i]
                    .iter()
                    .map(|b| b.low.0.to_string().parse::<f64>().unwrap_or(0.0))
                    .reduce(f64::min)
            })
            .collect(),
        Metric::Week52High => (0..bars.len())
            .map(|i| {
                let start = (i + 1usize).saturating_sub(WEEK52_BARS);
                bars[start..=i]
                    .iter()
                    .map(|b| b.high.0.to_string().parse::<f64>().unwrap_or(0.0))
                    .reduce(f64::max)
            })
            .collect(),
        Metric::Sma(period) => {
            let mut sma = Sma::new(period);
            bars.iter()
                .map(|b| {
                    sma.update(b.close.0.to_string().parse::<f64>().unwrap_or(0.0))
                })
                .collect()
        }
        Metric::Rsi14 => {
            let mut rsi = Rsi::new(14);
            bars.iter()
                .map(|b| {
                    rsi.update(b.close.0.to_string().parse::<f64>().unwrap_or(0.0))
                })
                .collect()
        }
    }
}

/// Checks that `key` is a metric this evaluator can compute from bars.
pub fn check_metric(key: &str) -> Result<(), ScreenerError> {
    parse_metric(key).map(|_| ())
}

/// The latest value of each metric in `keys` (`None` while it warms up), keyed as asked.
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
        FilterOp::Ne => as_number(rhs) != Some(lhs),
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
        FilterOp::CrossesAbove | FilterOp::CrossesBelow => false,
    })
}

fn check_dimensions(key: &str, period: &Option<MetricPeriod>) -> Result<(), ScreenerError> {
    match period {
        None | Some(MetricPeriod::Snapshot | MetricPeriod::Current) => Ok(()),
        Some(_) => Err(ScreenerError::UnsupportedMetric {
            key: key.to_owned(),
        }),
    }
}

/// Evaluates one predicate against a chronological bar history (latest bar last).
pub fn evaluate_predicate(
    pred: &ScreenerFilterPredicate,
    bars: &[Bar],
) -> Result<bool, ScreenerError> {
    let lhs_metric = parse_metric(&pred.metric.key)?;
    check_dimensions(&pred.metric.key, &pred.metric.period)?;
    if bars.is_empty() {
        return Ok(false);
    }
    if matches!(lhs_metric, Metric::Week52Low | Metric::Week52High)
        && bars.len() < WEEK52_BARS
    {
        return Ok(false);
    }
    let lhs_series = series(lhs_metric, bars);
    let Some(lhs_curr) = lhs_series.last().copied().flatten() else {
        return Ok(false);
    };
    let lhs_prev = lhs_series.len().checked_sub(2).and_then(|i| lhs_series[i]);

    let scalar_val = serde_json::Number::from_f64(pred.value).unwrap_or(serde_json::Number::from(0));
    let scalar = as_number(&serde_json::Value::Number(scalar_val));
    let (rhs_curr, rhs_prev) = (scalar, scalar);

    match pred.op {
        FilterOp::CrossesAbove | FilterOp::CrossesBelow => {
            let (Some(rc), Some(lp), Some(rp)) = (rhs_curr, lhs_prev, rhs_prev) else {
                return Ok(false);
            };
            Ok(if pred.op == FilterOp::CrossesAbove {
                lp <= rp && lhs_curr > rc
            } else {
                lp >= rp && lhs_curr < rc
            })
        }
        _ => compare(lhs_curr, pred.op, &serde_json::Value::Number(
            serde_json::Number::from_f64(pred.value).unwrap_or(serde_json::Number::from(0))
        )),
    }
}

/// Evaluates a group of predicates against a bar history.
pub fn evaluate_group(
    group: &ScreenerFilterGroup,
    bars: &[Bar],
) -> Result<bool, ScreenerError> {
    match group {
        ScreenerFilterGroup::And(groups) => groups
            .iter()
            .map(|g| evaluate_group(g, bars))
            .collect::<Result<Vec<_>, _>>()
            .map(|results| results.into_iter().all(|x| x)),
        ScreenerFilterGroup::Or(groups) => groups
            .iter()
            .map(|g| evaluate_group(g, bars))
            .collect::<Result<Vec<_>, _>>()
            .map(|results| results.into_iter().any(|x| x)),
        ScreenerFilterGroup::Predicate(pred) => evaluate_predicate(pred, bars),
    }
}

/// Parses a metric key string into a Metric.
pub fn parse_metric(key: &str) -> Result<Metric, ScreenerError> {
    parse_metric_inner(key)
}
