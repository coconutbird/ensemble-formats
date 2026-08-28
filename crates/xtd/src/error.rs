//! Error types for XTD parsing.

use alloc::string::String;
use thiserror::Error;

/// XTD parsing errors.
#[derive(Debug, Error)]
pub enum Error {
    /// Binary I/O error.
    #[error("binary I/O error: {0}")]
    Io(#[from] nostdio::IoError),

    /// ECF error.
    #[error("ECF error: {0}")]
    Ecf(#[from] ecf::Error),

    /// Unexpected end of data.
    #[error("unexpected end of data")]
    UnexpectedEof,

    /// Invalid XTD version.
    #[error("Invalid XTD version: expected {expected:#06X}, got {actual:#06X}")]
    InvalidVersion { expected: i32, actual: i32 },

    /// Missing required chunk.
    #[error("Missing required chunk: {0:#06X}")]
    MissingChunk(u64),

    /// Invalid chunk data.
    #[error("Invalid chunk data: {0}")]
    InvalidChunkData(String),

    /// Invalid header size.
    #[error("Invalid header size: expected {expected}, got {actual}")]
    InvalidHeaderSize { expected: usize, actual: usize },

    /// A count, size, or offset cannot be represented safely.
    #[error("{0} is too large for the XTD format")]
    SizeOverflow(&'static str),
}

/// Result type for XTD operations.
pub type Result<T> = core::result::Result<T, Error>;
