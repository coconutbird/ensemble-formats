//! Error types for BBinaryDataTree operations.

use alloc::string::String;
use thiserror::Error;

/// Result type alias for BDT operations.
pub type Result<T> = core::result::Result<T, Error>;

/// Error types for BDT operations.
#[derive(Debug, Error)]
pub enum Error {
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
