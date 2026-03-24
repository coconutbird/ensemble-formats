//! Error and result types for PKG parsing.

use alloc::string::String;

use thiserror::Error;

/// PKG error type.
#[derive(Debug, Error)]
pub enum Error {
    /// Invalid PKG magic — expected `capack`.
    #[error("invalid PKG magic: expected \"capack\", found {found:?}")]
    InvalidMagic { found: [u8; 6] },

    /// Unexpected end of data (slice-based).
    #[error("unexpected end of data: need {need} bytes, have {have}")]
    UnexpectedEof { need: usize, have: usize },

    /// IO error (streaming).
    #[error("io error: {0}")]
    Io(nostdio::IoError),

    /// Unsupported PKG version.
    #[error("unsupported PKG version: {0}")]
    UnsupportedVersion(u64),

    /// A filename could not be decoded as UTF-8.
    #[error("invalid filename: {0}")]
    InvalidFilename(String),

    /// Filename length exceeds the engine limit (0x1FF bytes).
    #[error("filename too long: {0} bytes")]
    FilenameTooLong(u64),

    /// Entry index out of bounds.
    #[error("entry index {index} out of bounds (archive has {count} entries)")]
    EntryIndexOutOfBounds { index: usize, count: usize },

    /// Operation cancelled by progress callback.
    #[error("operation cancelled")]
    Cancelled,
}

impl From<nostdio::IoError> for Error {
    fn from(e: nostdio::IoError) -> Self {
        Self::Io(e)
    }
}

/// Result type for PKG operations.
pub type Result<T> = core::result::Result<T, Error>;
