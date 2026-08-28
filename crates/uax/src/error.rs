//! Error types for UAX parsing.

use alloc::string::String;
use thiserror::Error;

/// Result type for UAX operations.
pub type Result<T> = core::result::Result<T, Error>;

/// Error type for UAX parsing.
#[derive(Debug, Error)]
pub enum Error {
    /// ECF parsing error.
    #[error("ECF error: {0}")]
    Ecf(#[from] ecf::Error),

    /// Unexpected end of data.
    #[error("unexpected end of data")]
    UnexpectedEof,

    /// Invalid UAX file ID.
    #[error("Invalid UAX file ID: expected 0xAAC93747, got 0x{0:08X}")]
    InvalidFileId(u32),

    /// UAX chunk not found.
    #[error("UAX chunk (0x0700) not found")]
    ChunkNotFound,

    /// Chunk data too small for `granny_file_info`.
    #[error("Chunk data too small: {0} bytes (minimum: {1})")]
    ChunkTooSmall(usize, usize),

    /// Invalid `FromFileName` (must be "gr2ugx").
    #[error("Invalid FromFileName: expected 'gr2ugx', got '{0}'")]
    InvalidFromFileName(String),

    /// No animations in file.
    #[error("No animations in UAX file")]
    NoAnimations,

    /// The high-level API represents one animation, but the file contains more.
    #[error("UAX file contains {0} animations; exactly one is supported")]
    UnsupportedAnimationCount(i32),

    /// A signed Granny count is negative.
    #[error("Invalid negative {0}: {1}")]
    InvalidCount(&'static str, i32),

    /// Invalid pointer offset.
    #[error("Invalid pointer offset: 0x{0:X} (chunk size: 0x{1:X})")]
    InvalidPointerOffset(u64, usize),

    /// String read error.
    #[error("Failed to read null-terminated string at offset 0x{0:X}")]
    StringReadError(u64),

    /// A required pointer is null.
    #[error("Required UAX pointer is null: {0}")]
    NullPointer(&'static str),

    /// A byte range falls outside the Granny chunk.
    #[error(
        "Invalid {field} range: offset 0x{offset:X}, size 0x{size:X}, chunk size 0x{chunk_size:X}"
    )]
    InvalidRange {
        /// Field or structure being read.
        field: &'static str,
        /// Requested byte offset.
        offset: usize,
        /// Requested byte length.
        size: usize,
        /// Available chunk length.
        chunk_size: usize,
    },

    /// Animation track-group references disagree with the canonical root list.
    #[error("Animation track-group references do not match file_info TrackGroups")]
    TrackGroupReferenceMismatch,

    /// The semantic API cannot preserve a non-empty arbitrary Granny variant.
    #[error("Unsupported non-empty Granny extended-data variant in {0}")]
    UnsupportedExtendedData(&'static str),

    /// Curve format is not one of Granny's known formats.
    #[error("Unsupported Granny curve format: {0}")]
    UnsupportedCurveFormat(u8),

    /// The embedded curve type definition disagrees with the format byte.
    #[error("Curve format {format} expects type '{expected}', got '{actual}'")]
    InvalidCurveType {
        /// Curve format byte.
        format: u8,
        /// Expected Granny type name.
        expected: &'static str,
        /// Type name found in the embedded descriptor.
        actual: String,
    },

    /// Writer input pairs a format byte with the wrong typed payload.
    #[error("Curve format {format} does not match payload '{payload}'")]
    CurvePayloadMismatch {
        /// Curve format byte.
        format: u8,
        /// Supplied payload variant.
        payload: &'static str,
    },

    /// A string cannot be represented as a null-terminated Granny string.
    #[error("UAX {0} contains an embedded NUL byte")]
    EmbeddedNul(&'static str),

    /// A count, offset, or size cannot be represented by the file format or target.
    #[error("{0} is too large for the UAX format or this platform")]
    SizeOverflow(&'static str),
}
