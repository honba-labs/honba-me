//! Trend detectors.

use crate::detector::{Detector, Firing};
use crate::series::Series;

/// Fires when the fast EMA crosses the slow EMA: `ema_cross_up` / `ema_cross_down`.
pub(crate) struct EmaCross {
    pub fast: usize,
    pub slow: usize,
}

impl Detector for EmaCross {
    fn name(&self) -> &'static str {
        "ema_cross"
    }

    fn detect(&self, series: &Series) -> Vec<Firing> {
        let mut out = Vec::new();
        for i in 1..series.len() {
            let (Some(f_prev), Some(s_prev), Some(f_now), Some(s_now)) = (
                series.ema_fast[i - 1],
                series.ema_slow[i - 1],
                series.ema_fast[i],
                series.ema_slow[i],
            ) else {
                continue;
            };
            let prev = f_prev - s_prev;
            let now = f_now - s_now;
            if prev <= 0.0 && now > 0.0 {
                out.push(Firing {
                    code: "ema_cross_up",
                    bar: i,
                    detail: format!(
                        "EMA{} ({:.2}) crossed above EMA{} ({:.2})",
                        self.fast, f_now, self.slow, s_now
                    ),
                    evidence: vec![("ema_fast", f_now), ("ema_slow", s_now)],
                });
            } else if prev >= 0.0 && now < 0.0 {
                out.push(Firing {
                    code: "ema_cross_down",
                    bar: i,
                    detail: format!(
                        "EMA{} ({:.2}) crossed below EMA{} ({:.2})",
                        self.fast, f_now, self.slow, s_now
                    ),
                    evidence: vec![("ema_fast", f_now), ("ema_slow", s_now)],
                });
            }
        }
        out
    }
}
