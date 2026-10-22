//! CPR Breakout detector.
//!
//! Fires when price breaks above the CPR Top Central Pivot (TC) or below the
//! CPR Bottom Central Pivot (BC). Uses the CPR indicator from honba-indicators.

use crate::detector::{Detector, Firing};
use crate::series::Series;

/// CPR Breakout detector: fires when price breaks above TC or below BC.
pub struct CprBreakout;

impl Detector for CprBreakout {
    fn name(&self) -> &'static str {
        "cpr_breakout"
    }

    fn detect(&self, series: &Series) -> Vec<Firing> {
        let mut out = Vec::new();
        for i in 1..series.len() {
            let Some(tc) = series.cpr_tc[i] else {
                continue;
            };
            let Some(bc) = series.cpr_bc[i] else {
                continue;
            };
            let close = series.close[i];
            let prev_close = series.close[i - 1];
            let prev_tc = series.cpr_tc[i - 1];
            let prev_bc = series.cpr_bc[i - 1];

            // Price broke above TC
            if prev_close <= prev_tc.unwrap_or(f64::INFINITY) && close > tc {
                out.push(Firing {
                    code: "cpr_breakout_up",
                    bar: i,
                    detail: format!(
                        "Price {close:.2} broke above CPR Top Central Pivot {tc:.2}"
                    ),
                    evidence: vec![("cpr_tc", tc), ("close", close)],
                });
            }
            // Price broke below BC
            else if prev_close >= prev_bc.unwrap_or(f64::NEG_INFINITY) && close < bc {
                out.push(Firing {
                    code: "cpr_breakdown_down",
                    bar: i,
                    detail: format!(
                        "Price {close:.2} broke below CPR Bottom Central Pivot {bc:.2}"
                    ),
                    evidence: vec![("cpr_bc", bc), ("close", close)],
                });
            }
        }
        out
    }
}
