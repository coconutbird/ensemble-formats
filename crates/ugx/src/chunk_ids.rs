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

/// AABB Tree chunk — spatial acceleration structure for collision/ray queries.
/// Streamed format: version + node_count + nodes (variable-length) + sentinel.
pub(crate) const ECF_AABB_TREE_CHUNK_ID: u64 = 0x00000705;

/// BCachedData header signature for Halo Wars: Definitive Edition (version 4).
pub(crate) const GEOM_HEADER_SIGNATURE_HW1: u32 = 0xC2340004;

/// BCachedData header signature for Halo Wars 2 (version 6).
///
/// Key differences from DE:
/// - Sections are 72 bytes (no UnivertPacker) instead of 152 bytes.
/// - Valid accessories are stored as 4-byte indices instead of 24-byte structs.
/// - AABB tree chunk (0x705) is absent from all HW2 UGX files.
pub(crate) const GEOM_HEADER_SIGNATURE_HW2: u32 = 0xC2340006;
