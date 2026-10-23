//! Unit tests for this crate, one file per area.

mod atr;
mod bollinger;
mod donchian;
mod ema;
mod macd;
mod render;
mod rsi;
mod screener;
mod sma;
mod stochastic;
mod williams_r;

use honba_core::{Bar, Exchange, InstrumentId, Price, Quantity};
use rust_decimal::Decimal;

use crate::Indicator;

/// A placeholder instrument for tests where the instrument does not matter.
fn any_instrument() -> InstrumentId {
    InstrumentId::new(Exchange::Nse, "X")
}

/// A bar with the given high, low and close (open = close, volume 1).
fn hlc(high: f64, low: f64, close: f64) -> Bar {
    Bar {
        instrument: any_instrument(),
        open: Price::new(Decimal::from_f64_retain(close).unwrap_or_default()),
        high: Price::new(Decimal::from_f64_retain(high).unwrap_or_default()),
        low: Price::new(Decimal::from_f64_retain(low).unwrap_or_default()),
        close: Price::new(Decimal::from_f64_retain(close).unwrap_or_default()),
        volume: Quantity::from_u64(1),
        timestamp: chrono::Utc::now(),
    }
}

/// Feeds every input and returns the outputs in order.
fn feed<I>(ind: &mut I, inputs: &[f64]) -> Vec<Option<I::Output>>
where
    I: for<'a> Indicator<Input<'a> = f64>,
{
    inputs.iter().map(|&x| ind.update(x)).collect()
}

fn assert_close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-9, "{a} != {b}");
}
