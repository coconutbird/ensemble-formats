//! UAX (Animation) format parser for Halo Wars Definitive Edition / Halo Wars 2.
//!
//! UAX files contain animation data wrapped in an ECF container.
//! The animation chunk data is a Granny `file_info` structure in x64 native
//! layout — all internal pointers are 64-bit LE offsets from the chunk start.
//!
//! ## Format Structure
//!
//! - ECF container with file ID `0xAAC93747`
//! - Single chunk with ID `0x0700` containing Granny `file_info` directly
//! - No separate Granny section header — chunk data IS `file_info`
//! - x64 native pointer layout (same format for HW1 DE and HW2)
//!
//! ## Example (Read-Only)
//!
//! ```no_run
//! use uax::Reader;
//!
//! let data = std::fs::read("animation.uax").unwrap();
//! let anim = Reader::read(&data).unwrap();
//!
//! println!("Duration: {} seconds", anim.duration);
//! println!("Animation name: {:?}", anim.name);
//! println!("Track groups: {}", anim.track_groups.len());
//! for tg in &anim.track_groups {
//!     println!("  {} - {} transform tracks", tg.name.as_deref().unwrap_or("?"), tg.transform_tracks.len());
//! }
//! ```
//!
//! ## Example (Read/Write with `UaxFile`)
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

#![no_std]
extern crate alloc;

mod error;
pub use error::{Error, Result};

mod reader;
pub use reader::Reader;

mod writer;
pub use writer::Writer;

pub mod types;

mod file;
pub use file::UaxFile;

/// UAX ECF file ID (from uaxdefs.h)
pub const UAX_FILE_ID: u32 = 0xAAC9_3747;

/// UAX chunk ID for animation data (from uaxdefs.h)
pub const UAX_CHUNK_ID: u64 = 0x0700;

/// Expected `FromFileName` value in valid UAX files
pub const UAX_FROM_FILENAME: &str = "gr2ugx";
