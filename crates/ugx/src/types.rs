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
//!
//! Note: The DE (Definitive Edition) format differs from the original Xbox 360
//! source due to x64 pointer sizes and some additional fields.

use alloc::string::String;
use alloc::vec::Vec;

use crate::bytes::read_f32_le;
use crate::error::{Error, Result};
use crate::univert_packer::{UnivertPacker, UnpackedVertex};

// Re-export math types so downstream code using `types::Matrix4x4` still works.
pub use crate::math::{Matrix4x4, QForm};

/// UGX file version magic.
pub const UGX_VERSION: u32 = 0xECDA1015;

/// Unigeom map types (13 types, matching Ensemble's eMapType enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MapType {
    Diffuse = 0,
    Normal = 1,
    Gloss = 2,
    Opacity = 3,
    XForm = 4,
    Emissive = 5,
    AO = 6,
    Env = 7,
    EnvMask = 8,
    EmXForm = 9,
    Distortion = 10,
    Highlight = 11,
    Modulate = 12,
}

impl MapType {
    pub const NUM_TYPES: usize = 13;

    pub const ALL: [MapType; 13] = [
        MapType::Diffuse,
        MapType::Normal,
        MapType::Gloss,
        MapType::Opacity,
        MapType::XForm,
        MapType::Emissive,
        MapType::AO,
        MapType::Env,
        MapType::EnvMask,
        MapType::EmXForm,
        MapType::Distortion,
        MapType::Highlight,
        MapType::Modulate,
    ];

    /// Get the node name used in the BBinaryDataTree document.
    /// Names are lowercase to match the packed BDT format in UGX material chunks.
    pub fn name(&self) -> &'static str {
        match self {
            MapType::Diffuse => "diffuse",
            MapType::Normal => "normal",
            MapType::Gloss => "gloss",
            MapType::Opacity => "opacity",
            MapType::XForm => "xform",
            MapType::Emissive => "emissive",
            MapType::AO => "ao",
            MapType::Env => "env",
            MapType::EnvMask => "envmask",
            MapType::EmXForm => "emxform",
            MapType::Distortion => "distortion",
            MapType::Highlight => "highlight",
            MapType::Modulate => "modulate",
        }
    }
}

/// A texture map reference (from Unigeom::BMap).
#[derive(Debug, Clone, Default)]
pub struct Map {
    /// Texture filename.
    pub name: String,
    /// UV channel index.
    pub channel: i16,
    /// Flags.
    pub flags: u8,
}

/// Material definition (from BBinaryDataTree packed document).
///
/// Materials are stored in UGX chunk 0x704 as a BBinaryDataTree document.
/// Each material has 13 map type slots, UVW velocities per map type,
/// and properties from a BNameValueMap (SpecPower, Flags, BlendType, Opacity).
#[derive(Debug, Clone)]
pub struct Material {
    /// Material name.
    pub name: String,
    /// Texture maps indexed by MapType (13 slots, each can have multiple maps).
    pub maps: [Vec<Map>; MapType::NUM_TYPES],
    /// UVW velocity per map type.
    pub uvw_velocity: [[f32; 3]; MapType::NUM_TYPES],
    /// Specular power (default: 10.0).
    pub spec_power: f32,
    /// Material flags (default: 0).
    pub flags: u32,
    /// Blend type (default: 0).
    pub blend_type: u8,
    /// Opacity (default: 1.0).
    pub opacity: f32,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            name: String::new(),
            maps: Default::default(),
            uvw_velocity: [[0.0; 3]; MapType::NUM_TYPES],
            spec_power: 10.0,
            flags: 0,
            blend_type: 0,
            opacity: 1.0,
        }
    }
}

/// Bone definition.
#[derive(Debug, Clone, Default)]
pub struct Bone {
    /// Bone name.
    pub name: String,
    /// Parent bone index (-1 for root).
    pub parent_index: i32,
    /// Model-to-bone transform (4x4 matrix in packed format).
    pub model_to_bone: Matrix4x4,
}

/// Axis-aligned bounding box.
#[derive(Debug, Clone, Default)]
pub struct AABB {
    /// Minimum corner [x, y, z].
    pub min: [f32; 3],
    /// Maximum corner [x, y, z].
    pub max: [f32; 3],
}

impl AABB {
    pub fn read(data: &[u8], pos: &mut usize) -> Result<Self> {
        let min = [
            read_f32_le(data, pos)?,
            read_f32_le(data, pos)?,
            read_f32_le(data, pos)?,
        ];
        let max = [
            read_f32_le(data, pos)?,
            read_f32_le(data, pos)?,
            read_f32_le(data, pos)?,
        ];
        Ok(Self { min, max })
    }
}

/// Bounding sphere.
#[derive(Debug, Clone, Default)]
pub struct Sphere {
    /// Center [x, y, z].
    pub center: [f32; 3],
    /// Radius.
    pub radius: f32,
}

impl Sphere {
    pub fn read(data: &[u8], pos: &mut usize) -> Result<Self> {
        let center = [
            read_f32_le(data, pos)?,
            read_f32_le(data, pos)?,
            read_f32_le(data, pos)?,
        ];
        let radius = read_f32_le(data, pos)?;
        Ok(Self { center, radius })
    }
}

/// Mesh section - a submesh with its own material and vertex format.
///
/// DE packed format is 152 bytes (0x98):
/// - +0x00: mMaterialIndex (i32)
/// - +0x04: mAccessoryIndex (i32)
/// - +0x08: mMaxBones (i32)
/// - +0x0C: mRigidBoneIndex (i32)
/// - +0x10: mIBOfs (i32, in indices not bytes)
/// - +0x14: mNumTris (i32)
/// - +0x18: mVBOfs (i32)
/// - +0x1C: mVBBytes (i32)
/// - +0x20: mVertSize (i32)
/// - +0x24: mNumVerts (i32)
/// - +0x28: BoneRemap packed array (16 bytes)
/// - +0x38: UnivertPacker (84 bytes)
/// - +0x8C: mRigidOnly (i32)
/// - +0x90: mGlobalBones (i32) - not in 2008 source!
/// - +0x94: mPadding (i32)
#[derive(Debug, Clone)]
pub struct Section {
    /// Material index.
    pub material_index: i32,
    /// Accessory index.
    pub accessory_index: i32,
    /// Maximum bones influencing this section.
    pub max_bones: i32,
    /// Rigid bone index (if rigid_only).
    pub rigid_bone_index: i32,
    /// Index buffer offset (in indices, not bytes).
    pub ib_offset: i32,
    /// Number of triangles.
    pub num_tris: i32,
    /// Vertex buffer offset (in bytes).
    pub vb_offset: i32,
    /// Vertex buffer size in bytes.
    pub vb_bytes: i32,
    /// Vertex stride in bytes.
    pub vert_size: i32,
    /// Number of vertices.
    pub num_verts: i32,
    /// Base vertex packer.
    pub base_vert_packer: UnivertPacker,
    /// Local-to-global bone remap table.
    /// Maps section-local bone indices to global skeleton indices.
    /// TODO: Entry size assumed u8 — may be u16/u32 for large skeletons. See ugx.rs.
    pub bone_remap: Vec<u8>,
    /// Is this section rigid (no skinning)?
    pub rigid_only: bool,
    /// Uses global bone indices (DE-specific field).
    pub global_bones: bool,
}

// Note: Section is read via UgxGeom::read_packed_section() in ugx.rs
// The packed DE format (152 bytes) is different from the original Xbox 360 format.

/// Morph target keyframe.
#[derive(Debug, Clone, Default)]
pub struct Keyframe {
    /// Time in seconds.
    pub time: f32,
    /// Vertex data.
    pub verts: Vec<u8>,
}

impl Keyframe {
    pub fn read(data: &[u8], pos: &mut usize) -> Result<Self> {
        let time = read_f32_le(data, pos)?;
        let end4 = *pos + 4;
        if end4 > data.len() {
            return Err(Error::UnexpectedEof {
                context: String::from("keyframe length"),
            });
        }
        let len = u32::from_le_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]])
            as usize;
        *pos = end4;
        let verts_end = *pos + len;
        if verts_end > data.len() {
            return Err(Error::UnexpectedEof {
                context: String::from("keyframe verts"),
            });
        }
        let verts = data[*pos..verts_end].to_vec();
        *pos = verts_end;
        Ok(Self { time, verts })
    }
}

// ============================================================================
// Core geometry types (UgxGeom, GrannyBone, GrannyMesh)
// ============================================================================

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
    /// Is the entire mesh rigid (single bone)?
    pub rigid_only: bool,
    /// Rigid bone index (if rigid_only).
    pub rigid_bone_index: i32,
    /// Are all sections rigid (multi-bone rigid)?
    pub all_sections_rigid: bool,
    /// Are all sections skinned?
    pub all_sections_skinned: bool,
    /// Use global bones?
    pub global_bones: bool,
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

/// Bone data from granny chunk (0x703).
/// This has the correct inverse world matrix for positioning bones.
#[derive(Debug, Clone, Default)]
pub struct GrannyBone {
    /// Bone name.
    pub name: String,
    /// Parent bone index (-1 for root).
    pub parent_index: i32,
    /// Inverse world matrix (4x4) - read from offset 80 in granny bone struct.
    /// To get world matrix: invert then transpose this matrix.
    pub inverse_world_matrix: Matrix4x4,
}

/// Mesh data from granny chunk (0x703).
/// Each mesh has a name and a list of bone bindings for skinning.
#[derive(Debug, Clone, Default)]
pub struct GrannyMesh {
    /// Mesh name (e.g., "marine_01", "optionalAssaultRifle").
    pub name: String,
    /// Bone names that this mesh is bound to (for skinning).
    /// Each entry is the name of a bone in the skeleton.
    pub bone_bindings: Vec<String>,
}
