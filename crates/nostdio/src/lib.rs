//! `no_std`-compatible I/O traits and helpers.
//!
//! This crate provides [`Read`], [`Write`], and [`Seek`] traits that
//! work identically in `no_std` and `std` environments, along with
//! cursor types, endian-aware reading/writing extension traits, and
//! a progress-reporting interface.
//!
//! # Quick start
//!
//! ```
//! use nostdio::{SliceCursor, ReadLe};
//!
//! let data = [0x01, 0x00, 0x02, 0x00];
//! let mut cur = SliceCursor::new(&data);
//!
//! assert_eq!(cur.read_u16_le().unwrap(), 1);
//! assert_eq!(cur.read_u16_le().unwrap(), 2);
//! ```
//!
//! # Feature flags
//!
//! | Feature | Effect |
//! |---------|--------|
//! | `std`   | Re-exports [`Read`], [`Write`], [`Seek`], [`SeekFrom`] and [`IoError`] from `std::io`. |
//!
//! Without `std`, minimal replacement traits are provided so that crates
//! can be compiled for bare-metal targets.
//!
//! # Cursors
//!
//! * [`SliceCursor`] — read-only cursor over `&[u8]` (replaces
//!   `std::io::Cursor<&[u8]>`).
//! * [`MutCursor`] — read/write cursor over `&mut Vec<u8>` (replaces
//!   `std::io::Cursor<&mut Vec<u8>>`).
//!
//! # Endian-aware reading and writing
//!
//! The [`ReadLe`] / [`ReadBe`] and [`WriteLe`] / [`WriteBe`] extension
//! traits add typed little-endian and big-endian helper methods to any
//! [`Read`] or [`Write`] implementation.  They are blanket-implemented,
//! so importing the trait is all you need.
//!
//! When the byte order is only known at runtime, [`ReadEndian`] /
//! [`WriteEndian`] accept an [`Endian`] parameter and dispatch
//! accordingly (e.g. `cur.read_u32(Endian::Big)`).
//!
//! # Progress reporting
//!
//! The [`Progress`] trait provides a standard way for long-running
//! operations to report progress and support cancellation.  A blanket
//! impl lets closures `FnMut(u64, u64) -> bool` be used directly, and
//! [`NoProgress`] is a zero-cost no-op implementation.

#![no_std]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

mod cursor;
mod endian;
mod progress;
mod read;
mod traits;
mod write;

// Re-export everything at the crate root for a flat public API.
pub use cursor::*;
pub use endian::*;
pub use progress::*;
pub use read::*;
pub use traits::*;
pub use write::*;
