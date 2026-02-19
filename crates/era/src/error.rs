//! Error types for ERA parsing

use thiserror::Error;

/// Result type for ERA operations
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur when parsing ERA files
#[derive(Debug, Error)]
pub enum Error {
    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// ECF error
    #[error("ECF error: {0}")]
    Ecf(#[from] ecf::Error),

    /// Invalid archive magic number
    #[error("invalid archive magic: expected 0x{expected:08X}, found 0x{found:08X}")]
    InvalidArchiveMagic { expected: u32, found: u32 },

    /// Chunk index out of bounds
    #[error("chunk index {index} out of bounds (count: {count})")]
    ChunkIndexOutOfBounds { index: usize, count: usize },

    /// Decompression error
    #[error("decompression error: {0}")]
    DecompressionError(String),

    /// Operation was cancelled by the progress callback
    #[error("operation cancelled")]
    Cancelled,
}
