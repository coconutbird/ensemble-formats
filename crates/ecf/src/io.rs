//! IO traits and helpers — re-exported from [`nostdio`].
//!
//! This module re-exports everything from the `nostdio` crate so that
//! existing code using `ecf::io::Read`, `ecf::io::SliceCursor`, etc.
//! continues to work unchanged.

pub use nostdio::*;
