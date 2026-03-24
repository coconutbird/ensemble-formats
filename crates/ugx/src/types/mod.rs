//! UGX data types - materials, bones, sections, etc.
//!
//! # C++ Equivalents
//!
//! These Rust types correspond to the following C++ types from the original source:
//!
//! | Rust Type       | C++ Type (xgeom/ugxGeom.h)       |
//! |-----------------|----------------------------------|
//! | `Section`       | `BUGXGeom::BSection`             |
//! | `Bone`          | `BUGXGeom::BBone`                |
//! | `Material`      | `Unigeom::BMaterial`             |
//! | `Map`           | `Unigeom::BMap`                  |
//! | `MapType`       | `Unigeom::eMapType`              |
//! | `UnivertPacker` | `Unigeom::BUnpacker`             |
//! | `Matrix4x4`     | `BMatrix` (row-major 4x4)        |
//! | `AABB`          | `AABB` (xcore/math/vectorTypes.h)|
//! | `Sphere`        | `BSphere`                        |
//! | `AabbTree`      | `BAABBTree` (xgeom/aabbTree.h)   |
//! | `AabbTreeNode`  | `BAABBTreeNode`                  |
//!
//! Note: The DE (Definitive Edition) format differs from the original Xbox 360
//! source due to x64 pointer sizes and some additional fields.

pub mod aabb_tree;
pub mod bone;
pub mod material;
pub mod primitives;
pub mod section;

// Re-export all public types for convenient access.
pub use aabb_tree::{AabbTree, AabbTreeNode};
pub use bone::{Bone, GrannyBone, GrannyMesh};
pub use material::{Map, MapType, Material};
pub use primitives::{AABB, Keyframe, Sphere};
pub use section::Section;

/// A model accessory (C++ `Unigeom::BAccessory`).
///
/// Accessories group bones and reference object (section) indices.
/// Layout verified from IDA `BPackedArray_Accessories__unpack` at `0x1406d8660`.
#[derive(Debug, Clone, PartialEq)]
pub struct Accessory {
    /// First bone index in this accessory group.
    pub first_bone: i32,
    /// Number of bones in this accessory group.
    pub num_bones: i32,
    /// Section/object indices that belong to this accessory.
    pub object_indices: Vec<i32>,
}

// Re-export math types so downstream code using `types::Matrix4x4` still works.
pub use crate::math::{Matrix4x4, QForm};

use alloc::vec::Vec;

use crate::error::Result;
use crate::vertex::packer::UnpackedVertex;

/// UGX file version magic.
pub const UGX_VERSION: u32 = 0xECDA1015;

/// Parsed UGX geometry data.
#[derive(Debug, Clone)]
pub struct UgxGeom {
    /// Bounding sphere.
    pub bounding_sphere: Sphere,
    /// Axis-aligned bounding box.
    pub bounds: AABB,
    /// Materials.
    pub materials: Vec<Material>,
    /// Bones (from cached data chunk 0x700).
    pub bones: Vec<Bone>,
    /// Granny bone data (from granny chunk 0x703) - contains inverse world matrices.
    pub granny_bones: Vec<GrannyBone>,
    /// Granny mesh data (from granny chunk 0x703) - contains mesh names and bone bindings.
    pub granny_meshes: Vec<GrannyMesh>,
    /// Per-bone bounding boxes.
    pub bone_bounds: Vec<AABB>,
    /// Mesh sections.
    pub sections: Vec<Section>,
    /// Raw vertex buffer.
    pub vertex_buffer: Vec<u8>,
    /// Raw index buffer (u16 indices).
    pub index_buffer: Vec<u16>,
    /// Accessories (from BCachedData).
    pub accessories: Vec<Accessory>,
    /// Valid accessories subset (from BCachedData).
    pub valid_accessories: Vec<Accessory>,
    /// Is the entire mesh rigid (single bone)?
    pub rigid_only: bool,
    /// Rigid bone index (if rigid_only).
    pub rigid_bone_index: i32,
    /// Maximum number of instances for instanced rendering.
    pub max_instances: i16,
    /// Instance index multiplier (next power of two of max vertex count).
    pub instance_index_multiplier: i16,
    /// Large geometry bone index (i16::MAX when unused).
    pub large_geom_bone_index: i16,
    /// Are all sections rigid (multi-bone rigid)?
    pub all_sections_rigid: bool,
    /// Are all sections skinned?
    pub all_sections_skinned: bool,
    /// Use global bones?
    pub global_bones: bool,
    /// Parsed AABB tree (chunk 0x705, optional).
    ///
    /// Spatial acceleration structure used by the game engine for collision
    /// and ray-casting queries. When `None`, the chunk is omitted from the
    /// written ECF container.
    pub aabb_tree: Option<AabbTree>,
}

impl UgxGeom {
    /// Get unpacked vertices for a section.
    pub fn unpack_section_vertices(&self, section_idx: usize) -> Result<Vec<UnpackedVertex>> {
        let section = &self.sections[section_idx];
        let packer = &section.base_vert_packer;

        let vb_start = section.vb_offset as usize;
        let vb_end = vb_start + section.vb_bytes as usize;
        let vb_slice = &self.vertex_buffer[vb_start..vb_end];

        let mut vb_pos = 0usize;
        let mut vertices = Vec::with_capacity(section.num_verts as usize);

        for _ in 0..section.num_verts {
            let vert = packer.unpack_vertex(vb_slice, &mut vb_pos)?;
            vertices.push(vert);
        }

        Ok(vertices)
    }

    /// Get indices for a section.
    pub fn get_section_indices(&self, section_idx: usize) -> Vec<u16> {
        let section = &self.sections[section_idx];
        let start = section.ib_offset as usize;
        let count = section.num_tris as usize * 3;
        self.index_buffer[start..start + count].to_vec()
    }

    /// Get total vertex count across all sections.
    pub fn total_vertices(&self) -> usize {
        self.sections.iter().map(|s| s.num_verts as usize).sum()
    }

    /// Get total triangle count across all sections.
    pub fn total_triangles(&self) -> usize {
        self.sections.iter().map(|s| s.num_tris as usize).sum()
    }
}
