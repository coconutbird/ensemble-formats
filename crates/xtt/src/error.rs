//! Error types for XTT parsing.

use alloc::string::String;
use thiserror::Error;

/// XTT parsing errors.
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

    /// Invalid XTT version.
    #[error("Invalid XTT version: expected {expected:#06X}, got {actual:#06X}")]
    InvalidVersion { expected: i32, actual: i32 },

    /// Invalid XTT ECF file identifier.
    #[error("Invalid XTT file ID: expected {expected:#010X}, got {actual:#010X}")]
    InvalidFileId { expected: u32, actual: u32 },

    /// Missing required chunk.
    #[error("Missing required chunk: {0:#06X}")]
    MissingChunk(u64),

    /// Invalid chunk data.
    #[error("Invalid chunk data: {0}")]
    InvalidChunkData(String),

    /// Invalid header size.
    #[error("Invalid header size: expected {expected}, got {actual}")]
    InvalidHeaderSize { expected: usize, actual: usize },

    /// A count, size, or offset cannot be represented by the on-disk format.
    #[error("{0} is too large for the XTT format")]
    SizeOverflow(&'static str),

    /// A fixed-size filename does not fit without truncation.
    #[error("{context} is too long: maximum {max} bytes, got {actual}")]
    StringTooLong {
        context: &'static str,
        max: usize,
        actual: usize,
    },

    /// The typed representation cannot preserve an unknown chunk when writing.
    #[error("Cannot write unsupported XTT chunk {0:#018X}")]
    UnsupportedChunk(u64),
}

/// Result type for XTT operations.
pub type Result<T> = core::result::Result<T, Error>;
