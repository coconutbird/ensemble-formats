//! Error and result types for ECF parsing and serialisation.

use alloc::string::String;
use thiserror::Error;

/// ECF error type.
#[derive(Debug, Error)]
pub enum Error {
    /// Invalid ECF magic number.
    #[error("invalid ECF magic: expected 0x{expected:08X}, found 0x{found:08X}")]
    InvalidMagic { expected: u32, found: u32 },

    /// The encoded ECF header is smaller than the fixed header.
    #[error("invalid ECF header size: expected at least {minimum}, found {actual}")]
    InvalidHeaderSize { minimum: usize, actual: usize },

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

    /// Invalid `BDeflateStream` signature.
    #[error("invalid BDeflateStream signature: 0x{0:08X}")]
    InvalidDeflateStreamSignature(u32),

    /// Decompression error.
    #[error("decompression error: {0}")]
    DecompressionError(String),

    /// A collection or offset is too large for the on-disk format.
    #[error("{0} is too large for the ECF format")]
    SizeOverflow(&'static str),

    /// A chunk requests an alignment that cannot be represented.
    #[error("invalid ECF chunk alignment log2: {0}")]
    InvalidAlignment(u8),
}

/// Result type for ECF operations.
pub type Result<T> = core::result::Result<T, Error>;
