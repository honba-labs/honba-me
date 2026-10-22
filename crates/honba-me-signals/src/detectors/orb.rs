//! Opening Range Breakout (ORB) detector.
//!
//! Fires when price breaks above the opening range high (orb_breakout_up) or
//! below the opening range low (orb_breakdown_down). The opening range is
//! defined as the first N bars (default 15 minutes) of each trading day.
//!
//! The detector tracks each day separately: when the day changes, a new opening
//! range window begins.

use crate::detector::{Detector, Firing};
use crate::series::Series;

/// Opening Range Breakout detector.
///
/// The opening range is the first `orb_minutes` bars of each trading day.
/// Once the range is established, any break above the range high or below the
/// range low fires a signal.
pub struct OrbBreakout {
    /// Number of bars that define the opening range (e.g., 15 for 15-minute bars).
    pub orb_minutes: usize,
}

impl OrbBreakout {
    /// Creates a new ORB detector with the given opening range length in bars.
    pub fn new(orb_minutes: usize) -> Self {
        Self { orb_minutes }
    }
}

impl Detector for OrbBreakout {
    fn name(&self) -> &'static str {
        "orb_breakout"
    }

    fn detect(&self, series: &Series) -> Vec<Firing> {
        let mut out = Vec::new();

        // Track opening range state per day
        let mut current_day: Option<i64> = None;
        let mut orb_start_idx: Option<usize> = None;
        let mut orb_high: Option<f64> = None;
        let mut orb_low: Option<f64> = None;
        let mut orb_completed = false;

        for i in 0..series.len() {
            let day = series.days[i];
            let high = series.high[i];
            let low = series.low[i];
            let close = series.close[i];

            // Check if we've moved to a new day
            if current_day != Some(day) {
                // New day - reset opening range tracking
                current_day = Some(day);
                orb_start_idx = Some(i);
                orb_high = Some(high);
                orb_low = Some(low);
                orb_completed = false;
                continue;
            }

            // We're in the same day - update opening range if still collecting
            if orb_completed {
                // Already have the opening range, check for breakout
                let orb_h = orb_high.unwrap();
                let orb_l = orb_low.unwrap();

                // Break above opening range high
                if close > orb_h {
                    out.push(Firing {
                        code: "orb_breakout_up",
                        bar: i,
                        detail: format!(
                            "Price {close:.2} broke above opening range high {orb_h:.2}"
                        ),
                        evidence: vec![("orb_high", orb_h), ("close", close)],
                    });
                }
                // Break below opening range low
                else if close < orb_l {
                    out.push(Firing {
                        code: "orb_breakdown_down",
                        bar: i,
                        detail: format!(
                            "Price {close:.2} broke below opening range low {orb_l:.2}"
                        ),
                        evidence: vec![("orb_low", orb_l), ("close", close)],
                    });
                }
            } else {
                // Still collecting the opening range
                orb_high = Some(orb_high.unwrap().max(high));
                orb_low = Some(orb_low.unwrap().min(low));

                // Check if we've collected enough bars for the opening range
                let bars_in_range = i - orb_start_idx.unwrap() + 1;
                if bars_in_range >= self.orb_minutes {
                    orb_completed = true;
                }
            }
        }

        out
    }
}
