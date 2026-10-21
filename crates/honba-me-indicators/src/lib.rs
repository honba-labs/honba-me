#![deny(unsafe_code)]

//! Incremental technical indicators for the Honba platform.
//!
//! Every indicator implements [`Indicator`] and consumes inputs one at a
//! time. Nothing buffers the full series; each indicator holds only the
//! state it needs, so it can run over an unbounded live stream.
//!
//! # Example
//!
//! ```
//! use honba_indicators::{Indicator, Sma, Rsi, BollingerBands};
//!
//! let mut sma = Sma::new(20);
//! let mut rsi = Rsi::new(14);
//! let mut bb  = BollingerBands::new(20, 2.0);
//!
//! for px in [100.0, 101.0, 99.5, 102.0, 103.5] {
//!     let _ = sma.update(px);
//!     let _ = rsi.update(px);
//!     let _ = bb.update(px);
//! }
//! ```

pub mod adx;
pub mod atr;
pub mod bollinger;
pub mod cpr;
pub mod donchian;
pub mod ema;
pub mod indicator;
pub mod macd;
pub mod obv;
pub mod rma;
pub mod render;
pub mod rsi;
pub mod screener;
pub mod sma;
pub mod stochastic;
pub mod supertrend;
pub mod vwap;
pub mod williams_r;

pub use adx::Adx;
pub use atr::Atr;
pub use bollinger::{BollingerBands, BollingerValue};
pub use cpr::Cpr;
pub use donchian::{Donchian, DonchianValue};
pub use ema::Ema;
pub use indicator::Indicator;
pub use macd::{Macd, MacdValue};
pub use obv::Obv;
pub use rma::Rma;
pub use rsi::Rsi;
pub use sma::Sma;
pub use stochastic::{Stochastic, StochasticValue};
pub use supertrend::Supertrend;
pub use vwap::Vwap;
pub use williams_r::WilliamsR;

#[cfg(test)]
mod tests;
