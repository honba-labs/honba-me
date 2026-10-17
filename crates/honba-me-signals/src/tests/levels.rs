//! Tests for the level detectors: prior-day and 52-week breaks.

use honba_entities::SignalHorizon;

use crate::catalog::Params;
use crate::detector::Detector;
use crate::detectors::levels::Levels;
use crate::series::Series;
use crate::tests::{bars_at_days, bars_ohlcv};

fn codes(firings: &[crate::detector::Firing]) -> Vec<&'static str> {
    firings.iter().map(|f| f.code).collect()
}

fn params() -> Params {
    Params {
        range_52w: 3,
        ..Params::for_horizon(SignalHorizon::Intraday)
    }
}

#[test]
fn breaks_the_prior_days_high_and_low() {
    // Bars 0-1 are day 0 (high 11.5, low 9.5); bars 2-3 are day 1.
    let days = [0, 0, 1, 1];
    let close = [10.0, 11.0, 12.0, 8.0];
    let high = [10.5, 11.5, 12.5, 8.5];
    let low = [9.5, 10.5, 11.5, 7.5];
    let open = close;
    let volume = vec![1_000.0; 4];
    let series = Series::compute(
        &bars_at_days(&open, &high, &low, &close, &volume, &days),
        &params(),
    );
    let fired = codes(&Levels { range_52w: 3 }.detect(&series));
    // Bar 2 clears day 0's high; bar 3 clears day 0's low.
    assert!(fired.contains(&"break_prior_day_high"), "got {fired:?}");
    assert!(fired.contains(&"break_prior_day_low"), "got {fired:?}");
}

#[test]
fn a_new_day_uses_the_previous_day_not_the_new_one() {
    // The breakout bar (2) opens day 1, so it must compare against day 0, not itself.
    let days = [0, 0, 1];
    let close = [10.0, 11.0, 12.0];
    let high = [10.5, 11.5, 20.0];
    let low = [9.5, 10.5, 11.5];
    let open = close;
    let volume = vec![1_000.0; 3];
    let series = Series::compute(
        &bars_at_days(&open, &high, &low, &close, &volume, &days),
        &params(),
    );
    let firings = Levels { range_52w: 3 }.detect(&series);
    let pdh = firings
        .iter()
        .find(|f| f.bar == 2 && f.code == "break_prior_day_high")
        .expect("prior-day high fires on the new day's first bar");
    let used = pdh
        .evidence
        .iter()
        .find(|(k, _)| *k == "prior_day_high")
        .map(|(_, v)| *v)
        .unwrap();
    // Compares against day 0's high (11.5), not the new day's own 20.0.
    assert_eq!(used, 11.5);
}

#[test]
fn a_prior_day_break_fires_once_per_new_day() {
    // Day 1 has three bars, all above day 0's high (11.5). Only the first bar crosses, so the
    // prior-day-high reason must fire exactly once, not on every bar of the day.
    let days = [0, 0, 1, 1, 1];
    let close = [10.0, 11.0, 12.0, 13.0, 14.0];
    let high = [10.5, 11.5, 12.5, 13.5, 14.5];
    let low = [9.5, 10.5, 11.5, 12.5, 13.5];
    let open = close;
    let volume = vec![1_000.0; 5];
    let series = Series::compute(
        &bars_at_days(&open, &high, &low, &close, &volume, &days),
        &params(),
    );
    let hits = Levels { range_52w: 3 }
        .detect(&series)
        .iter()
        .filter(|f| f.code == "break_prior_day_high")
        .count();
    assert_eq!(hits, 1, "the break fires on the crossing bar only");
}

#[test]
fn breaks_the_rolling_year_window() {
    // Rising closes clear the max high of the prior `range_52w` bars.
    let close = [10.0, 11.0, 12.0, 13.0];
    let high = [10.5, 11.5, 12.5, 13.5];
    let low = [9.5, 10.5, 11.5, 12.5];
    let open = close;
    let volume = vec![1_000.0; 4];
    let series = Series::compute(&bars_ohlcv(&open, &high, &low, &close, &volume), &params());
    let fired = codes(&Levels { range_52w: 3 }.detect(&series));
    assert!(fired.contains(&"break_52w_high"), "got {fired:?}");

    // Falling closes clear the min low.
    let close = [12.0, 11.0, 10.0, 7.0];
    let high = [12.5, 11.5, 10.5, 7.5];
    let low = [11.5, 10.5, 9.5, 6.5];
    let open = close;
    let series = Series::compute(&bars_ohlcv(&open, &high, &low, &close, &volume), &params());
    let fired = codes(&Levels { range_52w: 3 }.detect(&series));
    assert!(fired.contains(&"break_52w_low"), "got {fired:?}");
}

#[test]
fn no_level_fires_before_a_prior_high_exists() {
    // A single bar has no prior day and no prior window: nothing fires.
    let close = [10.0];
    let high = [10.5];
    let low = [9.5];
    let open = close;
    let volume = [1_000.0];
    let series = Series::compute(&bars_ohlcv(&open, &high, &low, &close, &volume), &params());
    assert!(Levels { range_52w: 3 }.detect(&series).is_empty());
}
