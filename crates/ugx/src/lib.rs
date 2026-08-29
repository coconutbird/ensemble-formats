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
//! # Reading a file with bad UGX signatures
//!
//! Strict parsing rejects the wrong ECF file ID or cached-data signature. To
//! inspect such a file while retaining checksum and engine-structure checks,
//! provide the layout explicitly:
//!
//! ```no_run
//! use ugx::{ReadOptions, UgxGeom, UgxVersion};
//!
//! let data = std::fs::read("model-with-bad-signature.ugx").expect("failed to read file");
//! let options = ReadOptions::accepting_bad_signatures(UgxVersion::Hw1);
//! let geom = UgxGeom::from_bytes_with_options(&data, options)
//!     .expect("failed to parse UGX with a v4 layout hint");
//! println!("Sections: {}", geom.sections.len());
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
//!     let indices = geom.get_section_indices(i).expect("indices unavailable");
//!     println!("Section {i}: {} verts, {} tris", vertices.len(), indices.len() / 3);
//! }
//! ```

#![no_std]
extern crate alloc;

#[cfg(any(feature = "std", test))]
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
pub use reader::{
    ReadOptions, Reader, detect_version, detect_version_with_options, read_materials,
    read_materials_with_options,
};

mod writer;
pub use writer::Writer;

pub(crate) fn checked_usize(value: u64, context: &'static str) -> Result<usize> {
    usize::try_from(value).map_err(|_| Error::SizeOverflow(context))
}

pub(crate) fn checked_usize_i32(value: i32, context: &'static str) -> Result<usize> {
    let value = u64::try_from(value).map_err(|_| Error::SizeOverflow(context))?;
    checked_usize(value, context)
}

pub(crate) fn checked_u32(value: usize, context: &'static str) -> Result<u32> {
    u32::try_from(value).map_err(|_| Error::SizeOverflow(context))
}

pub(crate) fn checked_i32(value: usize, context: &'static str) -> Result<i32> {
    i32::try_from(value).map_err(|_| Error::SizeOverflow(context))
}

pub(crate) fn advance_position(
    position: &mut usize,
    amount: u64,
    context: &'static str,
) -> Result<()> {
    let amount = checked_usize(amount, context)?;
    *position = position
        .checked_add(amount)
        .ok_or(Error::SizeOverflow(context))?;
    Ok(())
}
