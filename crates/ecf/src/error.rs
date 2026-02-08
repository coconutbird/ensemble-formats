//! Error types for ECF operations.

use thiserror::Error;

/// ECF error type.
#[derive(Debug, Error)]
pub enum Error {
    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Invalid ECF magic number.
    #[error("invalid ECF magic: expected 0x{expected:08X}, found 0x{found:08X}")]
    InvalidMagic { expected: u32, found: u32 },

    /// Chunk not found.
    #[error("chunk not found: 0x{0:016X}")]
    ChunkNotFound(u64),

    /// Unexpected end of file.
    #[error("unexpected end of file")]
    UnexpectedEof,

    /// Invalid BDeflateStream signature.
    #[error("invalid BDeflateStream signature: 0x{0:08X}")]
    InvalidDeflateStreamSignature(u32),

    /// Decompression error.
    #[error("decompression error: {0}")]
    DecompressionError(String),
}

/// Result type for ECF operations.
pub type Result<T> = std::result::Result<T, Error>;
