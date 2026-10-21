#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![deny(unsafe_code)]

//! India-specific market structure for the Honba platform.
//!
//! No holiday, tax, or constituent data is hardcoded here. Every value is
//! injected by the caller through the traits in this crate.

pub mod calendar;
pub mod costs;
pub mod error;
pub mod exchange;
pub mod fno;
pub mod profile;
pub mod universes;

pub use error::{IndiaError, Result};
pub use fno::{
    FnoContract, FnoExpiry, FnoParseError, default_fno_lot_size, format_fno_symbol,
    parse_fno_symbol,
};
pub use profile::IndiaMarketProfile;
