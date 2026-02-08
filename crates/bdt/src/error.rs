//! Error types for BBinaryDataTree operations.

use thiserror::Error;

/// Result type alias for BDT operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Error types for BDT operations.
#[derive(Debug, Error)]
pub enum Error {
    /// I/O error during reading or writing.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Invalid variant type.
    #[error("Invalid variant type: {0}")]
    InvalidVariantType(u8),

    /// Invalid string encoding.
    #[error("Invalid string: {0}")]
    InvalidString(String),

    /// Data truncated unexpectedly.
    #[error("Unexpected end of data")]
    UnexpectedEof,
}
