//! Volume detectors.

use crate::detector::{Detector, Firing};
use crate::series::Series;

/// Fires when volume exceeds a multiple of its moving average; the polarity follows the bar's
/// own direction (close vs open): `rel_volume_spike_up` / `rel_volume_spike_down`.
pub(crate) struct RelativeVolume {
    pub window: usize,
    pub multiple: f64,
}

impl Detector for RelativeVolume {
    fn name(&self) -> &'static str {
        "relative_volume"
    }

    fn detect(&self, series: &Series) -> Vec<Firing> {
        let mut out = Vec::new();
        for i in 0..series.len() {
            let Some(avg) = series.vol_sma[i] else {
                continue;
            };
            if avg <= 0.0 {
                continue;
            }
            let ratio = series.volume[i] / avg;
            if ratio < self.multiple {
                continue;
            }
            let up = series.close[i] >= series.open[i];
            out.push(Firing {
                code: if up {
                    "rel_volume_spike_up"
                } else {
                    "rel_volume_spike_down"
                },
                bar: i,
                detail: format!(
                    "Volume {:.0} is {:.1}x the {}-bar average",
                    series.volume[i], ratio, self.window
                ),
                evidence: vec![("relative_volume", ratio)],
            });
        }
        out
    }
}
