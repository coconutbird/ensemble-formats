//! Shared helpers for the command-line tools of this repository.
//!
//! The tools use these helpers to keep exit codes, size formatting, archive
//! extraction and shader output the same in each tool.

pub mod exit_code;
pub mod extract;
pub mod shader;
pub mod size;

pub use size::format_size;
