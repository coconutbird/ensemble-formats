//! Error types for `BBinaryDataTree` operations.

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

    /// A collection or offset is too large for the on-disk format.
    #[error("{0} is too large for the BDT format")]
    SizeOverflow(&'static str),

    /// A compact float vector has an unsupported component count.
    #[error("compact float vectors require 2 to 4 components, got {0}")]
    InvalidFloatVectorLength(usize),

    /// A variant cannot be represented by the compact BDT encoding.
    #[error("the {0} variant is not supported by the compact BDT writer")]
    UnsupportedCompactVariant(&'static str),
}
