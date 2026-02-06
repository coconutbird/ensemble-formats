//! Error types for XMB parsing and writing.

use thiserror::Error;

/// Result type alias for XMB operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Error types for XMB operations.
#[derive(Debug, Error)]
pub enum Error {
    /// I/O error during reading or writing.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// ECF format error.
    #[error("ECF error: {0}")]
    Ecf(#[from] ecf::Error),

    /// Invalid XMB signature.
    #[error("Invalid XMB signature: expected 0x{expected:08X}, got 0x{actual:08X}")]
    InvalidXmbSignature { expected: u32, actual: u32 },

    /// Invalid XMB file ID (ECF header ID mismatch).
    #[error("Invalid XMB file ID: expected 0x{expected:08X}, got 0x{actual:08X}")]
    InvalidXmbFileId { expected: u32, actual: u32 },

    /// Required chunk not found.
    #[error("Required chunk not found: 0x{0:016X}")]
    ChunkNotFound(u64),

    /// Invalid variant type.
    #[error("Invalid variant type: {0}")]
    InvalidVariantType(u8),

    /// Invalid string encoding.
    #[error("Invalid string: {0}")]
    InvalidString(String),

    /// Data truncated unexpectedly.
    #[error("Unexpected end of data")]
    UnexpectedEof,

    /// Invalid node structure.
    #[error("Invalid node structure: {0}")]
    InvalidNode(String),

    /// Adler32 checksum mismatch.
    #[error("Adler32 checksum mismatch: expected 0x{expected:08X}, got 0x{actual:08X}")]
    ChecksumMismatch { expected: u32, actual: u32 },
}
