//! Indicator series computed once per evaluation and shared by every detector.
//!
//! Each series is a `Vec<Option<f64>>` of the same length as the bars: `None` until the
//! indicator has primed, so a detector naturally cannot fire before its inputs are warm. Built
//! with the incremental indicators in `honba-indicators`, so the computation is causal.

use honba_indicators::{Adx, Cpr, Ema, Indicator, Macd, MacdValue, Obv, Rsi, Sma, Supertrend, Vwap, Atr};
use honba_core::Bar;

use crate::catalog::Params;

/// The bar columns plus the indicator series the detectors read.
pub(crate) struct Series {
    /// Open prices, one per bar.
    pub open: Vec<f64>,
    /// High prices.
    pub high: Vec<f64>,
    /// Low prices.
    pub low: Vec<f64>,
    /// Close prices.
    pub close: Vec<f64>,
    /// Volumes.
    pub volume: Vec<f64>,
    /// UTC day number of each bar (`ts_event / 1 day`), for session grouping.
    pub days: Vec<i64>,
    /// Fast EMA of close.
    pub ema_fast: Vec<Option<f64>>,
    /// Slow EMA of close.
    pub ema_slow: Vec<Option<f64>>,
    /// RSI of close.
    pub rsi: Vec<Option<f64>>,
    /// MACD of close.
    pub macd: Vec<Option<MacdValue>>,
    /// Simple moving average of volume.
    pub vol_sma: Vec<Option<f64>>,
    /// VWAP of close.
    pub vwap: Vec<Option<f64>>,
    /// CPR Pivot value.
    pub cpr_pivot: Vec<Option<f64>>,
    /// CPR Bottom Central Pivot.
    pub cpr_bc: Vec<Option<f64>>,
    /// CPR Top Central Pivot.
    pub cpr_tc: Vec<Option<f64>>,
    /// ATR (Average True Range).
    pub atr: Vec<Option<f64>>,
}

impl Series {
    /// Computes every series over a closed bar window.
    pub fn compute(bars: &[Bar], params: &Params) -> Self {
        let n = bars.len();
        let mut open = Vec::with_capacity(n);
        let mut high = Vec::with_capacity(n);
        let mut low = Vec::with_capacity(n);
        let mut close = Vec::with_capacity(n);
        let mut volume = Vec::with_capacity(n);
        let mut days = Vec::with_capacity(n);
        let mut ema_fast = Vec::with_capacity(n);
        let mut ema_slow = Vec::with_capacity(n);
        let mut rsi = Vec::with_capacity(n);
        let mut macd = Vec::with_capacity(n);
        let mut vol_sma = Vec::with_capacity(n);
        let mut vwap = Vec::with_capacity(n);
        let mut cpr_pivot = Vec::with_capacity(n);
        let mut cpr_bc = Vec::with_capacity(n);
        let mut cpr_tc = Vec::with_capacity(n);
        let mut atr = Vec::with_capacity(n);

        let mut ema_fast_ind = Ema::new(params.ema_fast);
        let mut ema_slow_ind = Ema::new(params.ema_slow);
        let mut rsi_ind = Rsi::new(params.rsi_period);
        let mut macd_ind = Macd::new(params.macd_fast, params.macd_slow, params.macd_signal);
        let mut vol_sma_ind = Sma::new(params.vol_window);
        let mut vwap_ind = Vwap::new();
        let mut cpr_ind = Cpr::new();
        let mut atr_ind = Atr::new(params.rsi_period);

        for bar in bars {
            let c = bar.close.0.to_string().parse::<f64>().unwrap_or(0.0);
            open.push(bar.open.0.to_string().parse::<f64>().unwrap_or(0.0));
            high.push(bar.high.0.to_string().parse::<f64>().unwrap_or(0.0));
            low.push(bar.low.0.to_string().parse::<f64>().unwrap_or(0.0));
            close.push(c);
            volume.push(bar.volume.0.to_string().parse::<f64>().unwrap_or(0.0));
            days.push(bar.timestamp.timestamp_millis() / 86_400_000);
            ema_fast.push(ema_fast_ind.update(c));
            ema_slow.push(ema_slow_ind.update(c));
            rsi.push(rsi_ind.update(c));
            macd.push(macd_ind.update(c));
            vol_sma.push(vol_sma_ind.update(volume.last().copied().unwrap_or(0.0)));
            vwap.push(vwap_ind.update(bar));
            cpr_pivot.push(cpr_ind.update(bar));
            cpr_bc.push(cpr_ind.bc());
            cpr_tc.push(cpr_ind.tc());
            atr.push(atr_ind.update(bar));
        }

        Self {
            open,
            high,
            low,
            close,
            volume,
            days,
            ema_fast,
            ema_slow,
            rsi,
            macd,
            vol_sma,
            vwap,
            cpr_pivot,
            cpr_bc,
            cpr_tc,
            atr,
        }
    }

    /// Number of bars.
    pub fn len(&self) -> usize {
        self.close.len()
    }
}
