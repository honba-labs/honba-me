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

use honba_messages::{
    Bar, BarAggregation, BarSpecification, BarType, Exchange, InstrumentId, PriceType, UnixNanos,
};

use crate::Indicator;

/// A placeholder instrument for tests where the instrument does not matter.
fn any_instrument() -> InstrumentId {
    InstrumentId::new("X", Exchange::new("NSE"))
}

/// A bar with the given high, low and close (open = close, volume 1).
fn hlc(high: f64, low: f64, close: f64) -> Bar {
    let bt = BarType::new(
        any_instrument(),
        BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
    );
    let t = UnixNanos::from_u64(1);
    Bar::new(bt, close, high, low, close, 1.0, t, t)
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
