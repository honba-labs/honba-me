#![deny(unsafe_code)]

//! Explainable buy/sell signal detectors (ADR 0031, ROADMAP E12).
//!
//! Pure and causal: given a closed bar series, [`engine::compute`] runs the detectors a
//! [`catalog::Ruleset`] enables, scores their firings into a [`Signal`], and
//! attaches the [`Reason`]s that fired. There is no clock, no I/O and no
//! lookahead: a signal at bar *N* uses only bars `<= N`, so truncating future bars never changes
//! an earlier signal.
//!
//! Two horizons share one engine: `signals/day:v1` (intraday) and `signals/swing:v1`
//! (positional), which differ only in parameters, detector set, cooldown and strength threshold
//! ([`catalog::RULESETS`]).
//!
//! ```
//! use honba_core::{Bar, Exchange, InstrumentId, Price, Quantity};
//! use honba_signals::{catalog::SignalHorizon, signals_for_horizon};
//! use rust_decimal::Decimal;
//!
//! let id = InstrumentId::new(Exchange::Nse, "X");
//! let bars: Vec<Bar> = (0..120)
//!     .map(|i| {
//!         let px = Price::new(Decimal::from(100 + i));
//!         let ts = chrono::DateTime::from_timestamp_nanos((i as i64) * 86_400_000_000_000);
//!         Bar {
//!             instrument: id.clone(),
//!             open: px,
//!             high: Price::new(Decimal::from(101 + i)),
//!             low: Price::new(Decimal::from(99 + i)),
//!             close: px,
//!             volume: Quantity::from_u64(1_000),
//!             timestamp: ts,
//!         }
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
pub use engine::{Reason, Signal, SignalDirection, UnixNanos, compute, signals_for_horizon};

#[cfg(test)]
mod tests;
