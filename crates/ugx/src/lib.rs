//! UGX (Unit Graphics) model format parser for Halo Wars.
//!
//! UGX is the 3D model format used by Halo Wars (HW1 and HW2). It contains:
//! - Mesh geometry (vertices and indices)
//! - Materials with texture references
//! - Bone hierarchy for skeletal animation
//! - Morph targets (keyframes)
//! - AABB spatial tree (optional)
//!
//! UGX files are stored inside ECF (Ensemble Common Format) containers.
//! The main entry point is [`UgxGeom::from_bytes`], which parses all chunks.
//!
//! # Reading a UGX file
//!
//! ```no_run
//! use ugx::UgxGeom;
//!
//! let data = std::fs::read("model.ugx").expect("failed to read file");
//! let geom = UgxGeom::from_bytes(&data).expect("failed to parse UGX");
//!
//! println!("Sections: {}", geom.sections.len());
//! println!("Materials: {}", geom.materials.len());
//! println!("Bones: {}", geom.bones.len());
//! println!("Vertices: {}", geom.total_vertices());
//! println!("Triangles: {}", geom.total_triangles());
//! ```
//!
//! # Reading only materials (faster)
//!
//! ```no_run
//! let data = std::fs::read("model.ugx").expect("failed to read file");
//! let materials = ugx::read_materials(&data).expect("failed to parse materials");
//!
//! for mat in &materials {
//!     println!("Material: {}", mat.name);
//! }
//! ```
//!
//! # Unpacking vertices
//!
//! ```no_run
//! use ugx::UgxGeom;
//!
//! let data = std::fs::read("model.ugx").expect("failed to read file");
//! let geom = UgxGeom::from_bytes(&data).expect("failed to parse UGX");
//!
//! for (i, section) in geom.sections.iter().enumerate() {
//!     let vertices = geom.unpack_section_vertices(i).expect("unpack failed");
//!     let indices = geom.get_section_indices(i);
//!     println!("Section {i}: {} verts, {} tris", vertices.len(), indices.len() / 3);
//! }
//! ```

#![no_std]
extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

mod constants;
mod error;
pub use error::{Error, Result};

pub mod vertex;
pub use vertex::{MAX_UV, UnivertPacker, UnpackedVertex, VertexElementType};

pub mod types;
pub use types::*;

mod processing;
mod reader;
pub use reader::{Reader, ReaderOptions, read_materials};

mod writer;
pub use writer::Writer;
