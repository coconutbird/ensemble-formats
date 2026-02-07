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

mod error;
pub use error::{Error, Result};

mod vertex_element;
pub use vertex_element::VertexElementType;

mod univert_packer;
pub use univert_packer::{UnivertPacker, UnpackedVertex};

mod types;
pub use types::*;

mod ugx;
pub use ugx::UgxGeom;

mod gltf_export;
pub use gltf_export::{export_to_gltf, export_to_gltf_with_buffer_name, GltfExport, GltfExportOptions};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ugx_version_constant() {
        assert_eq!(UGX_VERSION, 0xECDA1015);
    }
}
