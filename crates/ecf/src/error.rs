//! Error and result types for ECF parsing and serialisation.

use alloc::string::String;
use thiserror::Error;

/// ECF error type.
#[derive(Debug, Error)]
pub enum Error {
    /// Invalid ECF magic number.
    #[error("invalid ECF magic: expected 0x{expected:08X}, found 0x{found:08X}")]
    InvalidMagic { expected: u32, found: u32 },

    /// Header adler32 checksum mismatch.
    #[error("ECF header checksum mismatch: expected 0x{expected:08X}, computed 0x{computed:08X}")]
    HeaderChecksumMismatch { expected: u32, computed: u32 },

    /// Chunk data adler32 checksum mismatch.
    #[error(
        "ECF chunk {index} checksum mismatch: expected 0x{expected:08X}, computed 0x{computed:08X}"
    )]
    ChunkChecksumMismatch {
        index: usize,
        expected: u32,
        computed: u32,
    },

    /// Chunk index out of bounds.
    #[error("chunk not found: 0x{0:016X}")]
    ChunkNotFound(u64),

    /// Data too short for the expected structure.
    #[error("unexpected end of data")]
    UnexpectedEof,

    /// Invalid BDeflateStream signature.
    #[error("invalid BDeflateStream signature: 0x{0:08X}")]
    InvalidDeflateStreamSignature(u32),

    /// Decompression error.
    #[error("decompression error: {0}")]
    DecompressionError(String),
}

/// Result type for ECF operations.
pub type Result<T> = core::result::Result<T, Error>;
