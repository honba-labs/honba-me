#![deny(unsafe_code)]

//! Explainable buy/sell signal detectors (ADR 0031, ROADMAP E12).
//!
//! Pure and causal: given a closed bar series, [`engine::compute`] runs the detectors a
//! [`catalog::Ruleset`] enables, scores their firings into a [`honba_entities::Signal`], and
//! attaches the [`honba_entities::Reason`]s that fired. There is no clock, no I/O and no
//! lookahead: a signal at bar *N* uses only bars `<= N`, so truncating future bars never changes
//! an earlier signal.
//!
//! Two horizons share one engine: `signals/day:v1` (intraday) and `signals/swing:v1`
//! (positional), which differ only in parameters, detector set, cooldown and strength threshold
//! ([`catalog::RULESETS`]).
//!
//! ```
//! use honba_entities::SignalHorizon;
//! use honba_messages::{
//!     Bar, BarAggregation, BarSpecification, BarType, Exchange, InstrumentId, PriceType, UnixNanos,
//! };
//! use honba_signals::signals_for_horizon;
//!
//! let id = InstrumentId::new("X", Exchange::new("NSE"));
//! let spec = BarSpecification::new(1, BarAggregation::Day, PriceType::Last);
//! let bars: Vec<Bar> = (0..120)
//!     .map(|i| {
//!         let px = 100.0 + (i as f64 * 0.1);
//!         let ts = UnixNanos::from_u64(i * 86_400_000_000_000);
//!         Bar::new(
//!             BarType::new(id.clone(), spec),
//!             px, px + 1.0, px - 1.0, px, 1_000.0, ts, ts,
//!         )
//!     })
//!     .collect();
//! let signals = signals_for_horizon(&bars, &id, "1d", SignalHorizon::Swing);
//! assert!(signals.iter().all(|s| !s.reasons.is_empty()));
//! ```

pub mod catalog;
mod detector;
mod detectors;
pub mod engine;
mod series;

pub use catalog::Params;
pub use engine::{compute, signals_for_horizon};

#[cfg(test)]
mod tests;
