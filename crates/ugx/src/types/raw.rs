//! Shared on-disk raw structs and constants for UGX reader/writer.
//!
//! These `zerocopy` overlay types define the exact binary layout of structures
//! in the UGX file format. Both the reader (`FromBytes`) and writer (`IntoBytes`)
//! use these as the single source of truth for on-disk layout.

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// Raw on-disk BUGXGeomHeader (64 bytes, little-endian).
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

/// Raw on-disk BPackedArray header (16 bytes, little-endian).
///
/// Used throughout BCachedData to describe arrays with count + offset.
/// The offset is relative to the start of the chunk.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Debug, Clone)]
#[repr(C)]
pub(crate) struct PackedArrayRaw {
    pub count: [u8; 4],
    pub _padding: [u8; 4],
    pub offset: [u8; 8],
}

/// Raw on-disk BBone (80 bytes, little-endian).
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

/// Raw on-disk BAccessory (24 bytes, little-endian).
///
/// Layout from IDA `BPackedArray_Accessories__unpack` at `0x1406d8660`:
/// - `+0x00` (4 bytes): i32 mFirstBone
/// - `+0x04` (4 bytes): i32 mNumBones
/// - `+0x08` (16 bytes): BPackedArray<int> mObjectIndices
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

/// Raw on-disk BSection fixed fields (40 bytes, little-endian).
///
/// This is the first 40 bytes of each 152-byte section record.
/// After this come the bone remap packed array (16 bytes) and
/// UnivertPacker data (84 bytes) + trailing flags (12 bytes).
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
