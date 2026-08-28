//! Configurable XTT reader validation.

/// Validation controls for XTT parsing.
///
/// The default is strict and matches the retail loader's signature, checksum,
/// and required-header checks. Recovery tools can explicitly accept bad ECF
/// magic, file ID, and XTT version with [`Self::accepting_bad_signatures`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadOptions {
    /// Validate ECF header and chunk checksums.
    pub validate_checksums: bool,
    /// Validate ECF magic, XTT file ID, and XTT version.
    pub validate_signatures: bool,
    /// Require the singleton header and lossless recognized chunk shapes.
    pub validate_engine_requirements: bool,
}

impl ReadOptions {
    /// Strict validation matching the game loader.
    #[must_use]
    pub const fn strict() -> Self {
        Self {
            validate_checksums: true,
            validate_signatures: true,
            validate_engine_requirements: true,
        }
    }

    /// Strict validation except for ECF checksums.
    #[must_use]
    pub const fn unchecked_checksums() -> Self {
        Self {
            validate_checksums: false,
            ..Self::strict()
        }
    }

    /// Accept bad ECF/XTT signatures while retaining structural validation.
    #[must_use]
    pub const fn accepting_bad_signatures() -> Self {
        Self {
            validate_signatures: false,
            ..Self::strict()
        }
    }
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self::strict()
    }
}
