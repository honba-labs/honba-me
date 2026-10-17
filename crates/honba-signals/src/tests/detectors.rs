//! Tests for the individual detectors. Each asserts the semantics on a hand-built series.

use honba_entities::SignalHorizon;
use honba_indicators::Indicator;

use crate::catalog::Params;
use crate::detector::Detector;
use crate::detectors::{momentum, trend, volume};
use crate::series::Series;
use crate::tests::{bars, bars_ohlcv, wavy};

fn codes(firings: &[crate::detector::Firing]) -> Vec<&'static str> {
    firings.iter().map(|f| f.code).collect()
}

#[test]
fn ema_cross_fires_both_ways_on_an_oscillation() {
    let params = Params::for_horizon(SignalHorizon::Swing);
    let series = Series::compute(&wavy(), &params);
    let detector = trend::EmaCross {
        fast: params.ema_fast,
        slow: params.ema_slow,
    };
    let firings = detector.detect(&series);
    let fired = codes(&firings);
    assert!(fired.contains(&"ema_cross_down"), "got {fired:?}");
    assert!(fired.contains(&"ema_cross_up"), "got {fired:?}");
    // Causal: no firing before the second bar (a cross needs two points).
    assert!(firings.iter().all(|f| f.bar >= 1));
}

#[test]
fn rsi_turn_fires_out_of_both_extremes() {
    // Decline hard (oversold), then rally hard (overbought), then fall again.
    let mut closes = Vec::new();
    for i in 0..40 {
        closes.push(100.0 - i as f64);
    }
    for i in 0..40 {
        closes.push(60.0 + i as f64);
    }
    for i in 0..20 {
        closes.push(100.0 - i as f64);
    }
    let volumes = vec![1_000.0; closes.len()];
    let params = Params {
        rsi_period: 5,
        ..Params::for_horizon(SignalHorizon::Intraday)
    };
    let series = Series::compute(&bars(&closes, &volumes), &params);
    let detector = momentum::RsiTurn {
        period: params.rsi_period,
        oversold: params.rsi_oversold,
        overbought: params.rsi_overbought,
    };
    let fired = codes(&detector.detect(&series));
    assert!(fired.contains(&"rsi_oversold_turn"), "got {fired:?}");
    assert!(fired.contains(&"rsi_overbought_turn"), "got {fired:?}");
}

#[test]
fn macd_cross_fires_both_ways_on_an_oscillation() {
    let params = Params::for_horizon(SignalHorizon::Swing);
    let series = Series::compute(&wavy(), &params);
    let fired = codes(&momentum::MacdCross.detect(&series));
    assert!(fired.contains(&"macd_cross_up"), "got {fired:?}");
    assert!(fired.contains(&"macd_cross_down"), "got {fired:?}");
}

#[test]
fn relative_volume_follows_the_bar_direction() {
    // 40 flat bars, then a big up bar, then a big down bar.
    let mut open = vec![100.0; 40];
    let mut close = vec![100.0; 40];
    let mut high = vec![100.0; 40];
    let mut low = vec![100.0; 40];
    let mut volume = vec![1_000.0; 40];
    open.push(100.0);
    close.push(110.0);
    high.push(110.0);
    low.push(100.0);
    volume.push(5_000.0);
    open.push(110.0);
    close.push(100.0);
    high.push(110.0);
    low.push(100.0);
    volume.push(10_000.0);

    let params = Params {
        vol_window: 5,
        rel_volume: 2.0,
        ..Params::for_horizon(SignalHorizon::Intraday)
    };
    let series = Series::compute(&bars_ohlcv(&open, &high, &low, &close, &volume), &params);
    let detector = volume::RelativeVolume {
        window: params.vol_window,
        multiple: params.rel_volume,
    };
    let fired = codes(&detector.detect(&series));
    assert!(fired.contains(&"rel_volume_spike_up"), "got {fired:?}");
    assert!(fired.contains(&"rel_volume_spike_down"), "got {fired:?}");
}

#[test]
fn every_detector_instance_matches_its_catalogue_spec() {
    let params = Params::default();
    let mut names = std::collections::BTreeSet::new();
    for detector in crate::detectors::instances(&params) {
        assert!(
            crate::catalog::DETECTORS
                .iter()
                .any(|d| d.name == detector.name()),
            "{} is not catalogued",
            detector.name()
        );
        assert!(names.insert(detector.name()), "duplicate instance");
    }
}

#[test]
fn detectors_do_not_fire_before_warmup() {
    // A short series cannot prime the slow EMA / RSI / MACD, so nothing fires.
    let volumes = vec![1_000.0; 5];
    let params = Params::for_horizon(SignalHorizon::Swing);
    let series = Series::compute(&bars(&[10.0, 11.0, 12.0, 11.0, 10.0], &volumes), &params);
    assert!(trend::EmaCross {
        fast: params.ema_fast,
        slow: params.ema_slow
    }
    .detect(&series)
    .is_empty());
    assert!(momentum::MacdCross.detect(&series).is_empty());
}

#[test]
fn the_series_matches_the_underlying_indicator() {
    // Guards against the Series wiring drifting from honba-indicators: EMA(fast) at the last
    // bar equals a directly-updated Ema(fast).
    let params = Params::for_horizon(SignalHorizon::Intraday);
    let bars = wavy();
    let series = Series::compute(&bars, &params);
    let mut ema = honba_indicators::Ema::new(params.ema_fast);
    let mut last = None;
    for bar in &bars {
        last = ema.update(bar.close());
    }
    assert_eq!(series.ema_fast[series.len() - 1], last);
}
