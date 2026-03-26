//! Shared test infrastructure for ensemble-formats.
//!
//! Provides dotenv loading, game directory discovery, file finders,
//! and roundtrip helpers used across integration tests in multiple crates.
//!
//! # Usage
//!
//! Add `test-utils` as a `[dev-dependency]` and import the prelude:
//!
//! ```ignore
//! use test_utils::prelude::*;
//! ```

pub mod env;
pub mod files;

/// Convenient wildcard import for test files.
pub mod prelude {
    pub use crate::env::*;
    pub use crate::files::*;
}
