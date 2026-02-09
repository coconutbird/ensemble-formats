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
//! ## Example
//!
//! ```no_run
//! use uax::UaxAnimation;
//! use std::io::Cursor;
//!
//! let data = std::fs::read("animation.uax").unwrap();
//! let anim = UaxAnimation::from_reader(Cursor::new(&data)).unwrap();
//!
//! println!("Duration: {} seconds", anim.duration());
//! println!("Animation name: {:?}", anim.name());
//! ```

mod error;
pub use error::{Error, Result};

mod reader;
pub use reader::UaxAnimation;

/// UAX ECF file ID (from uaxdefs.h)
pub const UAX_FILE_ID: u32 = 0xAAC93747;

/// UAX chunk ID for animation data (from uaxdefs.h)  
pub const UAX_CHUNK_ID: u64 = 0x0700;

/// Expected FromFileName value in valid UAX files
pub const UAX_FROM_FILENAME: &str = "gr2ugx";

