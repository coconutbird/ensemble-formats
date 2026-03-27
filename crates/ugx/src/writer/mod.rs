//! UGX file writer — supports both HW1/DE (v4) and HW2 (v6) formats.
//!
//! Serializes a `UgxGeom` into UGX binary format (ECF container).
//! Writes chunks 0x700 (cached data), 0x701 (index buffer), 0x702 (vertex buffer),
//! 0x703 (granny bones), 0x704 (materials), and optionally 0x705 (AABB tree).
//!
//! Version differences:
//! - HW1/DE (v4): Signature `0xC2340004`, 152-byte sections with UnivertPacker,
//!   i32 index valid accessories, includes AABB tree chunk (0x705).
//! - HW2 (v6): Signature `0xC2340006`, 72-byte sections (no UnivertPacker),
//!   i32 index valid accessories, no AABB tree chunk.

mod aabb_tree;
mod cached_data;
mod granny;
mod material;
pub(crate) mod string_table;

use alloc::vec::Vec;

use nostdio::WriteLe;

use crate::constants::*;
use crate::error::Result;
use crate::types::{UgxGeom, UgxVersion};

/// UGX file writer.
pub struct Writer;

impl Writer {
    /// Write a UGX geometry to a byte vector using the specified version format.
    pub fn write(geom: &UgxGeom, version: UgxVersion) -> Result<Vec<u8>> {
        write_ugx(geom, version)
    }
}

impl UgxGeom {
    /// Serialize this geometry to UGX HW1/DE (v4) binary format.
    pub fn to_bytes_hw1(&self) -> Result<Vec<u8>> {
        write_ugx(self, UgxVersion::Hw1)
    }

    /// Serialize this geometry to UGX HW2 (v6) binary format.
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
    let cached_data = cached_data::build_cached_data(geom, version)?;
    let ib_data = build_index_buffer(geom)?;

    // ECF file ID 0xAAC93746 is required for UGX files - the game validates this in BGrannyModel::load
    let mut ecf = ecf::Writer::new(0xAAC93746);

    // Chunk order: granny → cached → VB → IB → materials [→ AABB]
    // This matches the original engine output ordering.

    // 0x703 — granny bones (first, no special flags)
    if !geom.granny_bones.is_empty() {
        let granny_data = granny::build_granny_data(geom)?;
        ecf.add_chunk(ECF_GRANNY_CHUNK_ID, granny_data);
    }

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
    if !geom.materials.is_empty() {
        let mat_data = material::build_material_data(geom)?;
        ecf.add_chunk_with_alignment(ECF_MATERIAL_CHUNK_ID, mat_data, 2);
    }

    // 0x705 — AABB tree (only for versions that include it)
    if version.has_aabb_tree()
        && let Some(ref tree) = geom.aabb_tree
    {
        let tree_data = aabb_tree::build_aabb_tree_data(tree)?;
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
    let mut buf = Vec::with_capacity(geom.index_buffer.len() * 2);
    for &idx in &geom.index_buffer {
        buf.write_u16_le(idx)?;
    }

    Ok(buf)
}

#[cfg(test)]
mod tests;
