//! Error types for UAX parsing.

use thiserror::Error;

/// Result type for UAX operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Error type for UAX parsing.
#[derive(Debug, Error)]
pub enum Error {
    /// I/O error during reading.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// ECF parsing error.
    #[error("ECF error: {0}")]
    Ecf(#[from] ecf::Error),

    /// Invalid UAX file ID.
    #[error("Invalid UAX file ID: expected 0xAAC93747, got 0x{0:08X}")]
    InvalidFileId(u32),

    /// UAX chunk not found.
    #[error("UAX chunk (0x0700) not found")]
    ChunkNotFound,

    /// Chunk data too small for granny_file_info.
    #[error("Chunk data too small: {0} bytes (minimum: {1})")]
    ChunkTooSmall(usize, usize),

    /// Invalid FromFileName (must be "gr2ugx").
    #[error("Invalid FromFileName: expected 'gr2ugx', got '{0}'")]
    InvalidFromFileName(String),

    /// No animations in file.
    #[error("No animations in UAX file")]
    NoAnimations,

    /// Invalid pointer offset.
    #[error("Invalid pointer offset: 0x{0:X} (chunk size: 0x{1:X})")]
    InvalidPointerOffset(u64, usize),

    /// String read error.
    #[error("Failed to read null-terminated string at offset 0x{0:X}")]
    StringReadError(u64),
}
