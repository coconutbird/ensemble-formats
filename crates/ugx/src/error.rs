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
    InvalidVersion {
        /// Version that the reader supports.
        expected: u32,
        /// Version that the data contains.
        actual: u32,
    },

    /// Invalid geometry header signature.
    #[error(
        "Invalid geometry header signature: 0x{actual:08X} (expected HW1 0xC2340004 or HW2 0xC2340006)"
    )]
    InvalidSignature {
        /// Signature that the data contains.
        actual: u32,
    },

    /// Invalid UGX ECF file identifier.
    #[error("Invalid UGX file ID: expected 0x{expected:08X}, got 0x{actual:08X}")]
    InvalidFileId {
        /// File ID that the format requires.
        expected: u32,
        /// File ID that the data contains.
        actual: u32,
    },

    /// An unrecognized cached-data signature needs an explicit layout hint.
    #[error(
        "Cannot infer the UGX layout from signature 0x{actual:08X}; provide a version hint when signature validation is disabled"
    )]
    MissingVersionHint {
        /// Signature that the data contains.
        actual: u32,
    },

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
    UnexpectedEof {
        /// Item that the reader reads when the data ends.
        context: String,
    },

    /// String too long.
    #[error("String too long: max {max} bytes, got {actual}")]
    StringTooLong {
        /// Maximum length in bytes.
        max: usize,
        /// Length of the string in bytes.
        actual: usize,
    },

    /// Invalid UTF-8 string.
    #[error("Invalid UTF-8 string: {0}")]
    InvalidUtf8(#[from] FromUtf8Error),

    /// Unsupported format.
    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),

    /// Unexpected value in a reserved field.
    #[error(
        "Unexpected reserved field value in {context}: expected 0x{expected:08X}, got 0x{actual:08X}"
    )]
    UnexpectedReservedValue {
        /// Name of the reserved field.
        context: &'static str,
        /// Value that the format requires.
        expected: u32,
        /// Value that the data contains.
        actual: u32,
    },

    /// Invalid Granny chunk — present but `FromFileName` is not "gr2ugx".
    #[error("Invalid Granny chunk: FromFileName is \"{actual}\", expected \"gr2ugx\"")]
    InvalidGrannyChunk {
        /// `FromFileName` value that the chunk contains.
        actual: String,
    },

    /// The Granny chunk does not contain exactly one model.
    #[error("Invalid Granny chunk: expected exactly one model, got {actual}")]
    InvalidGrannyModelCount {
        /// Number of models in the chunk.
        actual: u32,
    },

    /// A required Granny pointer is null or outside the chunk.
    #[error("Invalid Granny chunk: {context} pointer is null or out of bounds")]
    InvalidGrannyPointer {
        /// Name of the pointer.
        context: &'static str,
    },

    /// A legacy material omits an attribute required by the game.
    #[error("Legacy material is missing required attribute {0}")]
    MissingMaterialAttribute(&'static str),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] nostdio::IoError),

    /// A size or offset cannot be represented on the target platform.
    #[error("{0} is too large for the target platform")]
    SizeOverflow(&'static str),
}

/// Result type for UGX operations.
pub type Result<T> = core::result::Result<T, Error>;
