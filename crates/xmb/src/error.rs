//! Error types for XMB parsing and writing.

use alloc::string::String;
use thiserror::Error;

/// Result type alias for XMB operations.
pub type Result<T> = core::result::Result<T, Error>;

/// Error types for XMB operations.
#[derive(Debug, Error)]
pub enum Error {
    /// ECF format error.
    #[error("ECF error: {0}")]
    Ecf(#[from] ecf::Error),

    /// BDT format error.
    #[error("BDT error: {0}")]
    Bdt(#[from] bdt::Error),

    /// Invalid XMB signature in the packed data header.
    #[error("invalid signature: expected 0x{expected:08X}, got 0x{actual:08X}")]
    InvalidSignature { expected: u32, actual: u32 },

    /// Invalid ECF file ID (header ID mismatch).
    #[error("invalid file ID: expected 0x{expected:08X}, got 0x{actual:08X}")]
    InvalidFileId { expected: u32, actual: u32 },

    /// Required ECF chunk not found.
    #[error("required chunk not found: 0x{0:016X}")]
    ChunkNotFound(u64),

    /// XML parsing failed.
    #[error("XML parse error: {0}")]
    Xml(String),

    /// Input was detected as XML text but contains invalid UTF-8.
    #[error("invalid UTF-8: {0}")]
    InvalidUtf8(#[from] core::str::Utf8Error),

    /// Data truncated unexpectedly.
    #[error("unexpected end of data")]
    UnexpectedEof,
}
