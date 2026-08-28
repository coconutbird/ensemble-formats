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

/// `BCachedData` chunk — header, sections, bones, accessories.
/// All pointers in this chunk are stored as offsets for position independence.
pub(crate) const ECF_CACHED_DATA_CHUNK_ID: u64 = 0x0000_0700;

/// Index Buffer chunk — raw array of u16 triangle indices.
pub(crate) const ECF_IB_CHUNK_ID: u64 = 0x0000_0701;

/// Vertex Buffer chunk — packed vertex data (format defined by `UnivertPacker`).
pub(crate) const ECF_VB_CHUNK_ID: u64 = 0x0000_0702;

/// Granny chunk — skeleton with inverse world matrices for skinning.
/// This is the authoritative source for bone transforms in skinned meshes.
pub(crate) const ECF_GRANNY_CHUNK_ID: u64 = 0x0000_0703;

/// Material chunk — `BBinaryDataTree` document with material definitions.
/// Contains texture paths, blend modes, specular settings, etc.
pub(crate) const ECF_MATERIAL_CHUNK_ID: u64 = 0x0000_0704;

/// AABB Tree chunk — spatial acceleration structure for collision/ray queries.
/// Streamed format: version + `node_count` + nodes (variable-length) + sentinel.
pub(crate) const ECF_AABB_TREE_CHUNK_ID: u64 = 0x0000_0705;

// ---------------------------------------------------------------------------
// Format signatures
// ---------------------------------------------------------------------------

/// `BCachedData` header signature for Halo Wars: Definitive Edition (version 4).
pub(crate) const GEOM_HEADER_SIGNATURE_HW1: u32 = 0xC234_0004;

/// `BCachedData` header signature for Halo Wars 2 (version 6).
///
/// Key differences from HW1:
/// - Sections are 72 bytes (no `UnivertPacker`) instead of 152 bytes.
/// - Valid accessories are stored as 4-byte indices instead of 24-byte structs.
/// - AABB tree chunk (0x705) is absent from all HW2 UGX files.
pub(crate) const GEOM_HEADER_SIGNATURE_HW2: u32 = 0xC234_0006;

// ---------------------------------------------------------------------------
// Binary layout sizes (bytes)
// ---------------------------------------------------------------------------

/// Section stride for HW1: 40B fixed + 16B `bone_remap` + 84B packer + 12B flags.
pub(crate) const SECTION_STRIDE_HW1: usize = 152;

/// Section stride for HW2: 40B fixed + 4B `rigid_only` + 4B `lod_near` + 4B `lod_far` + 4B `lod_fade` + 16B `bone_remap`.
pub(crate) const SECTION_STRIDE_HW2: usize = 72;

// ---------------------------------------------------------------------------
// Sentinel / null values
// ---------------------------------------------------------------------------

/// Sentinel value used for null 64-bit offsets in packed arrays and string fields.
pub(crate) const EMPTY_OFFSET_SENTINEL: u64 = 0xFFFF_FFFF_FFFF_FFFF;

/// Alternative 32-bit sentinel used in some older packed-array offset checks.
pub(crate) const EMPTY_OFFSET_SENTINEL_32: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// UGX file ID
// ---------------------------------------------------------------------------

/// UGX ECF file-level version magic (written in ECF header `id` field).
pub const UGX_VERSION: u32 = 0xECDA_1015;

// ---------------------------------------------------------------------------
// AABB tree
// ---------------------------------------------------------------------------

/// AABB tree stream version magic (`BAABBTree::StreamVersion`).
pub const AABB_TREE_VERSION: u32 = 0x3344_0002;

/// Sentinel value meaning "no child" / "no parent" (NULL pointer offset).
pub const AABB_NULL_INDEX: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// Granny layout (binary struct sizes & field offsets)
// ---------------------------------------------------------------------------

/// Granny bone struct size in bytes (164 = 0xA4).
///
/// Layout:
/// - `+0x00` (12 bytes): `BPackedString` name (pointer + count)
///   - `+0x00` (8 bytes): u64 name offset
///   - `+0x08` (4 bytes): u32 parent bone index
/// - `+0x0C` (4 bytes): u32 local transform flags
/// - `+0x10` (12 bytes): f32×3 local position
/// - `+0x1C` (16 bytes): f32×4 local orientation (quaternion xyzw)
/// - `+0x2C` (36 bytes): f32×9 local `scale_shear` (3×3 row-major)
/// - `+0x50` (64 bytes): f32×16 inverse world matrix (4×4 row-major)
/// - `+0x90` (4 bytes): f32 LOD error
/// - `+0x94` (16 bytes): extended data (zeros)
pub(crate) const GRANNY_BONE_SIZE: usize = 164;

/// Granny mesh struct size in bytes (76 = 0x4C).
///
/// Verified from IDA: `BoneBindingCount` at +0x30, `BoneBindings` at +0x34.
pub(crate) const GRANNY_MESH_SIZE: usize = 0x4C;

/// Granny `bone_binding` struct size in bytes (44 = 0x2C).
///
/// Verified from IDA: loop stride is 44 bytes in `NewMeshBinding`.
pub(crate) const GRANNY_BONE_BINDING_SIZE: usize = 0x2C;

/// Offset within a Granny bone struct where the inverse world matrix starts.
pub(crate) const GRANNY_BONE_INVERSE_WORLD_OFFSET: usize = 0x50;

/// Granny local transform flag: bone has a position component.
pub(crate) const GRANNY_HAS_POSITION: u32 = 0x1;

/// Granny local transform flag: bone has an orientation component.
pub(crate) const GRANNY_HAS_ORIENTATION: u32 = 0x2;

/// Granny local transform flag: bone has a scale/shear component.
pub(crate) const GRANNY_HAS_SCALE_SHEAR: u32 = 0x4;

/// Granny type definition entry stride (44 bytes = 11 × u32).
///
/// Each `GrannyDataTypeDefinition` entry is 44 bytes on disk:
/// `MemberType(4) + Name(8) + ReferenceType(8) + ArrayWidth(4) + Extra(12) + Unused(8)`.
pub(crate) const GRANNY_TYPE_DEF_STRIDE: usize = 44;

/// Offset within a Granny bone struct where the `ExtendedData` variant ref starts.
///
/// This is a 16-byte `{type_def_ptr(u64), data_ptr(u64)}` pair at bone+0x94.
pub(crate) const GRANNY_BONE_EXTENDED_DATA_OFFSET: usize = 0x94;
