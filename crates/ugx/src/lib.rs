//! UGX (Unit Graphics) model format parser for Halo Wars.
//!
//! UGX is the 3D model format used by Halo Wars. It contains:
//! - Mesh geometry (vertices and indices)
//! - Materials with texture references
//! - Bone hierarchy for skeletal animation
//! - Morph targets (keyframes)
//!
//! ## Example
//!
//! ```ignore
//! use ugx::Reader;
//!
//! let data = std::fs::read("model.ugx")?;
//! let geom = Reader::read(&data)?;
//!
//! println!("Sections: {}", geom.sections.len());
//! println!("Materials: {}", geom.materials.len());
//! println!("Bones: {}", geom.bones.len());
//! ```

#![no_std]
extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

mod bytes;
mod chunk_ids;
mod error;
pub mod math;
mod raw;
pub use error::{Error, Result};

pub mod vertex;
pub use vertex::{MAX_UV, UnivertPacker, UnpackedVertex, VertexElementType};

pub mod types;
pub use types::*;

mod reader;
mod rebuild;
pub use reader::Reader;

mod writer;
pub use writer::Writer;
