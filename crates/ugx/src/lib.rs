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
//! use ugx::UgxGeom;
//!
//! let data = std::fs::read("model.ugx")?;
//! let geom = UgxGeom::read(&data)?;
//!
//! println!("Sections: {}", geom.sections.len());
//! println!("Materials: {}", geom.materials.len());
//! println!("Bones: {}", geom.bones.len());
//! ```

#![no_std]
extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

mod error;
pub use error::{Error, Result};

mod vertex_element;
pub use vertex_element::VertexElementType;

mod univert_packer;
pub use univert_packer::{UnivertPacker, UnpackedVertex};

mod types;
pub use types::*;

mod ugx;
pub use ugx::{GrannyBone, GrannyMesh, UgxGeom};

#[cfg(feature = "std")]
mod gltf_export;
#[cfg(feature = "std")]
pub use gltf_export::{
    GltfExport, GltfExportOptions, export_to_gltf, export_to_gltf_with_buffer_name,
};

#[cfg(feature = "std")]
mod gltf_import;
#[cfg(feature = "std")]
pub use gltf_import::{GltfImportOptions, import_from_gltf};

#[cfg(feature = "std")]
mod ugx_writer;
#[cfg(feature = "std")]
pub use ugx_writer::write_ugx;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ugx_version_constant() {
        assert_eq!(UGX_VERSION, 0xECDA1015);
    }
}
