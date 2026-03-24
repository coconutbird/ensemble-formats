//! Shared on-disk raw structs and constants for UGX reader/writer.
//!
//! These `zerocopy` overlay types define the exact binary layout of structures
//! in the UGX file format. Both the reader (`FromBytes`) and writer (`IntoBytes`)
//! use these as the single source of truth for on-disk layout.

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

// ============================================================================
// BCachedData (chunk 0x700) raw structs
// ============================================================================

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

// ============================================================================
// Granny chunk (0x703) constants
// ============================================================================

/// Granny bone size in bytes (164 = 0xA4).
///
/// Per-bone layout:
/// - `+0x00` (8 bytes): u64 name string offset
/// - `+0x08` (4 bytes): i32 parent index
/// - `+0x0C` (4 bytes): u32 local transform flags
/// - `+0x10` (12 bytes): f32×3 local position
/// - `+0x1C` (16 bytes): f32×4 local orientation (quaternion xyzw)
/// - `+0x2C` (36 bytes): f32×9 local scale_shear (3×3 row-major)
/// - `+0x50` (64 bytes): f32×16 inverse world matrix (4×4 row-major)
/// - `+0x90` (4 bytes): f32 LOD error
/// - `+0x94` (16 bytes): extended data (zeros)
pub(crate) const GRANNY_BONE_SIZE: usize = 164;

/// Granny mesh struct size in bytes (76 = 0x4C).
///
/// Verified from IDA: BoneBindingCount at +0x30, BoneBindings at +0x34.
pub(crate) const GRANNY_MESH_SIZE: usize = 0x4C;

/// Granny bone_binding struct size in bytes (44 = 0x2C).
///
/// Verified from IDA: loop stride is 44 bytes in NewMeshBinding.
pub(crate) const GRANNY_BONE_BINDING_SIZE: usize = 0x2C;

/// Offset within a Granny bone struct where the inverse world matrix starts.
pub(crate) const GRANNY_BONE_INVERSE_WORLD_OFFSET: usize = 0x50;

/// Granny local transform flags.
pub(crate) const GRANNY_HAS_POSITION: u32 = 0x1;
pub(crate) const GRANNY_HAS_ORIENTATION: u32 = 0x2;
pub(crate) const GRANNY_HAS_SCALE_SHEAR: u32 = 0x4;
