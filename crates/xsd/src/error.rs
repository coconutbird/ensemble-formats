//! Error types for XSD parsing.

use thiserror::Error;

/// XSD parsing errors.
#[derive(Debug, Error)]
pub enum Error {
    /// The outer ECF container is not valid.
    #[error("invalid XSD container: {0}")]
    Container(#[from] ecf::Error),

    /// A required XSD chunk is absent.
    #[error("XSD is missing required chunk 0x{0:04X}")]
    MissingChunk(u64),

    /// The XSD header is shorter than the version-4 layout.
    #[error("XSD header is truncated: expected at least {expected} bytes, found {actual}")]
    TruncatedHeader {
        /// The minimum header size, in bytes.
        expected: usize,
        /// The size of the header data that the file has, in bytes.
        actual: usize,
    },

    /// The XSD header has an unsupported version or byte order.
    #[error("unsupported XSD version bytes {0:02X?}")]
    UnsupportedVersion([u8; 4]),

    /// The terrain grid dimensions, scales or samples are not usable.
    #[error("invalid XSD terrain dimensions: {0}")]
    InvalidDimensions(&'static str),

    /// The height chunk does not contain the cache-aligned grid.
    #[error("XSD height grid is truncated: expected {expected} bytes, found {actual}")]
    TruncatedHeights {
        /// The size that the height grid needs, in bytes.
        expected: usize,
        /// The size of the height chunk, in bytes.
        actual: usize,
    },

    /// The optional tile-type chunk does not contain the row-major data grid.
    #[error("XSD tile-type grid is truncated: expected {expected} bytes, found {actual}")]
    TruncatedTileTypes {
        /// The size that the tile-type grid needs, in bytes.
        expected: usize,
        /// The size of the tile-type chunk, in bytes.
        actual: usize,
    },
}

/// Result type for XSD operations.
pub type Result<T, E = Error> = core::result::Result<T, E>;
