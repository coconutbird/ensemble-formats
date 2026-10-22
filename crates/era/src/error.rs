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
    InvalidArchiveMagic {
        /// Magic number that the format requires.
        expected: u32,
        /// Magic number that the data contains.
        found: u32,
    },

    /// Chunk index out of bounds.
    #[error("chunk index {index} out of bounds (count: {count})")]
    ChunkIndexOutOfBounds {
        /// Requested chunk index.
        index: usize,
        /// Number of chunks in the archive.
        count: usize,
    },

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
    InvalidSignatureMagic {
        /// Magic number that the format requires.
        expected: u32,
        /// Magic number that the data contains.
        found: u32,
    },

    /// Invalid signature tree depth.
    #[error("invalid signature tree depth: {depth} (must be 2..=32)")]
    InvalidTreeDepth {
        /// Tree depth that the signature declares.
        depth: u8,
    },

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
