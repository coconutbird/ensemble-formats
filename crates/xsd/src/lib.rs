//! XSD (simulation terrain) format handling for Halo Wars.
//!
//! An XSD file stores the terrain that the game simulation uses for height and
//! surface queries. The data is in an ECF container.
//!
//! ## File structure
//!
//! XSD files contain these chunks:
//!
//! - `0x1111` - Header: version, grid dimensions and tile scales (32 bytes).
//! - `0x2222` - Height grid: half-float samples in 8 by 8 blocks.
//! - `0x8888` - Tile types: one byte for each data tile, row-major (optional).
//!
//! The version field also gives the byte order. Big-endian and little-endian
//! files are both valid.
//!
//! ## Example
//!
//! ```no_run
//! # fn example(bytes: &[u8]) -> xsd::Result<()> {
//! let terrain = xsd::Terrain::read(bytes)?;
//! let height = terrain.height_sample(0, 0);
//! # let _ = height;
//! # Ok(())
//! # }
//! ```

#![no_std]
extern crate alloc;

mod error;
pub use error::{Error, Result};

mod terrain;
pub use terrain::{ByteOrder, Terrain};

/// XSD file version.
pub const XSD_VERSION: i32 = 4;

/// Header chunk ID.
pub const CHUNK_HEADER: u64 = 0x1111;

/// Height grid chunk ID.
pub const CHUNK_HEIGHTS: u64 = 0x2222;

/// Tile-type grid chunk ID.
pub const CHUNK_TILE_TYPES: u64 = 0x8888;

/// Size of the header chunk data that the reader needs, in bytes.
pub const HEADER_SIZE: usize = 32;

/// Number of height samples on each axis of one block of the height grid.
pub const HEIGHT_BLOCK_AXIS: usize = 8;

/// Number of height samples in one block of the height grid.
pub const HEIGHT_BLOCK_SIZE: usize = HEIGHT_BLOCK_AXIS * HEIGHT_BLOCK_AXIS;
