//! Error types for LGT parsing.

use thiserror::Error;

/// LGT parsing errors.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum Error {
    /// The version prefix is not a supported Phoenix light-scene family.
    #[error("unsupported light-effect version 0x{0:08X}")]
    UnsupportedVersion(u32),

    /// A serialized count is negative or too large.
    #[error("invalid light-effect {field} count {value}")]
    InvalidCount {
        /// Record group or byte run that the reader decodes.
        field: &'static str,
        /// Serialized signed count.
        value: i32,
    },

    /// The stream ends before the field is complete.
    #[error("truncated light effect at byte {offset} while reading {field}")]
    Truncated {
        /// Byte offset at which the field starts.
        offset: usize,
        /// Field type that the reader decodes.
        field: &'static str,
    },

    /// The opening and closing version words are different.
    #[error(
        "light-effect version footer mismatch: opening 0x{opening:08X}, closing 0x{closing:08X}"
    )]
    VersionFooter {
        /// Opening version word.
        opening: u32,
        /// Closing version word.
        closing: u32,
    },
}

/// Result type for LGT operations.
pub type Result<T, E = Error> = core::result::Result<T, E>;
