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
pub use bone::{
    Bone, GrannyBone, GrannyBoneBinding, GrannyLocalTransform, GrannyMemberType, GrannyMesh,
    GrannyTypeMember, GrannyVariant,
};
pub use material::{HoganMaterialData, Map, MapType, Material, ShaderPermutation};
pub use primitives::{AABB, Keyframe, Sphere};
pub use section::Section;

/// UGX format version, derived from the geometry header signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UgxVersion {
    /// Halo Wars: Definitive Edition (signature 0xC2340004).
    /// 152-byte sections with embedded UnivertPacker, i32 index valid accessories,
    /// includes AABB tree chunk (0x705).
    Hw1,
    /// Halo Wars 2 (signature 0xC2340006).
    /// 72-byte sections (no UnivertPacker), i32 index valid accessories,
    /// omits AABB tree chunk.
    Hw2,
}

impl UgxVersion {
    /// BCachedData header signature for this version.
    pub fn signature(self) -> u32 {
        match self {
            Self::Hw1 => crate::constants::GEOM_HEADER_SIGNATURE_HW1,
            Self::Hw2 => crate::constants::GEOM_HEADER_SIGNATURE_HW2,
        }
    }

    /// Per-section stride in the cached-data chunk (bytes).
    pub fn section_stride(self) -> usize {
        match self {
            Self::Hw1 => crate::constants::SECTION_STRIDE_HW1,
            Self::Hw2 => crate::constants::SECTION_STRIDE_HW2,
        }
    }

    /// Whether this version includes an AABB tree chunk (0x705).
    pub fn has_aabb_tree(self) -> bool {
        matches!(self, Self::Hw1)
    }

    /// Whether sections embed a `UnivertPacker` (84 bytes).
    pub fn has_embedded_packer(self) -> bool {
        matches!(self, Self::Hw1)
    }

    /// Whether valid accessories are stored as full 24-byte structs
    /// rather than 4-byte i32 indices.
    ///
    /// IDA analysis: `BUGXGeomData::readCachedData` uses
    /// `BPackedArray_Simple__unpack` (NOT `BPackedArray_Accessories__unpack`)
    /// for validAccessories in BOTH HW1 and HW2. They are always i32 indices.
    pub fn accessory_is_struct(self) -> bool {
        false
    }
}

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
    /// Skeleton LOD type from the Granny chunk (0x703). Preserved for round-tripping.
    pub skeleton_lod_type: u32,
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
    ///
    /// For DE sections, uses the embedded `UnivertPacker`.
    /// For HW2 sections (no packer), infers the vertex layout from `vert_size`.
    pub fn unpack_section_vertices(&self, section_idx: usize) -> Result<Vec<UnpackedVertex>> {
        let section = &self.sections[section_idx];
        let vb_start = section.vb_offset as usize;
        let vb_end = vb_start + section.vb_bytes as usize;
        let vb_slice = &self.vertex_buffer[vb_start..vb_end];

        let mut vb_pos = 0usize;
        let mut vertices = Vec::with_capacity(section.num_verts as usize);

        if let Some(ref packer) = section.base_vert_packer {
            // DE path — use the embedded UnivertPacker
            for _ in 0..section.num_verts {
                vertices.push(packer.unpack_vertex(vb_slice, &mut vb_pos)?);
            }
        } else {
            // HW2 path — infer layout from vert_size
            let vert_size = section.vert_size as usize;
            let is_skinned = !section.rigid_only && vert_size >= 28;

            for _ in 0..section.num_verts {
                let vert = unpack_hw2_vertex(vb_slice, &mut vb_pos, vert_size, is_skinned)?;
                vertices.push(vert);
            }
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

/// Unpack a single HW2 vertex whose layout is inferred from `vert_size`.
///
/// HW2 vertex formats (all sizes in bytes):
///
/// |  Size | Layout                                                              |
/// |------:|---------------------------------------------------------------------|
/// |     8 | pos(half4)                                                          |
/// |    12 | pos(half4) + uv(half2)                                              |
/// |    20 | pos(half4) + uv(half2) + normal(Dec3N) + tangent(Dec3N)             |
/// |    24 | pos(8) + uv(4) + normal(4) + tangent(4) + color(4)                 |
/// |    28 | rigid:  base20 + uv2(4) + color(4)                                 |
/// |       | skinned: base20 + indices(UByte4) + weights(UByte4N)                |
/// |    32 | rigid:  base20 + uv2(4) + color(4) + color2(4)                     |
/// |       | skinned: base20 + indices(4) + weights(4) + color(4)               |
/// |    36 | skinned: base20 + indices(4) + weights(4) + color(4) + color2(4)    |
fn unpack_hw2_vertex(
    data: &[u8],
    pos: &mut usize,
    vert_size: usize,
    is_skinned: bool,
) -> Result<UnpackedVertex> {
    use crate::vertex::element::VertexElementType;

    let start = *pos;
    let mut v = UnpackedVertex::default();

    // -- Position: always first 8 bytes (HalfFloat4) --
    let p = VertexElementType::HalfFloat4.unpack(data, pos)?;
    v.position = [p[0], p[1], p[2]];

    if vert_size <= 8 {
        *pos = start + vert_size;
        return Ok(v);
    }

    // -- UV0: next 4 bytes (HalfFloat2) --
    let uv = VertexElementType::HalfFloat2.unpack(data, pos)?;
    v.texcoords[0] = [uv[0], uv[1]];
    v.num_texcoords = 1;

    if vert_size <= 12 {
        *pos = start + vert_size;
        return Ok(v);
    }

    // -- Normal + Tangent: 4 bytes each (Dec3N) --
    let n = VertexElementType::Dec3N.unpack(data, pos)?;
    v.normal = [n[0], n[1], n[2]];

    let t = VertexElementType::Dec3N.unpack(data, pos)?;
    v.tangent = [t[0], t[1], t[2], t[3]];

    // We're now at 20 bytes consumed.
    let remaining = vert_size - 20;

    if remaining == 0 {
        return Ok(v);
    }

    if is_skinned {
        // Skinned: indices(4) + weights(4), then optional color(s)
        if remaining >= 8 {
            v.bone_indices = VertexElementType::UByte4.unpack_as_indices(data, pos)?;
            v.bone_weights = VertexElementType::UByte4N.unpack(data, pos)?;
        }
        if remaining >= 12 {
            v.diffuse = VertexElementType::D3DColor.unpack(data, pos)?;
        }
    } else {
        // Rigid: optional color/uv2
        if remaining >= 4 {
            // 24-byte: color; 28+: uv2 first
            if remaining >= 8 {
                let uv2 = VertexElementType::HalfFloat2.unpack(data, pos)?;
                v.texcoords[1] = [uv2[0], uv2[1]];
                v.num_texcoords = 2;
            }
            // color
            if remaining >= 4 + (if remaining >= 8 { 4 } else { 0 }) {
                v.diffuse = VertexElementType::D3DColor.unpack(data, pos)?;
            }
        }
    }

    // Ensure we advance exactly vert_size bytes regardless of what we consumed
    *pos = start + vert_size;

    Ok(v)
}
