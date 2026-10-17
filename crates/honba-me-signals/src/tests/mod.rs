//! Unit tests for the signal engine, one file per area.

mod catalog;
mod detectors;
mod engine;
mod levels;
mod price_action;

use honba_messages::{
    Bar, BarAggregation, BarSpecification, BarType, Exchange, InstrumentId, PriceType, UnixNanos,
};

/// A placeholder instrument for tests where the instrument does not matter.
pub(crate) fn instrument() -> InstrumentId {
    InstrumentId::new("X", Exchange::new("NSE"))
}

fn spec() -> BarSpecification {
    BarSpecification::new(1, BarAggregation::Minute, PriceType::Last)
}

/// Bars with `open == high == low == close`, at one-minute steps.
pub(crate) fn bars(closes: &[f64], volumes: &[f64]) -> Vec<Bar> {
    let id = instrument();
    let spec = spec();
    closes
        .iter()
        .zip(volumes)
        .enumerate()
        .map(|(i, (&c, &v))| {
            let ts = UnixNanos::from_u64((i as u64 + 1) * 60_000_000_000);
            Bar::new(BarType::new(id.clone(), spec), c, c, c, c, v, ts, ts)
        })
        .collect()
}

/// Bars with explicit OHLC and volume, at one-minute steps.
pub(crate) fn bars_ohlcv(
    open: &[f64],
    high: &[f64],
    low: &[f64],
    close: &[f64],
    volume: &[f64],
) -> Vec<Bar> {
    let id = instrument();
    let spec = spec();
    (0..close.len())
        .map(|i| {
            let ts = UnixNanos::from_u64((i as u64 + 1) * 60_000_000_000);
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

/// Bars with explicit OHLC, volume and a UTC day number per bar: `ts_event` is the day boundary
/// plus `i` minutes, so bars on the same day share a day but stay ordered.
pub(crate) fn bars_at_days(
    open: &[f64],
    high: &[f64],
    low: &[f64],
    close: &[f64],
    volume: &[f64],
    days: &[u64],
) -> Vec<Bar> {
    let id = instrument();
    let spec = spec();
    (0..close.len())
        .map(|i| {
            let ts = UnixNanos::from_u64(days[i] * 86_400_000_000_000 + i as u64 * 60_000_000_000);
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

/// A long, deterministic oscillation with occasional volume spikes: enough variety for every
/// detector to fire several times over the window.
pub(crate) fn wavy() -> Vec<Bar> {
    let mut closes = Vec::new();
    let mut volumes = Vec::new();
    for i in 0..200 {
        let t = i as f64;
        closes.push(100.0 + 15.0 * (t / 6.0).sin() + 5.0 * (t / 2.5).cos());
        volumes.push(if i % 17 == 0 { 5_000.0 } else { 1_000.0 });
    }
    bars(&closes, &volumes)
}
