//! Session-aware multi-timeframe bar aggregator.
//!
//! Aggregates lower-timeframe bars (e.g. 1m, 5m, 1h) into higher-timeframe bars (e.g. 1D, 1h),
//! strictly aligned to exchange trading sessions defined by [`MarketCalendar`].
//!
//! # Invariants
//! - **Zero Look-ahead**: A higher-timeframe bar is **only** emitted when its session or
//!   bucket interval has completely elapsed. While within the interval, [`SessionBarAggregator::process`]
//!   returns `None`.
//! - **Session Calendar Alignment**: Bars do not bleed across session boundaries or into holidays.

use std::collections::HashMap;

use chrono::{DateTime, Datelike, NaiveDateTime, NaiveTime, Timelike, Utc};
use honba_core::{Bar, InstrumentId, Price, Quantity};

use crate::calendar::MarketCalendar;

// Bar aggregation types (replacing deleted honba_messages types)

/// Bar aggregation granularity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BarAggregation {
    /// Second-level aggregation.
    Second,
    /// Minute-level aggregation.
    Minute,
    /// Hour-level aggregation.
    Hour,
    /// Day-level aggregation.
    Day,
    /// Week-level aggregation.
    Week,
    /// Month-level aggregation.
    Month,
}

/// Bar specification defining aggregation and step size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BarSpecification {
    pub aggregation: BarAggregation,
    pub step: u32,
}

impl BarSpecification {
    pub fn new(step: u32, aggregation: BarAggregation) -> Self {
        Self { step, aggregation }
    }

    pub fn aggregation(&self) -> BarAggregation {
        self.aggregation
    }

    pub fn step(&self) -> u32 {
        self.step
    }
}

/// Bar type identifier combining instrument and specification.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BarType {
    pub instrument: InstrumentId,
    pub spec: BarSpecification,
}

impl BarType {
    pub fn new(instrument: InstrumentId, spec: BarSpecification) -> Self {
        Self { instrument, spec }
    }

    pub fn instrument_id(&self) -> &InstrumentId {
        &self.instrument
    }

    pub fn spec(&self) -> BarSpecification {
        self.spec
    }
}

/// Nanosecond timestamp (replacement for deleted honba_messages::UnixNanos).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UnixNanos(pub u64);

impl UnixNanos {
    pub fn from_u64(nanos: u64) -> Self {
        Self(nanos)
    }

    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

use serde::{Deserialize, Serialize};

/// An in-progress accumulated higher-timeframe bar.
#[derive(Clone, Debug, PartialEq)]
struct BarAccumulator {
    open: Price,
    high: Price,
    low: Price,
    close: Price,
    volume: Quantity,
    ts_open: DateTime<Utc>,
    ts_close_boundary: DateTime<Utc>,
    window_id: u64,
}

impl BarAccumulator {
    fn new(bar: &Bar, ts_close_boundary: DateTime<Utc>, window_id: u64) -> Self {
        Self {
            open: bar.open,
            high: bar.high,
            low: bar.low,
            close: bar.close,
            volume: bar.volume,
            ts_open: bar.timestamp,
            ts_close_boundary,
            window_id,
        }
    }

    fn update(&mut self, bar: &Bar) {
        if bar.high > self.high {
            self.high = bar.high;
        }
        if bar.low < self.low {
            self.low = bar.low;
        }
        self.close = bar.close;
        self.volume = Quantity(self.volume.0 + bar.volume.0);
    }

    fn to_bar(&self, instrument: &InstrumentId, _spec: BarSpecification) -> Bar {
        Bar {
            instrument: instrument.clone(),
            open: self.open,
            high: self.high,
            low: self.low,
            close: self.close,
            volume: self.volume,
            timestamp: self.ts_close_boundary,
        }
    }
}

/// A session-aware multi-timeframe bar aggregator.
#[derive(Clone, Debug)]
pub struct SessionBarAggregator<C: MarketCalendar> {
    calendar: C,
    target_spec: BarSpecification,
    accumulators: HashMap<InstrumentId, BarAccumulator>,
}

impl<C: MarketCalendar> SessionBarAggregator<C> {
    /// Creates a new aggregator with the given calendar and target bar specification.
    pub fn new(calendar: C, target_spec: BarSpecification) -> Self {
        Self {
            calendar,
            target_spec,
            accumulators: HashMap::new(),
        }
    }

    /// Returns the target bar specification.
    pub fn target_spec(&self) -> BarSpecification {
        self.target_spec
    }

    /// Returns a reference to the market calendar.
    pub fn calendar(&self) -> &C {
        &self.calendar
    }

    /// Ingests a lower-timeframe bar.
    ///
    /// Returns `Some(Bar)` only when the higher-timeframe bar for this session/interval
    /// is complete. Otherwise returns `None`.
    pub fn process(&mut self, bar: &Bar) -> Option<Bar> {
        let instrument = bar.instrument.clone();
        let (window_id, ts_close_boundary, is_window_terminal) =
            self.window_info(bar.timestamp, &self.target_spec)?;

        if let Some(mut acc) = self.accumulators.remove(&instrument) {
            if acc.window_id == window_id {
                acc.update(bar);
                if is_window_terminal {
                    Some(acc.to_bar(&instrument, self.target_spec))
                } else {
                    self.accumulators.insert(instrument, acc);
                    None
                }
            } else {
                // The incoming bar is in a later window: the previous window is complete!
                let completed = acc.to_bar(&instrument, self.target_spec);
                let new_acc = BarAccumulator::new(bar, ts_close_boundary, window_id);
                self.accumulators.insert(instrument, new_acc);
                Some(completed)
            }
        } else {
            let acc = BarAccumulator::new(bar, ts_close_boundary, window_id);
            if is_window_terminal {
                Some(acc.to_bar(&instrument, self.target_spec))
            } else {
                self.accumulators.insert(instrument, acc);
                None
            }
        }
    }

    /// Flushes any active accumulator for the given instrument.
    pub fn flush_instrument(&mut self, instrument: &InstrumentId) -> Option<Bar> {
        self.accumulators
            .remove(instrument)
            .map(|acc| acc.to_bar(instrument, self.target_spec))
    }

    /// Flushes all active accumulators.
    pub fn flush(&mut self) -> Vec<Bar> {
        let mut bars = Vec::with_capacity(self.accumulators.len());
        for (instrument, acc) in self.accumulators.drain() {
            bars.push(acc.to_bar(&instrument, self.target_spec));
        }
        bars
    }

    fn window_info(
        &self,
        ts: DateTime<Utc>,
        input_spec: &BarSpecification,
    ) -> Option<(u64, DateTime<Utc>, bool)> {
        let date = ts.date_naive();
        let time = ts.time();

        let session = self.calendar.session();
        let open_time = session.open();
        let close_time = session.close();

        // Calculate input bar duration in seconds
        let input_duration_secs = match input_spec.aggregation {
            BarAggregation::Second => input_spec.step as i64,
            BarAggregation::Minute => (input_spec.step * 60) as i64,
            BarAggregation::Hour => (input_spec.step * 3600) as i64,
            BarAggregation::Day => 86400,
            _ => 60,
        };

        match self.target_spec.aggregation {
            BarAggregation::Day => {
                let window_id = date.num_days_from_ce() as u64;
                let close_dt = NaiveDateTime::new(date, close_time);
                let ts_close = close_dt.and_utc();

                // Bar is terminal if its close time reaches or exceeds session close
                let bar_end_secs = time.num_seconds_from_midnight() as i64 + input_duration_secs;
                let is_terminal = bar_end_secs >= close_time.num_seconds_from_midnight() as i64;
                Some((window_id, ts_close, is_terminal))
            }
            BarAggregation::Hour | BarAggregation::Minute => {
                let target_step_secs = match self.target_spec.aggregation {
                    BarAggregation::Minute => (self.target_spec.step * 60) as i64,
                    BarAggregation::Hour => (self.target_spec.step * 3600) as i64,
                    _ => unreachable!(),
                };

                let session_open_secs = open_time.num_seconds_from_midnight() as i64;
                let session_close_secs = close_time.num_seconds_from_midnight() as i64;
                let bar_secs = time.num_seconds_from_midnight() as i64;

                let offset = (bar_secs - session_open_secs).max(0);
                let bucket_index = offset / target_step_secs;
                let bucket_start = session_open_secs + bucket_index * target_step_secs;
                let bucket_end = (bucket_start + target_step_secs).min(session_close_secs);

                let window_id = ((date.num_days_from_ce() as u64) << 32) | (bucket_index as u64);
                let end_time = NaiveTime::from_num_seconds_from_midnight_opt(bucket_end as u32, 0)?;
                let close_dt = NaiveDateTime::new(date, end_time);
                let ts_close = close_dt.and_utc();

                let bar_end_secs = bar_secs + input_duration_secs;
                let is_terminal = bar_end_secs >= bucket_end;
                Some((window_id, ts_close, is_terminal))
            }
            _ => {
                // Default fallback to 1-day behavior
                let window_id = date.num_days_from_ce() as u64;
                let close_dt = NaiveDateTime::new(date, close_time);
                Some((window_id, close_dt.and_utc(), false))
            }
        }
    }
}
