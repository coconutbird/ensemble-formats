//! UAX (Animation) format parser for Halo Wars Definitive Edition.
//!
//! UAX files contain animation data wrapped in an ECF container.
//! The animation data uses RAD Game Tools' Granny format with pointer offsets
//! that need rebasing to extract the actual animation metadata.
//!
//! ## Format Structure
//!
//! - ECF container with file ID `0xAAC93747`
//! - Single chunk with ID `0x0700` containing Granny file_info
//! - Granny data uses little-endian pointer offsets from chunk start
//!
//! ## Example (Read-Only)
//!
//! ```no_run
//! use uax::UaxAnimation;
//!
//! let data = std::fs::read("animation.uax").unwrap();
//! let anim = UaxAnimation::from_bytes(&data).unwrap();
//!
//! println!("Duration: {} seconds", anim.duration());
//! println!("Animation name: {:?}", anim.name());
//! ```
//!
//! ## Example (Read/Write with UaxFile)
//!
//! ```no_run
//! use uax::UaxFile;
//!
//! // Read UAX file
//! let data = std::fs::read("animation.uax").unwrap();
//! let mut uax = UaxFile::from_bytes(&data).unwrap();
//!
//! // Modify animation properties
//! let new_duration = uax.duration().unwrap() * 2.0;
//! uax.set_duration(new_duration).unwrap();
//!
//! // Write back to file
//! std::fs::write("modified.uax", uax.to_bytes().unwrap()).unwrap();
//! ```

mod error;
pub use error::{Error, Result};

mod reader;
pub use reader::UaxAnimation;

pub mod types;
pub use types::{GRANNY_HEADER_SIZE, POINTER_REBASE_OFFSET};

mod uax_file;
pub use uax_file::UaxFile;

/// UAX ECF file ID (from uaxdefs.h)
pub const UAX_FILE_ID: u32 = 0xAAC93747;

/// UAX chunk ID for animation data (from uaxdefs.h)
pub const UAX_CHUNK_ID: u64 = 0x0700;

/// Expected FromFileName value in valid UAX files
pub const UAX_FROM_FILENAME: &str = "gr2ugx";
