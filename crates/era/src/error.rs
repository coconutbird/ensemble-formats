//! Error types for ERA parsing.

use alloc::string::String;
use thiserror::Error;

/// Result type for ERA operations.
pub type Result<T> = core::result::Result<T, Error>;

/// Errors that can occur when parsing ERA files.
#[derive(Debug, Error)]
pub enum Error {
    /// ECF error.
    #[error("ECF error: {0}")]
    Ecf(#[from] ecf::Error),

    /// Invalid archive magic number.
    #[error("invalid archive magic: expected 0x{expected:08X}, found 0x{found:08X}")]
    InvalidArchiveMagic { expected: u32, found: u32 },

    /// Chunk index out of bounds.
    #[error("chunk index {index} out of bounds (count: {count})")]
    ChunkIndexOutOfBounds { index: usize, count: usize },

    /// Decompression error.
    #[error("decompression error: {0}")]
    DecompressionError(String),

    /// Data truncated unexpectedly.
    #[error("unexpected end of data")]
    UnexpectedEof,

    /// Operation was cancelled by the progress callback.
    #[error("operation cancelled")]
    Cancelled,
}
