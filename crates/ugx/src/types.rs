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
use zerocopy::{FromBytes, Immutable, KnownLayout};

use crate::error::{Error, Result};
use crate::univert_packer::{UnivertPacker, UnpackedVertex};

// ============================================================================
// Zerocopy raw overlays
// ============================================================================

/// Raw on-disk BHeader (64 bytes, little-endian).
///
/// ```text
/// +0x00: uint32 signature          (0xC2340004)
/// +0x04: int32  rigid_bone_index
/// +0x08: float[3] sphere_center    (12 bytes)
/// +0x14: float  sphere_radius
/// +0x18: float[3] aabb_min         (12 bytes)
/// +0x24: float[3] aabb_max         (12 bytes)
/// +0x30: int16  max_instances
/// +0x32: int16  instance_index_multiplier
/// +0x34: int16  large_geom_bone_index
/// +0x36: uint8  all_sections_rigid
/// +0x37: uint8  global_bones
/// +0x38: uint8  all_sections_skinned
/// +0x39: uint8  rigid_only
/// +0x3A: uint16 padding
/// +0x3C: uint32 padding2
/// ```
#[derive(FromBytes, KnownLayout, Immutable, Debug)]
#[repr(C)]
pub struct GeomHeaderRaw {
    pub signature: [u8; 4],
    pub rigid_bone_index: [u8; 4],
    pub sphere_center: [[u8; 4]; 3],
    pub sphere_radius: [u8; 4],
    pub aabb_min: [[u8; 4]; 3],
    pub aabb_max: [[u8; 4]; 3],
    pub max_instances: [u8; 2],
    pub instance_index_multiplier: [u8; 2],
    pub large_geom_bone_index: [u8; 2],
    pub all_sections_rigid: u8,
    pub global_bones: u8,
    pub all_sections_skinned: u8,
    pub rigid_only: u8,
    pub _padding: [u8; 2],
    pub _padding2: [u8; 4],
}

/// Raw on-disk BPackedArray header (16 bytes, little-endian).
///
/// ```text
/// +0x00: uint32 count
/// +0x04: uint32 padding
/// +0x08: uint64 offset   (0xFFFFFFFFFFFFFFFF = NULL)
/// ```
#[derive(FromBytes, KnownLayout, Immutable, Debug)]
#[repr(C)]
pub struct PackedArrayRaw {
    pub count: [u8; 4],
    pub _padding: [u8; 4],
    pub offset: [u8; 8],
}

/// Raw on-disk BBone (80 bytes, little-endian).
///
/// ```text
/// +0x00: uint64 name_offset       - Offset to null-terminated name
/// +0x08: float[4][4] model_to_bone - 4x4 row-major matrix (64 bytes)
/// +0x48: int32  parent_index
/// +0x4C: uint32 padding
/// ```
#[derive(FromBytes, KnownLayout, Immutable, Debug)]
#[repr(C)]
pub struct PackedBoneRaw {
    pub name_offset: [u8; 8],
    pub model_to_bone: [[u8; 4]; 16],
    pub parent_index: [u8; 4],
    pub _padding: [u8; 4],
}

/// Raw on-disk BSection fixed fields (40 bytes, little-endian).
///
/// The first 10 × i32 fields of a packed section, before the nested
/// bone-remap packed array and UnivertPacker.
///
/// ```text
/// +0x00: int32 material_index
/// +0x04: int32 accessory_index
/// +0x08: int32 max_bones
/// +0x0C: int32 rigid_bone_index
/// +0x10: int32 ib_offset
/// +0x14: int32 num_tris
/// +0x18: int32 vb_offset
/// +0x1C: int32 vb_bytes
/// +0x20: int32 vert_size
/// +0x24: int32 num_verts
/// ```
#[derive(FromBytes, KnownLayout, Immutable, Debug)]
#[repr(C)]
pub struct PackedSectionFixedRaw {
    pub material_index: [u8; 4],
    pub accessory_index: [u8; 4],
    pub max_bones: [u8; 4],
    pub rigid_bone_index: [u8; 4],
    pub ib_offset: [u8; 4],
    pub num_tris: [u8; 4],
    pub vb_offset: [u8; 4],
    pub vb_bytes: [u8; 4],
    pub vert_size: [u8; 4],
    pub num_verts: [u8; 4],
}

/// Read a little-endian f32 from `data` at `*pos`, advancing `*pos` by 4.
#[inline]
fn read_f32_le(data: &[u8], pos: &mut usize) -> Result<f32> {
    let end = *pos + 4;
    if end > data.len() {
        return Err(Error::UnexpectedEof {
            context: String::from("f32"),
        });
    }
    let v = f32::from_le_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]]);
    *pos = end;
    Ok(v)
}

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

/// Quaternion + translation transform (used in non-packed format).
#[derive(Debug, Clone, Default)]
pub struct QForm {
    /// Quaternion [x, y, z, w].
    pub rotation: [f32; 4],
    /// Translation [x, y, z].
    pub translation: [f32; 3],
}

/// 4x4 transformation matrix (used in packed format).
/// Row-major order: row[0] = [m00, m01, m02, m03], etc.
#[derive(Debug, Clone)]
pub struct Matrix4x4 {
    /// Matrix rows.
    pub rows: [[f32; 4]; 4],
}

impl Default for Matrix4x4 {
    fn default() -> Self {
        Self {
            rows: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }
}

impl Matrix4x4 {
    /// Get translation from the matrix (row 3, columns 0-2).
    pub fn translation(&self) -> [f32; 3] {
        [self.rows[3][0], self.rows[3][1], self.rows[3][2]]
    }

    /// Convert to glTF column-major format (16-element array).
    /// glTF expects: [m00, m10, m20, m30, m01, m11, m21, m31, m02, m12, m22, m32, m03, m13, m23, m33]
    pub fn to_gltf_column_major(&self) -> [f32; 16] {
        let m = &self.rows;
        [
            m[0][0], m[1][0], m[2][0], m[3][0], // column 0
            m[0][1], m[1][1], m[2][1], m[3][1], // column 1
            m[0][2], m[1][2], m[2][2], m[3][2], // column 2
            m[0][3], m[1][3], m[2][3], m[3][3], // column 3
        ]
    }

    /// Create an identity matrix.
    pub fn identity() -> Self {
        Self {
            rows: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    /// Invert this 4x4 matrix. Returns None if the matrix is singular.
    pub fn inverse(&self) -> Option<Self> {
        let m = &self.rows;

        // Calculate cofactors for first row (used for determinant)
        let c00 = m[1][1] * (m[2][2] * m[3][3] - m[2][3] * m[3][2])
            - m[1][2] * (m[2][1] * m[3][3] - m[2][3] * m[3][1])
            + m[1][3] * (m[2][1] * m[3][2] - m[2][2] * m[3][1]);

        let c01 = -(m[1][0] * (m[2][2] * m[3][3] - m[2][3] * m[3][2])
            - m[1][2] * (m[2][0] * m[3][3] - m[2][3] * m[3][0])
            + m[1][3] * (m[2][0] * m[3][2] - m[2][2] * m[3][0]));

        let c02 = m[1][0] * (m[2][1] * m[3][3] - m[2][3] * m[3][1])
            - m[1][1] * (m[2][0] * m[3][3] - m[2][3] * m[3][0])
            + m[1][3] * (m[2][0] * m[3][1] - m[2][1] * m[3][0]);

        let c03 = -(m[1][0] * (m[2][1] * m[3][2] - m[2][2] * m[3][1])
            - m[1][1] * (m[2][0] * m[3][2] - m[2][2] * m[3][0])
            + m[1][2] * (m[2][0] * m[3][1] - m[2][1] * m[3][0]));

        let det = m[0][0] * c00 + m[0][1] * c01 + m[0][2] * c02 + m[0][3] * c03;

        if det.abs() < 1e-10 {
            return None;
        }

        let inv_det = 1.0 / det;

        // Calculate remaining cofactors
        let c10 = -(m[0][1] * (m[2][2] * m[3][3] - m[2][3] * m[3][2])
            - m[0][2] * (m[2][1] * m[3][3] - m[2][3] * m[3][1])
            + m[0][3] * (m[2][1] * m[3][2] - m[2][2] * m[3][1]));

        let c11 = m[0][0] * (m[2][2] * m[3][3] - m[2][3] * m[3][2])
            - m[0][2] * (m[2][0] * m[3][3] - m[2][3] * m[3][0])
            + m[0][3] * (m[2][0] * m[3][2] - m[2][2] * m[3][0]);

        let c12 = -(m[0][0] * (m[2][1] * m[3][3] - m[2][3] * m[3][1])
            - m[0][1] * (m[2][0] * m[3][3] - m[2][3] * m[3][0])
            + m[0][3] * (m[2][0] * m[3][1] - m[2][1] * m[3][0]));

        let c13 = m[0][0] * (m[2][1] * m[3][2] - m[2][2] * m[3][1])
            - m[0][1] * (m[2][0] * m[3][2] - m[2][2] * m[3][0])
            + m[0][2] * (m[2][0] * m[3][1] - m[2][1] * m[3][0]);

        let c20 = m[0][1] * (m[1][2] * m[3][3] - m[1][3] * m[3][2])
            - m[0][2] * (m[1][1] * m[3][3] - m[1][3] * m[3][1])
            + m[0][3] * (m[1][1] * m[3][2] - m[1][2] * m[3][1]);

        let c21 = -(m[0][0] * (m[1][2] * m[3][3] - m[1][3] * m[3][2])
            - m[0][2] * (m[1][0] * m[3][3] - m[1][3] * m[3][0])
            + m[0][3] * (m[1][0] * m[3][2] - m[1][2] * m[3][0]));

        let c22 = m[0][0] * (m[1][1] * m[3][3] - m[1][3] * m[3][1])
            - m[0][1] * (m[1][0] * m[3][3] - m[1][3] * m[3][0])
            + m[0][3] * (m[1][0] * m[3][1] - m[1][1] * m[3][0]);

        let c23 = -(m[0][0] * (m[1][1] * m[3][2] - m[1][2] * m[3][1])
            - m[0][1] * (m[1][0] * m[3][2] - m[1][2] * m[3][0])
            + m[0][2] * (m[1][0] * m[3][1] - m[1][1] * m[3][0]));

        let c30 = -(m[0][1] * (m[1][2] * m[2][3] - m[1][3] * m[2][2])
            - m[0][2] * (m[1][1] * m[2][3] - m[1][3] * m[2][1])
            + m[0][3] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]));

        let c31 = m[0][0] * (m[1][2] * m[2][3] - m[1][3] * m[2][2])
            - m[0][2] * (m[1][0] * m[2][3] - m[1][3] * m[2][0])
            + m[0][3] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]);

        let c32 = -(m[0][0] * (m[1][1] * m[2][3] - m[1][3] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][3] - m[1][3] * m[2][0])
            + m[0][3] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]));

        let c33 = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);

        Some(Self {
            rows: [
                [c00 * inv_det, c10 * inv_det, c20 * inv_det, c30 * inv_det],
                [c01 * inv_det, c11 * inv_det, c21 * inv_det, c31 * inv_det],
                [c02 * inv_det, c12 * inv_det, c22 * inv_det, c32 * inv_det],
                [c03 * inv_det, c13 * inv_det, c23 * inv_det, c33 * inv_det],
            ],
        })
    }

    /// Multiply two matrices: self * other
    pub fn multiply(&self, other: &Self) -> Self {
        let a = &self.rows;
        let b = &other.rows;
        let mut result = [[0.0f32; 4]; 4];

        for i in 0..4 {
            for j in 0..4 {
                result[i][j] =
                    a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j] + a[i][3] * b[3][j];
            }
        }

        Self { rows: result }
    }

    /// Transpose the matrix (swap rows and columns).
    pub fn transpose(&self) -> Self {
        let m = &self.rows;
        Self {
            rows: [
                [m[0][0], m[1][0], m[2][0], m[3][0]],
                [m[0][1], m[1][1], m[2][1], m[3][1]],
                [m[0][2], m[1][2], m[2][2], m[3][2]],
                [m[0][3], m[1][3], m[2][3], m[3][3]],
            ],
        }
    }

    /// Extract rotation as a quaternion [x, y, z, w] from the 3x3 rotation part.
    /// Uses the Shepperd method for numerical stability.
    pub fn to_quaternion(&self) -> [f32; 4] {
        let m = &self.rows;
        // Extract 3x3 rotation matrix (upper-left)
        let m00 = m[0][0];
        let m11 = m[1][1];
        let m22 = m[2][2];
        let trace = m00 + m11 + m22;

        let (x, y, z, w) = if trace > 0.0 {
            let s = (trace + 1.0).sqrt() * 2.0;
            let w = 0.25 * s;
            let x = (m[2][1] - m[1][2]) / s;
            let y = (m[0][2] - m[2][0]) / s;
            let z = (m[1][0] - m[0][1]) / s;
            (x, y, z, w)
        } else if m00 > m11 && m00 > m22 {
            let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
            let w = (m[2][1] - m[1][2]) / s;
            let x = 0.25 * s;
            let y = (m[0][1] + m[1][0]) / s;
            let z = (m[0][2] + m[2][0]) / s;
            (x, y, z, w)
        } else if m11 > m22 {
            let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
            let w = (m[0][2] - m[2][0]) / s;
            let x = (m[0][1] + m[1][0]) / s;
            let y = 0.25 * s;
            let z = (m[1][2] + m[2][1]) / s;
            (x, y, z, w)
        } else {
            let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
            let w = (m[1][0] - m[0][1]) / s;
            let x = (m[0][2] + m[2][0]) / s;
            let y = (m[1][2] + m[2][1]) / s;
            let z = 0.25 * s;
            (x, y, z, w)
        };

        // Normalize
        let len = (x * x + y * y + z * z + w * w).sqrt();
        if len > 1e-10 {
            [x / len, y / len, z / len, w / len]
        } else {
            [0.0, 0.0, 0.0, 1.0] // Identity quaternion
        }
    }
}

impl QForm {
    pub fn read(data: &[u8], pos: &mut usize) -> Result<Self> {
        let rotation = [
            read_f32_le(data, pos)?,
            read_f32_le(data, pos)?,
            read_f32_le(data, pos)?,
            read_f32_le(data, pos)?,
        ];
        let translation = [
            read_f32_le(data, pos)?,
            read_f32_le(data, pos)?,
            read_f32_le(data, pos)?,
        ];
        Ok(Self {
            rotation,
            translation,
        })
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
