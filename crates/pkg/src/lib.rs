//! Halo Wars 2 PKG packed-file archive format.
//!
//! PKG files are binary containers (`"capack"` magic) containing multiple
//! files indexed by FNV-1a 64-bit filename hashes. All integers are
//! little-endian.
//!
//! ## Quick start
//!
//! ```ignore
//! // Streaming from a file:
//! let file = std::io::BufReader::new(std::fs::File::open("fonts.pkg")?);
//! let mut pkg = pkg::Reader::new(file)?;
//!
//! // Or from a byte slice (zero-copy):
//! // let mut pkg = pkg::Reader::from_bytes(&bytes)?;
//!
//! println!("version: {}", pkg.version());
//! println!("entries: {}", pkg.entry_count());
//!
//! for entry in pkg.entries() {
//!     println!("  {} ({} bytes)", entry.filename, entry.data_size);
//! }
//!
//! if let Some(idx) = pkg.find("data\\fonts\\arial.fnt") {
//!     let data = pkg.read_entry(idx)?;
//!     // …
//! }
//! ```

#![no_std]
extern crate alloc;

mod error;
pub use error::{Error, Result};

mod header;
pub use header::*;

mod reader;
pub use reader::{PkgEntry, Reader, fnv1a_64};

mod writer;
pub use writer::Writer;
