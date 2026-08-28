//! Shared on-disk raw structs and constants for UGX reader/writer.
//!
//! These `zerocopy` overlay types define the exact binary layout of structures
//! in the UGX file format. Both the reader (`FromBytes`) and writer (`IntoBytes`)
//! use these as the single source of truth for on-disk layout.
//!
//! Domain-type conversions (`From` impls) are co-located here so that the
//! mapping between on-disk layout and in-memory representation lives in one
//! place.

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use super::math::Matrix4x4;
use super::primitives::{AABB, Sphere};

/// Raw on-disk `BUGXGeomHeader` (64 bytes, little-endian).
///
/// Layout verified from IDA disassembly of `BUGXGeom::load`.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, Clone)]
#[repr(C)]
pub(crate) struct GeomHeaderRaw {
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

/// Raw on-disk `BPackedArray` header (16 bytes, little-endian).
///
/// Used throughout `BCachedData` to describe arrays with count + offset.
/// The offset is relative to the start of the chunk.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, Clone)]
#[repr(C)]
pub(crate) struct PackedArrayRaw {
    pub count: [u8; 4],
    pub _padding: [u8; 4],
    pub offset: [u8; 8],
}

/// Raw on-disk `BBone` (80 bytes, little-endian).
///
/// The `name_offset` is a packed string pointer (offset from chunk start).
/// The `model_to_bone` is a 4×4 row-major matrix stored as 16 × f32le.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, Clone)]
#[repr(C)]
pub(crate) struct PackedBoneRaw {
    pub name_offset: [u8; 8],
    pub model_to_bone: [[u8; 4]; 16],
    pub parent_index: [u8; 4],
    pub _padding: [u8; 4],
}

/// Raw on-disk `BAccessory` (24 bytes, little-endian).
///
/// Layout from IDA `BPackedArray_Accessories__unpack` at `0x1406d8660`:
/// - `+0x00` (4 bytes): i32 mFirstBone
/// - `+0x04` (4 bytes): i32 mNumBones
/// - `+0x08` (16 bytes): `BPackedArray`<int> mObjectIndices
///
/// The nested `mObjectIndices` packed array requires a recursive fixup:
/// the outer array is fixed up first (8-byte aligned), then each accessory's
/// inner `mObjectIndices` offset is fixed up (4-byte aligned for i32 elements).
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, Clone)]
#[repr(C)]
pub(crate) struct AccessoryRaw {
    pub first_bone: [u8; 4],
    pub num_bones: [u8; 4],
    pub object_indices: PackedArrayRaw,
}

/// Raw on-disk `BSection` fixed fields (40 bytes, little-endian).
///
/// This is the first 40 bytes of each 152-byte section record.
/// After this come the bone remap packed array (16 bytes) and
/// `UnivertPacker` data (84 bytes) + trailing flags (12 bytes).
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, Clone)]
#[repr(C)]
pub(crate) struct PackedSectionFixedRaw {
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
/// Raw on-disk `BVector3` (12 bytes, little-endian).
///
/// Used for bone-bounds min/max arrays in `BCachedData`.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, Clone, Copy)]
#[repr(C)]
pub(crate) struct BVector3Raw {
    pub x: [u8; 4],
    pub y: [u8; 4],
    pub z: [u8; 4],
}

impl From<&BVector3Raw> for [f32; 3] {
    fn from(raw: &BVector3Raw) -> Self {
        [
            f32::from_le_bytes(raw.x),
            f32::from_le_bytes(raw.y),
            f32::from_le_bytes(raw.z),
        ]
    }
}

impl From<[f32; 3]> for BVector3Raw {
    fn from(v: [f32; 3]) -> Self {
        Self {
            x: v[0].to_le_bytes(),
            y: v[1].to_le_bytes(),
            z: v[2].to_le_bytes(),
        }
    }
}

impl From<&GeomHeaderRaw> for Sphere {
    fn from(hdr: &GeomHeaderRaw) -> Self {
        Self {
            center: [
                f32::from_le_bytes(hdr.sphere_center[0]),
                f32::from_le_bytes(hdr.sphere_center[1]),
                f32::from_le_bytes(hdr.sphere_center[2]),
            ],
            radius: f32::from_le_bytes(hdr.sphere_radius),
        }
    }
}

impl From<&GeomHeaderRaw> for AABB {
    fn from(hdr: &GeomHeaderRaw) -> Self {
        Self {
            min: [
                f32::from_le_bytes(hdr.aabb_min[0]),
                f32::from_le_bytes(hdr.aabb_min[1]),
                f32::from_le_bytes(hdr.aabb_min[2]),
            ],
            max: [
                f32::from_le_bytes(hdr.aabb_max[0]),
                f32::from_le_bytes(hdr.aabb_max[1]),
                f32::from_le_bytes(hdr.aabb_max[2]),
            ],
        }
    }
}

/// Extract a 4×4 row-major matrix from a `PackedBoneRaw` model-to-bone field.
impl From<&PackedBoneRaw> for Matrix4x4 {
    fn from(raw: &PackedBoneRaw) -> Self {
        let mut rows = [[0.0f32; 4]; 4];
        for (i, row) in rows.iter_mut().enumerate() {
            for (j, col) in row.iter_mut().enumerate() {
                *col = f32::from_le_bytes(raw.model_to_bone[i * 4 + j]);
            }
        }
        Self { rows }
    }
}

// ---------------------------------------------------------------------------
// Granny raw structs (writer-side — populate fields, then write as bytes)
// ---------------------------------------------------------------------------

/// Raw on-disk Granny bone struct (164 bytes = 0xA4).
///
/// Used by the writer to emit bones in one `IntoBytes` write per bone
/// instead of individual cursor calls for each field.
///
/// String pointers (`name_ptr`) and extended-data pointers are patched
/// after the initial write via the string table and extended-data pass.
#[derive(IntoBytes, KnownLayout, Immutable, Debug, Clone)]
#[repr(C)]
pub(crate) struct GrannyBoneRaw {
    /// +0x00: Name string pointer (u64, patched by string table).
    pub name_ptr: [u8; 8],
    /// +0x08: Parent bone index (i32).
    pub parent_index: [u8; 4],
    /// +0x0C: Local transform flags.
    pub transform_flags: [u8; 4],
    /// +0x10: Local position (f32×3).
    pub position: [[u8; 4]; 3],
    /// +0x1C: Local orientation quaternion (f32×4, xyzw).
    pub orientation: [[u8; 4]; 4],
    /// +0x2C: Local scale/shear matrix (f32×9, 3×3 row-major).
    pub scale_shear: [[u8; 4]; 9],
    /// +0x50: Inverse world matrix (f32×16, 4×4 row-major).
    pub inverse_world: [[u8; 4]; 16],
    /// +0x90: LOD error (f32).
    pub lod_error: [u8; 4],
    /// +0x94: Extended data type pointer (u64, patched later).
    pub ext_type_ptr: [u8; 8],
    /// +0x9C: Extended data pointer (u64, patched later).
    pub ext_data_ptr: [u8; 8],
}

impl GrannyBoneRaw {
    /// Size assertion — must match `GRANNY_BONE_SIZE` (164 bytes).
    const _SIZE_CHECK: () = assert!(core::mem::size_of::<Self>() == 164);

    /// Create a zeroed bone (all fields zero/null).
    pub fn zeroed() -> Self {
        Self {
            name_ptr: [0; 8],
            parent_index: [0; 4],
            transform_flags: [0; 4],
            position: [[0; 4]; 3],
            orientation: [[0; 4]; 4],
            scale_shear: [[0; 4]; 9],
            inverse_world: [[0; 4]; 16],
            lod_error: [0; 4],
            ext_type_ptr: [0; 8],
            ext_data_ptr: [0; 8],
        }
    }
}

/// Raw on-disk Granny skeleton struct (40 bytes = 0x28).
///
/// Layout verified from the reader which reads at `skeleton_offs + 0x08`:
/// `bone_count(u32) + bones_ptr(u64) + lod_type(u32)`.
#[derive(IntoBytes, KnownLayout, Immutable, Debug, Clone)]
#[repr(C)]
pub(crate) struct GrannySkeletonRaw {
    /// +0x00: Name string pointer (u64, patched by string table).
    pub name_ptr: [u8; 8],
    /// +0x08: Bone count (u32).
    pub bone_count: [u8; 4],
    /// +0x0C: Bones array pointer (u64) — note: NOT naturally aligned.
    pub bones_ptr: [u8; 8],
    /// +0x14: LOD type (u32).
    pub lod_type: [u8; 4],
    /// +0x18: Remaining padding to 0x28 (16 bytes).
    pub _pad: [u8; 16],
}

impl GrannySkeletonRaw {
    const _SIZE_CHECK: () = assert!(core::mem::size_of::<Self>() == 0x28);
}
