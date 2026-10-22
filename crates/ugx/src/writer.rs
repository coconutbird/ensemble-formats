//! UGX file writer — supports both HW1 (v4) and HW2 (v6) formats.
//!
//! Serializes a `UgxGeom` into UGX binary format (ECF container).
//! Writes chunks 0x700 (cached data), 0x701 (index buffer), 0x702 (vertex buffer),
//! 0x703 (granny bones), 0x704 (materials), and optionally 0x705 (AABB tree).
//!
//! Version differences:
//! - HW1 (v4): Signature `0xC2340004`, 152-byte sections with `UnivertPacker`,
//!   i32 index valid accessories, includes AABB tree chunk (0x705).
//! - HW2 (v6): Signature `0xC2340006`, 72-byte sections (no `UnivertPacker`),
//!   i32 index valid accessories, no AABB tree chunk.

mod aabb_tree;
mod cached_data;
mod granny;
mod material;
pub(crate) mod string_table;
mod validation;

use alloc::vec::Vec;

use nostdio::WriteLe;

use crate::constants::{
    ECF_AABB_TREE_CHUNK_ID, ECF_CACHED_DATA_CHUNK_ID, ECF_GRANNY_CHUNK_ID, ECF_IB_CHUNK_ID,
    ECF_MATERIAL_CHUNK_ID, ECF_VB_CHUNK_ID, UGX_FILE_ID,
};
use crate::error::Result;
use crate::types::{UgxGeom, UgxVersion};

/// UGX file writer.
pub struct Writer;

impl Writer {
    /// Write a UGX geometry to a byte vector using the specified version format.
    ///
    /// # Errors
    ///
    /// Returns an error if the geometry is not representable in the selected
    /// game's layout, exceeds UGX field limits, or a chunk cannot be serialized.
    pub fn write(geom: &UgxGeom, version: UgxVersion) -> Result<Vec<u8>> {
        write_ugx(geom, version)
    }
}

impl UgxGeom {
    /// Serialize this geometry to UGX HW1 (v4) binary format.
    ///
    /// # Errors
    ///
    /// Returns an error if the geometry violates HW1 layout requirements,
    /// exceeds UGX field limits, or a chunk cannot be serialized.
    pub fn to_bytes_hw1(&self) -> Result<Vec<u8>> {
        write_ugx(self, UgxVersion::Hw1)
    }

    /// Serialize this geometry to UGX HW2 (v6) binary format.
    ///
    /// # Errors
    ///
    /// Returns an error if the geometry violates HW2 layout requirements,
    /// exceeds UGX field limits, or a chunk cannot be serialized.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        write_ugx(self, UgxVersion::Hw2)
    }
}

/// Write a UGX geometry to bytes (ECF container).
///
/// Chunk ordering matches the original engine output:
///   0x703 (granny) → 0x700 (cached) → 0x702 (VB) → 0x701 (IB) → 0x704 (materials) [→ 0x705 (AABB)]
///
/// VB and IB chunks use CONTIGUOUS resource flag and 32-byte alignment (align=5)
/// to match the original layout expected by the engine.
fn write_ugx(geom: &UgxGeom, version: UgxVersion) -> Result<Vec<u8>> {
    validation::validate_for_write(geom, version)?;
    let cached_data = cached_data::build_cached_data(geom, version)?;
    crate::checked_u32(cached_data.len(), "cached-data chunk size")?;
    let ib_data = build_index_buffer(geom)?;

    // The game validates this file ID in BGrannyModel::load.
    let mut ecf = ecf::Writer::new(UGX_FILE_ID);

    // Chunk order: granny → cached → VB → IB → materials [→ AABB]
    // This matches the original engine output ordering.

    // 0x703 — granny bones (first, no special flags)
    let granny_data = granny::build_granny_data(geom)?;
    crate::checked_u32(granny_data.len(), "Granny chunk size")?;
    ecf.add_chunk(ECF_GRANNY_CHUNK_ID, granny_data);

    // 0x700 — cached data (header, sections, bones, accessories)
    ecf.add_chunk(ECF_CACHED_DATA_CHUNK_ID, cached_data);

    // 0x702 — vertex buffer (CONTIGUOUS, 32-byte aligned)
    ecf.add_chunk_full(
        ECF_VB_CHUNK_ID,
        geom.vertex_buffer.clone(),
        5, // align=5 → 32-byte alignment
        ecf::resource_flags::CONTIGUOUS,
    );

    // 0x701 — index buffer (CONTIGUOUS, 32-byte aligned)
    ecf.add_chunk_full(
        ECF_IB_CHUNK_ID,
        ib_data,
        5, // align=5 → 32-byte alignment
        ecf::resource_flags::CONTIGUOUS,
    );

    // 0x704 — materials
    let mat_data = material::build_material_data(geom)?;
    crate::checked_u32(mat_data.len(), "material chunk size")?;
    ecf.add_chunk_with_alignment(ECF_MATERIAL_CHUNK_ID, mat_data, 2);

    // 0x705 — AABB tree (only for versions that include it)
    if version.has_aabb_tree()
        && let Some(ref tree) = geom.aabb_tree
    {
        let tree_data = aabb_tree::build_aabb_tree_data(tree)?;
        crate::checked_u32(tree_data.len(), "AABB-tree chunk size")?;
        ecf.add_chunk(ECF_AABB_TREE_CHUNK_ID, tree_data);
    }

    Ok(ecf.finalize()?)
}

/// Build the index buffer chunk (0x701).
///
/// The file stores the fully baked instanced index buffer. When
/// `max_instances > 1`, the buffer contains `max_instances` copies of
/// each section's indices, each offset by `instance_index_multiplier`
/// vertices per instance. The engine uploads this blob to the GPU as-is
/// (via `BUGXGeomData::loadIndexBuffer`) and never splits or replicates
/// indices at runtime. The `rebuild_instanced_index_buffer()` method in
/// `rebuild.rs` is responsible for producing this baked layout.
fn build_index_buffer(geom: &UgxGeom) -> Result<Vec<u8>> {
    let byte_count = geom
        .index_buffer
        .len()
        .checked_mul(core::mem::size_of::<u16>())
        .ok_or(crate::Error::SizeOverflow("index-buffer byte count"))?;
    let mut buf = Vec::with_capacity(byte_count);
    for &idx in &geom.index_buffer {
        buf.write_u16_le(idx)?;
    }

    Ok(buf)
}

#[cfg(test)]
mod tests;
