//! Error types for DDX parsing.

use alloc::string::String;
use thiserror::Error;

/// DDX-specific errors.
#[derive(Error, Debug)]
pub enum Error {
    /// ECF parsing error.
    #[error("ECF error: {0}")]
    Ecf(#[from] ecf::Error),

    /// Invalid DDX magic number.
    #[error("Invalid DDX header magic: expected 0xDDBB7738, got 0x{0:08X}")]
    InvalidMagic(u32),

    /// Invalid ECF file ID for DDX.
    #[error("Invalid ECF file ID: expected 0x13CF5D01, got 0x{0:08X}")]
    InvalidEcfFileId(u32),

    /// Missing header chunk.
    #[error("Missing DDX header chunk")]
    MissingHeaderChunk,

    /// Missing mip0 data chunk.
    #[error("Missing mip0 data chunk")]
    MissingMip0Chunk,

    /// Header too short.
    #[error("DDX header too short: expected at least {expected} bytes, got {actual}")]
    HeaderTooShort { expected: usize, actual: usize },

    /// Invalid data format.
    #[error("Invalid DDX data format: {0}")]
    InvalidDataFormat(u32),

    /// Invalid resource type.
    #[error("Invalid DDX resource type: {0}")]
    InvalidResourceType(u32),

    /// Unsupported DDX version.
    #[error("Unsupported DDX version: {0} (minimum required: {1})")]
    UnsupportedVersion(u16, u16),

    /// Decompression error.
    #[error("Decompression error: {0}")]
    DecompressionError(String),

    /// Invalid checksum.
    #[error("Invalid checksum: expected 0x{expected:08X}, got 0x{actual:08X}")]
    InvalidChecksum { expected: u32, actual: u32 },

    /// Unsupported texture format for decoding.
    #[error("Unsupported texture format for decoding: {0:?}")]
    UnsupportedFormat(crate::format::DataFormat),

    /// A cursor read or write failed.
    #[error("I/O error: {0}")]
    Io(#[from] nostdio::IoError),

    /// A calculated size cannot be represented by the DDX or DDS format.
    #[error("{0} is too large for the texture format")]
    SizeOverflow(&'static str),
}

/// Result type for DDX operations.
pub type Result<T> = core::result::Result<T, Error>;
