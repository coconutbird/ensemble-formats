//! PKG header structures (Halo Wars 2).
//!
//! PKG files are binary packed-file archives (`"capack"` magic) used by
//! Halo Wars 2. All multi-byte integers are **little-endian**.
//!
//! ## On-disk layout
//!
//! ```text
//! Header (22 bytes):
//!   [6 bytes]  magic: "capack" (ASCII)
//!   [8 bytes]  version: u64 LE (1 or 2)
//!   [8 bytes]  entry_count: u64 LE
//!
//! Entry (repeated entry_count times):
//!   [8 bytes]  filename_length: u64 LE (max 0x1FF)
//!   [N bytes]  filename: ASCII bytes (not null-terminated on disk)
//!   [8 bytes]  data_offset: u64 LE
//!   [8 bytes]  data_size: u64 LE
//!
//! Footer (version >= 2 only):
//!   [8 bytes]  alignment: u64 LE
//!
//! Data section follows, aligned to the `alignment` boundary.
//! ```

/// PKG magic: `"capack"` (6 ASCII bytes).
pub const MAGIC: &[u8; 6] = b"capack";

/// Maximum supported PKG version.
pub const MAX_VERSION: u64 = 2;

/// Maximum filename length (engine limit at 0x1FF).
pub const MAX_FILENAME_LEN: u64 = 0x1FF;

/// Size of the fixed portion of the header (magic + version + `entry_count`).
pub const HEADER_SIZE: usize = 6 + 8 + 8;
