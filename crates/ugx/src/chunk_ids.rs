//! ECF chunk IDs and constants shared between reader and writer.
//!
//! These chunk IDs are defined in the original source at `xgeom/ugxGeom.h`.
//! The ECF container holds multiple chunks, each identified by a 64-bit ID.

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

// Note: Chunk 0x705 (AABB Tree) exists but is not currently parsed.
#[allow(dead_code)]
pub(crate) const ECF_AABB_TREE_CHUNK_ID: u64 = 0x00000705;

/// BCachedData header signature (verified from IDA: only 0xC2340004 is used).
pub(crate) const GEOM_HEADER_SIGNATURE: u32 = 0xC2340004;
