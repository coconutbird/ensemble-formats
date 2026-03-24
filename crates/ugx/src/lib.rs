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

mod vertex_element;
pub use vertex_element::VertexElementType;

mod univert_packer;
pub use univert_packer::{MAX_UV, UnivertPacker, UnpackedVertex};

mod types;
pub use types::*;

mod reader;
pub use reader::Reader;

#[cfg(feature = "std")]
mod writer;
#[cfg(feature = "std")]
pub use writer::Writer;
