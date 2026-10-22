//! Shared `no_std` helpers for binary format writers.
//!
//! The format crates use these helpers for layout tasks that do not depend
//! on one format. Each format crate maps the errors of this crate into its
//! own error type.

#![no_std]
extern crate alloc;

pub mod string_table;
pub use string_table::{StringTable, StringTableError};
