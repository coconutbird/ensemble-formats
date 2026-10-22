//! Process exit codes for scripts.

/// The command succeeds.
pub const SUCCESS: i32 = 0;
/// The command fails with a general error.
pub const ERROR: i32 = 1;
/// The input file does not exist.
pub const FILE_NOT_FOUND: i32 = 2;
/// The input file does not have a valid format.
pub const INVALID_FORMAT: i32 = 3;
/// An I/O operation fails.
pub const IO_ERROR: i32 = 4;
/// Some operations succeed and some fail.
pub const PARTIAL_FAILURE: i32 = 5;
