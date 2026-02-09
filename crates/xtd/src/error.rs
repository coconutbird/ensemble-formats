//! Error types for XTD parsing.

use thiserror::Error;

/// XTD parsing errors.
#[derive(Debug, Error)]
pub enum Error {
    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// ECF error.
    #[error("ECF error: {0}")]
    Ecf(#[from] ecf::Error),

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
}

/// Result type for XTD operations.
pub type Result<T> = std::result::Result<T, Error>;
