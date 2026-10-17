//! Level detectors: prior-day high/low and 52-week high/low breaks.
//!
//! The prior-day detector groups bars by UTC calendar day of their `ts_event`, so it works on
//! both intraday bars (a real session boundary) and daily bars (each bar is its own day, so the
//! prior day is the previous bar). The 52-week detector compares the close against the rolling
//! extreme of the prior `range_52w` bars (252 trading days is a common year).

use crate::detector::{Detector, Firing};
use crate::series::Series;

/// Prior-day and 52-week level breaks: `break_prior_day_high`, `break_prior_day_low`,
/// `break_52w_high`, `break_52w_low`.
pub(crate) struct Levels {
    pub range_52w: usize,
}

impl Detector for Levels {
    fn name(&self) -> &'static str {
        "levels"
    }

    fn detect(&self, series: &Series) -> Vec<Firing> {
        let mut out = Vec::new();
        // `detect` sees only the Series, so the day grouping needs the raw bars. The Series keeps
        // the same indices, so we walk it and read the day from the paired bar list.
        let days = &series.days;
        if days.is_empty() {
            return out;
        }

        // --- Prior day ---
        let mut cur_day = days[0];
        let mut cur_high = series.high[0];
        let mut cur_low = series.low[0];
        let mut prev_high: Option<f64> = None;
        let mut prev_low: Option<f64> = None;
        for (i, &day) in days.iter().enumerate() {
            if day != cur_day {
                prev_high = Some(cur_high);
                prev_low = Some(cur_low);
                cur_day = day;
                cur_high = series.high[i];
                cur_low = series.low[i];
            } else {
                cur_high = cur_high.max(series.high[i]);
                cur_low = cur_low.min(series.low[i]);
            }
            if let (Some(ph), Some(pl)) = (prev_high, prev_low) {
                let close = series.close[i];
                let prev_close = series.close[i - 1];
                if close > ph && prev_close <= ph {
                    out.push(Firing {
                        code: "break_prior_day_high",
                        bar: i,
                        detail: format!("Close {close:.2} above prior day high {ph:.2}"),
                        evidence: vec![("prior_day_high", ph), ("close", close)],
                    });
                } else if close < pl && prev_close >= pl {
                    out.push(Firing {
                        code: "break_prior_day_low",
                        bar: i,
                        detail: format!("Close {close:.2} below prior day low {pl:.2}"),
                        evidence: vec![("prior_day_low", pl), ("close", close)],
                    });
                }
            }
        }

        // --- 52-week ---
        // A 52-week high needs a full year of history behind it, so nothing fires until the
        // window is filled (otherwise every early bar would be an "all-time high").
        if self.range_52w == 0 {
            return out;
        }
        for i in self.range_52w..series.len() {
            let start = i - self.range_52w;
            let hi = series.high[start..i]
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max);
            let lo = series.low[start..i]
                .iter()
                .copied()
                .fold(f64::INFINITY, f64::min);
            let close = series.close[i];
            if close > hi {
                out.push(Firing {
                    code: "break_52w_high",
                    bar: i,
                    detail: format!("Close {close:.2} at a 52-week high ({hi:.2})"),
                    evidence: vec![("prior_52w_high", hi), ("close", close)],
                });
            } else if close < lo {
                out.push(Firing {
                    code: "break_52w_low",
                    bar: i,
                    detail: format!("Close {close:.2} at a 52-week low ({lo:.2})"),
                    evidence: vec![("prior_52w_low", lo), ("close", close)],
                });
            }
        }

        // Keep firings in bar order (prior-day then 52-week are each ordered, but interleave).
        out.sort_by_key(|f| f.bar);
        out
    }
}
