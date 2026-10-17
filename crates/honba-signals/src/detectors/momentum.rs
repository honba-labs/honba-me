//! Momentum detectors.

use crate::detector::{Detector, Firing};
use crate::series::Series;

/// Fires when RSI leaves an extreme: `rsi_oversold_turn` / `rsi_overbought_turn`.
pub(crate) struct RsiTurn {
    pub period: usize,
    pub oversold: f64,
    pub overbought: f64,
}

impl Detector for RsiTurn {
    fn name(&self) -> &'static str {
        "rsi_turn"
    }

    fn detect(&self, series: &Series) -> Vec<Firing> {
        let mut out = Vec::new();
        for i in 1..series.len() {
            let (Some(prev), Some(now)) = (series.rsi[i - 1], series.rsi[i]) else {
                continue;
            };
            if prev <= self.oversold && now > self.oversold {
                out.push(Firing {
                    code: "rsi_oversold_turn",
                    bar: i,
                    detail: format!(
                        "RSI{} turned up out of oversold ({:.1} -> {:.1})",
                        self.period, prev, now
                    ),
                    evidence: vec![("rsi", now)],
                });
            } else if prev >= self.overbought && now < self.overbought {
                out.push(Firing {
                    code: "rsi_overbought_turn",
                    bar: i,
                    detail: format!(
                        "RSI{} turned down out of overbought ({:.1} -> {:.1})",
                        self.period, prev, now
                    ),
                    evidence: vec![("rsi", now)],
                });
            }
        }
        out
    }
}

/// Fires when the MACD line crosses its signal line: `macd_cross_up` / `macd_cross_down`.
pub(crate) struct MacdCross;

impl Detector for MacdCross {
    fn name(&self) -> &'static str {
        "macd_cross"
    }

    fn detect(&self, series: &Series) -> Vec<Firing> {
        let mut out = Vec::new();
        for i in 1..series.len() {
            let (Some(prev), Some(now)) = (series.macd[i - 1], series.macd[i]) else {
                continue;
            };
            let prev = prev.macd - prev.signal;
            let now = now.macd - now.signal;
            if prev <= 0.0 && now > 0.0 {
                out.push(Firing {
                    code: "macd_cross_up",
                    bar: i,
                    detail: format!("MACD histogram turned positive ({:.3} -> {:.3})", prev, now),
                    evidence: vec![("macd_histogram", now)],
                });
            } else if prev >= 0.0 && now < 0.0 {
                out.push(Firing {
                    code: "macd_cross_down",
                    bar: i,
                    detail: format!("MACD histogram turned negative ({:.3} -> {:.3})", prev, now),
                    evidence: vec![("macd_histogram", now)],
                });
            }
        }
        out
    }
}
