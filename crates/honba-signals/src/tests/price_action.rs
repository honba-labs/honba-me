//! Tests for the price-action detectors: candlesticks, break of structure and pivots.

use honba_entities::SignalHorizon;

use crate::catalog::Params;
use crate::detector::Detector;
use crate::detectors::price_action::{BreakOfStructure, Candlestick, FractalPivot};
use crate::series::Series;
use crate::tests::bars_ohlcv;

fn codes(firings: &[crate::detector::Firing]) -> Vec<&'static str> {
    firings.iter().map(|f| f.code).collect()
}

fn params() -> Params {
    Params::for_horizon(SignalHorizon::Intraday)
}

#[test]
fn candlestick_fires_engulfing_both_ways() {
    // Bar 0 down (105 -> 100), bar 1 up (99 -> 106): bullish engulfing.
    let open = [105.0, 99.0];
    let high = [105.0, 106.0];
    let low = [100.0, 99.0];
    let close = [100.0, 106.0];
    let volume = [1_000.0, 1_000.0];
    let series = Series::compute(&bars_ohlcv(&open, &high, &low, &close, &volume), &params());
    assert!(codes(&Candlestick.detect(&series)).contains(&"bullish_engulfing"));

    // Bar 0 up (100 -> 105), bar 1 down (106 -> 99): bearish engulfing.
    let open = [100.0, 106.0];
    let high = [105.0, 106.0];
    let low = [100.0, 99.0];
    let close = [105.0, 99.0];
    let series = Series::compute(&bars_ohlcv(&open, &high, &low, &close, &volume), &params());
    assert!(codes(&Candlestick.detect(&series)).contains(&"bearish_engulfing"));
}

#[test]
fn candlestick_fires_hammer_and_shooting_star() {
    // A leading flat bar so the pattern bar is bar 1 (a candle needs a prior bar).
    let volume = [1_000.0, 1_000.0];

    // Hammer: small body at the top, long lower shadow.
    let open = [100.0, 100.0];
    let high = [100.0, 101.2];
    let low = [100.0, 97.0];
    let close = [100.0, 101.0];
    let series = Series::compute(&bars_ohlcv(&open, &high, &low, &close, &volume), &params());
    assert!(codes(&Candlestick.detect(&series)).contains(&"hammer"));

    // Shooting star: small body at the bottom, long upper shadow.
    let open = [100.0, 100.0];
    let high = [100.0, 104.0];
    let low = [100.0, 99.9];
    let close = [100.0, 100.1];
    let series = Series::compute(&bars_ohlcv(&open, &high, &low, &close, &volume), &params());
    assert!(codes(&Candlestick.detect(&series)).contains(&"shooting_star"));
}

#[test]
fn break_of_structure_clears_the_prior_window() {
    // Rises: bar 3 closes above the high of bars 0..2.
    let close = [10.0, 11.0, 12.0, 13.0];
    let high = [10.5, 11.5, 12.5, 13.5];
    let low = [9.5, 10.5, 11.5, 12.5];
    let open = close;
    let volume = vec![1_000.0; 4];
    let series = Series::compute(&bars_ohlcv(&open, &high, &low, &close, &volume), &params());
    let fired = codes(&BreakOfStructure { window: 3 }.detect(&series));
    assert!(fired.contains(&"break_of_structure_up"), "got {fired:?}");

    // Falls: bar 3 closes below the low of bars 0..2.
    let close = [10.0, 11.0, 12.0, 8.0];
    let high = [10.5, 11.5, 12.5, 8.5];
    let low = [9.5, 10.5, 11.5, 7.5];
    let open = close;
    let series = Series::compute(&bars_ohlcv(&open, &high, &low, &close, &volume), &params());
    let fired = codes(&BreakOfStructure { window: 3 }.detect(&series));
    assert!(fired.contains(&"break_of_structure_down"), "got {fired:?}");
}

#[test]
fn pivots_fire_at_the_confirmation_bar() {
    // Peak at index 2 (span 2): pivot high confirmed at bar 4.
    let close = [10.0, 11.0, 12.0, 11.0, 10.0];
    let high = [10.5, 11.5, 12.5, 11.5, 10.5];
    let low = [9.5, 10.5, 11.5, 10.5, 9.5];
    let open = close;
    let volume = vec![1_000.0; 5];
    let series = Series::compute(&bars_ohlcv(&open, &high, &low, &close, &volume), &params());
    let firings = FractalPivot { span: 2 }.detect(&series);
    assert_eq!(codes(&firings), vec!["pivot_high"]);
    assert_eq!(firings[0].bar, 4, "confirmed span bars later");

    // Valley at index 2: pivot low confirmed at bar 4.
    let close = [12.0, 11.0, 10.0, 11.0, 12.0];
    let high = [12.5, 11.5, 10.5, 11.5, 12.5];
    let low = [11.5, 10.5, 9.5, 10.5, 11.5];
    let open = close;
    let series = Series::compute(&bars_ohlcv(&open, &high, &low, &close, &volume), &params());
    let firings = FractalPivot { span: 2 }.detect(&series);
    assert_eq!(codes(&firings), vec!["pivot_low"]);
    assert_eq!(firings[0].bar, 4);
}

#[test]
fn a_pivot_never_uses_future_bars_before_confirmation() {
    // The firing bar is always at least `span` past the pivot bar.
    let params = params();
    let series = Series::compute(&crate::tests::wavy(), &params);
    for firing in (FractalPivot {
        span: params.pivot_span,
    })
    .detect(&series)
    {
        assert!(firing.bar >= params.pivot_span);
    }
}
