//! Error types for UGX parsing.

use alloc::string::{FromUtf8Error, String};
use thiserror::Error;

/// UGX parsing errors.
#[derive(Error, Debug)]
pub enum Error {
    /// ECF error.
    #[error("ECF error: {0}")]
    Ecf(#[from] ecf::Error),

    /// BDT error.
    #[error("BDT error: {0}")]
    Bdt(#[from] bdt::Error),

    /// Invalid UGX version.
    #[error("Invalid UGX version: expected 0x{expected:08X}, got 0x{actual:08X}")]
    InvalidVersion { expected: u32, actual: u32 },

    /// Invalid geometry header signature.
    #[error(
        "Invalid geometry header signature: 0x{actual:08X} (expected HW1 0xC2340004 or HW2 0xC2340006)"
    )]
    InvalidSignature { actual: u32 },

    /// Missing ECF chunk.
    #[error("Missing required ECF chunk: {0}")]
    MissingChunk(&'static str),

    /// Invalid vertex element type.
    #[error("Invalid vertex element type: {0}")]
    InvalidVertexElementType(u8),

    /// Invalid pack order character.
    #[error("Invalid pack order character: '{0}'")]
    InvalidPackOrderChar(char),

    /// Unexpected end of data.
    #[error("Unexpected end of data while reading {context}")]
    UnexpectedEof { context: String },

    /// String too long.
    #[error("String too long: max {max} bytes, got {actual}")]
    StringTooLong { max: usize, actual: usize },

    /// Invalid UTF-8 string.
    #[error("Invalid UTF-8 string: {0}")]
    InvalidUtf8(#[from] FromUtf8Error),

    /// Unsupported format.
    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),

    /// Invalid Granny chunk — present but FromFileName is not "gr2ugx".
    #[error("Invalid Granny chunk: FromFileName is \"{actual}\", expected \"gr2ugx\"")]
    InvalidGrannyChunk { actual: String },

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] ecf::io::IoError),
}

/// Result type for UGX operations.
pub type Result<T> = core::result::Result<T, Error>;
