//! VWAP Cross detector.
//!
//! Fires when price crosses above (vwap_cross_up) or below (vwap_cross_down) the
//! session VWAP. Uses the VWAP indicator from honba-indicators.

use crate::detector::{Detector, Firing};
use crate::series::Series;

/// VWAP Cross detector: fires when price crosses above or below the session VWAP.
pub struct VwapCross;

impl Detector for VwapCross {
    fn name(&self) -> &'static str {
        "vwap_cross"
    }

    fn detect(&self, series: &Series) -> Vec<Firing> {
        let mut out = Vec::new();
        for i in 1..series.len() {
            let Some(vwap) = series.vwap[i] else {
                continue;
            };
            let prev_vwap = series.vwap[i - 1];
            let prev_close = series.close[i - 1];
            let close = series.close[i];

            let prev_vwap_val = prev_vwap.unwrap_or(0.0);

            // Price crossed above VWAP
            if prev_close <= prev_vwap_val && close > vwap {
                out.push(Firing {
                    code: "vwap_cross_up",
                    bar: i,
                    detail: format!(
                        "Price {close:.2} crossed above VWAP {vwap:.2}"
                    ),
                    evidence: vec![("vwap", vwap), ("close", close)],
                });
            }
            // Price crossed below VWAP
            else if prev_close >= prev_vwap_val && close < vwap {
                out.push(Firing {
                    code: "vwap_cross_down",
                    bar: i,
                    detail: format!(
                        "Price {close:.2} crossed below VWAP {vwap:.2}"
                    ),
                    evidence: vec![("vwap", vwap), ("close", close)],
                });
            }
        }
        out
    }
}
