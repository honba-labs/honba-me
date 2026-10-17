//! Price-action detectors: candlestick patterns, break of structure and pivots.

use crate::detector::{Detector, Firing};
use crate::series::Series;

/// Candlestick patterns: `bullish_engulfing`, `bearish_engulfing`, `hammer`, `shooting_star`.
pub(crate) struct Candlestick;

impl Detector for Candlestick {
    fn name(&self) -> &'static str {
        "candlestick"
    }

    fn detect(&self, series: &Series) -> Vec<Firing> {
        let mut out = Vec::new();
        for i in 1..series.len() {
            let (po, pc) = (series.open[i - 1], series.close[i - 1]);
            let (o, h, l, c) = (
                series.open[i],
                series.high[i],
                series.low[i],
                series.close[i],
            );
            let body = (c - o).abs();
            let upper = h - o.max(c);
            let lower = o.min(c) - l;

            // Engulfing: the two bodies are opposite colours and today's body contains
            // yesterday's.
            let prev_down = pc < po;
            let prev_up = pc > po;
            let now_up = c > o;
            let now_down = c < o;
            if prev_down && now_up && c >= po && o <= pc {
                out.push(Firing {
                    code: "bullish_engulfing",
                    bar: i,
                    detail: format!("Bullish engulfing on bar {i}"),
                    evidence: vec![("body", body)],
                });
            } else if prev_up && now_down && c <= po && o >= pc {
                out.push(Firing {
                    code: "bearish_engulfing",
                    bar: i,
                    detail: format!("Bearish engulfing on bar {i}"),
                    evidence: vec![("body", body)],
                });
            }

            // Hammer / shooting star: a long single shadow and a small opposite shadow.
            if body > 0.0 {
                if lower >= 2.0 * body && upper <= body {
                    out.push(Firing {
                        code: "hammer",
                        bar: i,
                        detail: format!("Hammer on bar {i}"),
                        evidence: vec![("lower_shadow", lower), ("body", body)],
                    });
                } else if upper >= 2.0 * body && lower <= body {
                    out.push(Firing {
                        code: "shooting_star",
                        bar: i,
                        detail: format!("Shooting star on bar {i}"),
                        evidence: vec![("upper_shadow", upper), ("body", body)],
                    });
                }
            }
        }
        out
    }
}

/// Break of structure: the close clears the highest high (or lowest low) of the prior
/// `window` bars. Fires `break_of_structure_up` / `break_of_structure_down`.
pub(crate) struct BreakOfStructure {
    pub window: usize,
}

impl Detector for BreakOfStructure {
    fn name(&self) -> &'static str {
        "break_of_structure"
    }

    fn detect(&self, series: &Series) -> Vec<Firing> {
        let mut out = Vec::new();
        if self.window == 0 {
            return out;
        }
        for i in self.window..series.len() {
            let start = i - self.window;
            let prior_high = series.high[start..i]
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max);
            let prior_low = series.low[start..i]
                .iter()
                .copied()
                .fold(f64::INFINITY, f64::min);
            let close = series.close[i];
            if close > prior_high {
                out.push(Firing {
                    code: "break_of_structure_up",
                    bar: i,
                    detail: format!(
                        "Close {close:.2} broke the {}-bar high {prior_high:.2}",
                        self.window
                    ),
                    evidence: vec![("prior_high", prior_high), ("close", close)],
                });
            } else if close < prior_low {
                out.push(Firing {
                    code: "break_of_structure_down",
                    bar: i,
                    detail: format!(
                        "Close {close:.2} broke the {}-bar low {prior_low:.2}",
                        self.window
                    ),
                    evidence: vec![("prior_low", prior_low), ("close", close)],
                });
            }
        }
        out
    }
}

/// Fractal pivots: bar `i` is a pivot high (low) when its high (low) is the extreme of
/// `[i-span, i+span]`. Confirmed at bar `i+span`, where the firing is anchored. Fires
/// `pivot_low` (support) / `pivot_high` (resistance).
pub(crate) struct FractalPivot {
    pub span: usize,
}

impl Detector for FractalPivot {
    fn name(&self) -> &'static str {
        "pivot"
    }

    fn detect(&self, series: &Series) -> Vec<Firing> {
        let mut out = Vec::new();
        let k = self.span;
        if k == 0 || series.len() <= 2 * k {
            return out;
        }
        for i in k..series.len() - k {
            let hi = series.high[i];
            let lo = series.low[i];
            let mut is_high = true;
            let mut is_low = true;
            for j in (i - k)..=(i + k) {
                if j == i {
                    continue;
                }
                if series.high[j] >= hi {
                    is_high = false;
                }
                if series.low[j] <= lo {
                    is_low = false;
                }
            }
            let bar = i + k; // the bar at which the pivot becomes knowable
            if is_high {
                out.push(Firing {
                    code: "pivot_high",
                    bar,
                    detail: format!("Pivot high {hi:.2} at bar {i}"),
                    evidence: vec![("pivot_high", hi)],
                });
            } else if is_low {
                out.push(Firing {
                    code: "pivot_low",
                    bar,
                    detail: format!("Pivot low {lo:.2} at bar {i}"),
                    evidence: vec![("pivot_low", lo)],
                });
            }
        }
        out
    }
}
