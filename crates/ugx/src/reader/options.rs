//! Configurable UGX reader validation.

use crate::UgxVersion;

/// Validation controls for UGX parsing.
///
/// The default configuration is the strict, game-compatible mode. To inspect a file
/// with a bad ECF file ID or cached-data signature, set
/// [`Self::validate_signatures`] to `false`. An unknown cached-data signature
/// also needs [`Self::version_hint`] because the section layout cannot be
/// inferred safely from the corrupted value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadOptions {
    /// Validate ECF header and chunk checksums.
    pub validate_checksums: bool,
    /// Validate the UGX ECF file ID and cached-data layout signature.
    pub validate_signatures: bool,
    /// Require chunks and Granny invariants that the game loader requires.
    pub validate_engine_requirements: bool,
    /// Explicit layout to use when signature validation is disabled.
    ///
    /// A hint overrides the encoded cached-data signature in permissive mode.
    pub version_hint: Option<UgxVersion>,
}

impl ReadOptions {
    /// Strict validation matching the game loader's structural requirements.
    #[must_use]
    pub const fn strict() -> Self {
        Self {
            validate_checksums: true,
            validate_signatures: true,
            validate_engine_requirements: true,
            version_hint: None,
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

    /// Accept bad UGX identifiers/signatures and parse using `version`'s layout.
    ///
    /// Checksum and engine-structure validation remain enabled independently.
    #[must_use]
    pub const fn accepting_bad_signatures(version: UgxVersion) -> Self {
        Self {
            validate_signatures: false,
            version_hint: Some(version),
            ..Self::strict()
        }
    }
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self::strict()
    }
}
