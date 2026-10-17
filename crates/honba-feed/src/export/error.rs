//! Error types for the export crate.

use thiserror::Error;

/// Convenience alias for results produced by this crate.
pub type Result<T> = std::result::Result<T, ExportError>;

/// Errors that exporters can produce.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ExportError {
    /// An I/O error occurred while writing.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// A JSON serialization error occurred.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}
