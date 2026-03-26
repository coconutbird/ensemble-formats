//! ECF chunk IDs, format signatures, and binary layout constants.
//!
//! Centralises every magic number shared between reader and writer so that
//! version-specific logic can reference named constants instead of bare
//! literals scattered across the codebase.
//!
//! Chunk IDs are defined in the original source at `xgeom/ugxGeom.h`.

// ---------------------------------------------------------------------------
// ECF chunk IDs
// ---------------------------------------------------------------------------

/// BCachedData chunk — header, sections, bones, accessories.
/// All pointers in this chunk are stored as offsets for position independence.
pub(crate) const ECF_CACHED_DATA_CHUNK_ID: u64 = 0x00000700;

/// Index Buffer chunk — raw array of u16 triangle indices.
pub(crate) const ECF_IB_CHUNK_ID: u64 = 0x00000701;

/// Vertex Buffer chunk — packed vertex data (format defined by UnivertPacker).
pub(crate) const ECF_VB_CHUNK_ID: u64 = 0x00000702;

/// Granny chunk — skeleton with inverse world matrices for skinning.
/// This is the authoritative source for bone transforms in skinned meshes.
pub(crate) const ECF_GRANNY_CHUNK_ID: u64 = 0x00000703;

/// Material chunk — BBinaryDataTree document with material definitions.
/// Contains texture paths, blend modes, specular settings, etc.
pub(crate) const ECF_MATERIAL_CHUNK_ID: u64 = 0x00000704;

/// AABB Tree chunk — spatial acceleration structure for collision/ray queries.
/// Streamed format: version + node_count + nodes (variable-length) + sentinel.
pub(crate) const ECF_AABB_TREE_CHUNK_ID: u64 = 0x00000705;

// ---------------------------------------------------------------------------
// Format signatures
// ---------------------------------------------------------------------------

/// BCachedData header signature for Halo Wars: Definitive Edition (version 4).
pub(crate) const GEOM_HEADER_SIGNATURE_HW1: u32 = 0xC2340004;

/// BCachedData header signature for Halo Wars 2 (version 6).
///
/// Key differences from DE:
/// - Sections are 72 bytes (no UnivertPacker) instead of 152 bytes.
/// - Valid accessories are stored as 4-byte indices instead of 24-byte structs.
/// - AABB tree chunk (0x705) is absent from all HW2 UGX files.
pub(crate) const GEOM_HEADER_SIGNATURE_HW2: u32 = 0xC2340006;

// ---------------------------------------------------------------------------
// Binary layout sizes (bytes)
// ---------------------------------------------------------------------------

/// Section stride for HW1/DE: 40B fixed + 16B bone_remap + 84B packer + 12B flags.
pub(crate) const SECTION_STRIDE_HW1: usize = 152;

/// Section stride for HW2: 40B fixed + 8B flags + 8B unknown + 16B bone_remap.
pub(crate) const SECTION_STRIDE_HW2: usize = 72;

/// On-disk size of a serialised `UnivertPacker` (2 × u64 string offsets + 12 × u32 type fields).
#[allow(dead_code)]
pub(crate) const UNIVERT_PACKER_SIZE: usize = 84;

/// On-disk size of one `AccessoryRaw` struct (first_bone + num_bones + PackedArray).
#[allow(dead_code)]
pub(crate) const ACCESSORY_RAW_SIZE: usize = 24;

/// On-disk size of one `PackedBoneRaw` (name_offset + 4×4 matrix + parent_index + padding).
#[allow(dead_code)]
pub(crate) const PACKED_BONE_SIZE: usize = 80;

// ---------------------------------------------------------------------------
// Sentinel / null values
// ---------------------------------------------------------------------------

/// Sentinel value used for null 64-bit offsets in packed arrays and string fields.
pub(crate) const EMPTY_OFFSET_SENTINEL: u64 = 0xFFFF_FFFF_FFFF_FFFF;

/// Alternative 32-bit sentinel used in some older packed-array offset checks.
pub(crate) const EMPTY_OFFSET_SENTINEL_32: u32 = 0xFFFF_FFFF;
