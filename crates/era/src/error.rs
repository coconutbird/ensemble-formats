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

    /// Chunk data does not satisfy the cipher or archive format requirements.
    #[error("invalid chunk data: {0}")]
    InvalidChunkData(String),

    /// Operation was cancelled by the progress callback.
    #[error("operation cancelled")]
    Cancelled,

    /// Invalid signature magic.
    #[error("invalid signature magic: expected 0x{expected:08X}, found 0x{found:08X}")]
    InvalidSignatureMagic { expected: u32, found: u32 },

    /// Invalid signature tree depth.
    #[error("invalid signature tree depth: {depth} (must be 2..=32)")]
    InvalidTreeDepth { depth: u8 },

    /// Signature verification failed.
    #[error("signature verification failed")]
    SignatureVerifyFailed,

    /// Signature data truncated.
    #[error("signature data truncated")]
    SignatureTruncated,

    /// A count, size, or offset cannot be represented by the ERA format.
    #[error("{0} is too large for the ERA format")]
    SizeOverflow(&'static str),
}
