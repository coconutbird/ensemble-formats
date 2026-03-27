//! Cached data chunk (0x700) builder.
//!
//! Supports both HW1/DE (v4) and HW2 (v6) formats:
//!
//! ## HW1/DE Layout (v4):
//! - Signature `0xC2340004`, 152-byte sections with embedded UnivertPacker,
//!   valid accessories as i32 indices, includes AABB tree.
//!
//! ## HW2 Layout (v6):
//! - Signature `0xC2340006`, 72-byte sections (no UnivertPacker),
//!   valid accessories as 4-byte i32 indices, no AABB tree.

use alloc::vec::Vec;

use ecf::io::{MutCursor, Seek, SeekFrom, Write, WriteLe};
use zerocopy::IntoBytes;

use crate::constants::EMPTY_OFFSET_SENTINEL;
use crate::error::Result;
use crate::raw::{AccessoryRaw, GeomHeaderRaw, PackedArrayRaw};
use crate::types::{Accessory, UgxGeom, UgxVersion};

/// Positions of the six packed-array header placeholders written after the
/// geometry header.  Each position points to the start of the 16-byte
/// `PackedArrayRaw` that will be patched during the fixup pass.
struct ArrayHeaderPositions {
    sections: usize,
    bones: usize,
    accessories: usize,
    valid_accessories: usize,
    bounds_low: usize,
    bounds_high: usize,
}

/// Build the cached data chunk (0x700).
///
/// Orchestrates writing each section of the cached-data blob and then
/// patches the packed-array headers with final offsets/counts.
pub(super) fn build_cached_data(geom: &UgxGeom, version: UgxVersion) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut cursor = MutCursor::new(&mut buf);
    let mut strings = super::string_table::StringTable::new();

    // 1. Geometry header (64 bytes).
    write_header(&mut cursor, geom, version)?;

    // 2. Six packed-array header placeholders (6 × 16 bytes).
    let hdr_pos = write_array_header_placeholders(&mut cursor)?;

    // 3. Section data + deferred bone-remap writes.
    let (sections_offset, num_sections) = write_sections(&mut cursor, &mut strings, geom, version)?;

    // 4. Bone data.
    let (bones_offset, num_bones) = write_bones(&mut cursor, &mut strings, geom)?;

    // 5. Accessories (full 24-byte structs, used by both versions).
    let (acc_offset, num_acc, acc_fixups) =
        write_accessory_structs(&mut cursor, &geom.accessories)?;

    // 6. Valid accessories (version-dependent encoding).
    let (valid_acc_offset, num_valid_acc, valid_acc_fixups) =
        write_valid_accessories(&mut cursor, geom, version)?;

    // 7. Bone bounds (min[] then max[]).
    let (bounds_low_offset, bounds_high_offset, num_bounds) = write_bone_bounds(&mut cursor, geom)?;

    // 8. Accessory inner index data (must come after all structs).
    write_accessory_indices(&mut cursor, &geom.accessories, &acc_fixups)?;
    if !valid_acc_fixups.is_empty() {
        write_accessory_indices(&mut cursor, &geom.valid_accessories, &valid_acc_fixups)?;
    }

    // 9. String table.
    pad_to_alignment(&mut cursor, 2)?;
    let _ = cursor; // release borrow so StringTable can write directly
    strings.write_aligned(&mut buf, 2);

    // 10. Fixup pass — patch the six packed-array headers with real offsets.
    let mut cursor = MutCursor::new(&mut buf);
    fixup_packed_array_header(&mut cursor, hdr_pos.sections, num_sections, sections_offset)?;
    fixup_packed_array_header(&mut cursor, hdr_pos.bones, num_bones, bones_offset)?;
    fixup_packed_array_header(&mut cursor, hdr_pos.accessories, num_acc, acc_offset)?;
    fixup_packed_array_header(
        &mut cursor,
        hdr_pos.valid_accessories,
        num_valid_acc,
        valid_acc_offset,
    )?;
    fixup_packed_array_header(
        &mut cursor,
        hdr_pos.bounds_low,
        num_bounds,
        bounds_low_offset,
    )?;
    fixup_packed_array_header(
        &mut cursor,
        hdr_pos.bounds_high,
        num_bounds,
        bounds_high_offset,
    )?;

    Ok(buf)
}

// ---------------------------------------------------------------------------
// Sub-functions — each writes one logical section of the cached-data blob.
// ---------------------------------------------------------------------------

/// Write the 64-byte `GeomHeaderRaw`.
fn write_header(cursor: &mut MutCursor<'_>, geom: &UgxGeom, version: UgxVersion) -> Result<()> {
    let header = GeomHeaderRaw {
        signature: version.signature().to_le_bytes(),
        rigid_bone_index: geom.rigid_bone_index.to_le_bytes(),
        sphere_center: [
            geom.bounding_sphere.center[0].to_le_bytes(),
            geom.bounding_sphere.center[1].to_le_bytes(),
            geom.bounding_sphere.center[2].to_le_bytes(),
        ],
        sphere_radius: geom.bounding_sphere.radius.to_le_bytes(),
        aabb_min: [
            geom.bounds.min[0].to_le_bytes(),
            geom.bounds.min[1].to_le_bytes(),
            geom.bounds.min[2].to_le_bytes(),
        ],
        aabb_max: [
            geom.bounds.max[0].to_le_bytes(),
            geom.bounds.max[1].to_le_bytes(),
            geom.bounds.max[2].to_le_bytes(),
        ],
        max_instances: geom.max_instances.to_le_bytes(),
        instance_index_multiplier: geom.instance_index_multiplier.to_le_bytes(),
        large_geom_bone_index: geom.large_geom_bone_index.to_le_bytes(),
        all_sections_rigid: if geom.all_sections_rigid { 1 } else { 0 },
        global_bones: if geom.global_bones { 1 } else { 0 },
        all_sections_skinned: if geom.all_sections_skinned { 1 } else { 0 },
        rigid_only: if geom.rigid_only { 1 } else { 0 },
        _padding: [0; 2],
        _padding2: [0; 4],
    };
    cursor.write_all(header.as_bytes())?;

    Ok(())
}

/// Write six zero-filled `PackedArrayRaw` placeholders and return their
/// stream positions for the later fixup pass.
fn write_array_header_placeholders(cursor: &mut MutCursor<'_>) -> Result<ArrayHeaderPositions> {
    let empty = PackedArrayRaw {
        count: [0; 4],
        _padding: [0; 4],
        offset: [0; 8],
    };
    let sections = cursor.stream_position()? as usize;
    cursor.write_all(empty.as_bytes())?;
    let bones = cursor.stream_position()? as usize;
    cursor.write_all(empty.as_bytes())?;
    let accessories = cursor.stream_position()? as usize;
    cursor.write_all(empty.as_bytes())?;
    let valid_accessories = cursor.stream_position()? as usize;
    cursor.write_all(empty.as_bytes())?;
    let bounds_low = cursor.stream_position()? as usize;
    cursor.write_all(empty.as_bytes())?;
    let bounds_high = cursor.stream_position()? as usize;
    cursor.write_all(empty.as_bytes())?;
    Ok(ArrayHeaderPositions {
        sections,
        bones,
        accessories,
        valid_accessories,
        bounds_low,
        bounds_high,
    })
}

/// Write all section structs (version-branched layout) followed by deferred
/// bone-remap data.  Returns `(offset, count)` for the sections packed array.
fn write_sections(
    cursor: &mut MutCursor<'_>,
    strings: &mut super::string_table::StringTable,
    geom: &UgxGeom,
    version: UgxVersion,
) -> Result<(u64, u32)> {
    let sections_offset = cursor.stream_position()?;
    let num_sections = geom.sections.len() as u32;
    let mut bone_remap_fixups: Vec<(usize, usize)> = Vec::new();

    // Compute instanced ib_offsets: when max_instances > 1, the index buffer
    // contains repeated copies of each section's indices, so the offset for
    // section N = sum of (num_tris * 3 * max_instances) for sections 0..N.
    let max_inst = geom.max_instances.max(1) as i32;
    let mut instanced_ib_offsets = Vec::with_capacity(geom.sections.len());
    let mut running_ib_offset = 0i32;
    for section in &geom.sections {
        instanced_ib_offsets.push(running_ib_offset);
        running_ib_offset += section.num_tris * 3 * max_inst;
    }

    for (section_idx, section) in geom.sections.iter().enumerate() {
        // Use instanced ib_offset when max_instances > 1, otherwise use as-is.
        let ib_offset = if max_inst > 1 {
            instanced_ib_offsets[section_idx]
        } else {
            section.ib_offset
        };

        // Fixed section fields (40 bytes, shared by both versions).
        let fixed = crate::raw::PackedSectionFixedRaw {
            material_index: section.material_index.to_le_bytes(),
            accessory_index: section.accessory_index.to_le_bytes(),
            max_bones: section.max_bones.to_le_bytes(),
            rigid_bone_index: section.rigid_bone_index.to_le_bytes(),
            ib_offset: ib_offset.to_le_bytes(),
            num_tris: section.num_tris.to_le_bytes(),
            vb_offset: section.vb_offset.to_le_bytes(),
            vb_bytes: section.vb_bytes.to_le_bytes(),
            vert_size: section.vert_size.to_le_bytes(),
            num_verts: section.num_verts.to_le_bytes(),
        };
        cursor.write_all(fixed.as_bytes())?;

        // Version-specific trailing fields.
        write_section_tail(
            cursor,
            strings,
            section,
            section_idx,
            version,
            &mut bone_remap_fixups,
        )?;
    }

    // Deferred bone-remap data — written after all section structs so that
    // the section array is contiguous.
    for &(header_pos, section_idx) in &bone_remap_fixups {
        pad_to_alignment(cursor, 4)?;
        let remap_offset = cursor.stream_position()?;
        cursor.write_all(&geom.sections[section_idx].bone_remap)?;
        let saved = cursor.stream_position()?;
        cursor.seek(SeekFrom::Start((header_pos + 8) as u64))?;
        cursor.write_u64_le(remap_offset)?;
        cursor.seek(SeekFrom::Start(saved))?;
    }

    Ok((sections_offset, num_sections))
}

/// Write the version-specific tail of a single section struct.
///
/// HW1/DE (112 bytes after fixed): bone_remap(16) + UnivertPacker(84) + flags(12).
/// HW2 (32 bytes after fixed): flags(16) + bone_remap(16).
fn write_section_tail(
    cursor: &mut MutCursor<'_>,
    strings: &mut super::string_table::StringTable,
    section: &crate::types::Section,
    section_idx: usize,
    version: UgxVersion,
    bone_remap_fixups: &mut Vec<(usize, usize)>,
) -> Result<()> {
    match version {
        UgxVersion::Hw1 => {
            // BoneRemap packed array (16 bytes)
            let bone_remap_header_pos = cursor.stream_position()? as usize;
            let bone_remap_arr = PackedArrayRaw {
                count: (section.bone_remap.len() as u32).to_le_bytes(),
                _padding: [0; 4],
                offset: if section.bone_remap.is_empty() {
                    EMPTY_OFFSET_SENTINEL.to_le_bytes()
                } else {
                    0u64.to_le_bytes()
                },
            };
            cursor.write_all(bone_remap_arr.as_bytes())?;
            if !section.bone_remap.is_empty() {
                bone_remap_fixups.push((bone_remap_header_pos, section_idx));
            }

            // UnivertPacker (84 bytes)
            write_packed_univert_packer(cursor, strings, section.base_vert_packer.as_ref())?;

            // Trailing flags (12 bytes)
            cursor.write_i32_le(if section.rigid_only { 1 } else { 0 })?;
            cursor.write_i32_le(if section.global_bones { 1 } else { 0 })?;
            cursor.write_i32_le(0)?; // padding
        }
        UgxVersion::Hw2 => {
            // Flags first, then bone remap
            cursor.write_i32_le(if section.rigid_only { 1 } else { 0 })?;
            cursor.write_i32_le(if section.global_bones { 1 } else { 0 })?;
            cursor.write_i32_le(0)?; // unknown
            cursor.write_i32_le(0)?; // unknown2

            // BoneRemap packed array (16 bytes)
            let bone_remap_header_pos = cursor.stream_position()? as usize;
            let bone_remap_arr = PackedArrayRaw {
                count: (section.bone_remap.len() as u32).to_le_bytes(),
                _padding: [0; 4],
                offset: if section.bone_remap.is_empty() {
                    EMPTY_OFFSET_SENTINEL.to_le_bytes()
                } else {
                    0u64.to_le_bytes()
                },
            };
            cursor.write_all(bone_remap_arr.as_bytes())?;
            if !section.bone_remap.is_empty() {
                bone_remap_fixups.push((bone_remap_header_pos, section_idx));
            }
        }
    }
    Ok(())
}

/// Write packed bone structs.  Returns `(offset, count)` for the bones
/// packed-array header.
fn write_bones(
    cursor: &mut MutCursor<'_>,
    strings: &mut super::string_table::StringTable,
    geom: &UgxGeom,
) -> Result<(u64, u32)> {
    pad_to_alignment(cursor, 8)?;
    let offset = cursor.stream_position()?;
    let count = geom.bones.len() as u32;

    for bone in &geom.bones {
        let name_fixup_pos = cursor.stream_position()?;

        let mut mtb = [[0u8; 4]; 16];
        for (r, row) in bone.model_to_bone.rows.iter().enumerate() {
            for (c, &val) in row.iter().enumerate() {
                mtb[r * 4 + c] = val.to_le_bytes();
            }
        }

        let packed = crate::raw::PackedBoneRaw {
            name_offset: 0u64.to_le_bytes(), // placeholder
            model_to_bone: mtb,
            parent_index: bone.parent_index.to_le_bytes(),
            _padding: [0; 4],
        };
        cursor.write_all(packed.as_bytes())?;
        strings.add(name_fixup_pos as usize, bone.name.clone());
    }

    Ok((offset, count))
}

/// Write valid accessories as i32 indices into the accessories array.
///
/// IDA analysis confirms both HW1/DE and HW2 use `BPackedArray_Simple__unpack`
/// for validAccessories — they are always flat i32 index arrays.
///
/// Returns `(offset, count, inner_fixups)` (fixups always empty).
fn write_valid_accessories(
    cursor: &mut MutCursor<'_>,
    geom: &UgxGeom,
    _version: UgxVersion,
) -> Result<(u64, u32, Vec<usize>)> {
    if geom.valid_accessories.is_empty() {
        return Ok((0, 0, Vec::new()));
    }

    pad_to_alignment(cursor, 4)?;
    let offset = cursor.stream_position()?;
    let count = geom.valid_accessories.len() as u32;
    for valid_acc in &geom.valid_accessories {
        let idx = geom
            .accessories
            .iter()
            .position(|a| a == valid_acc)
            .map(|i| i as i32)
            .unwrap_or(-1);
        cursor.write_i32_le(idx)?;
    }
    Ok((offset, count, Vec::new()))
}

/// Write bone-bounds min/max arrays.  Returns
/// `(bounds_low_offset, bounds_high_offset, count)`.
fn write_bone_bounds(cursor: &mut MutCursor<'_>, geom: &UgxGeom) -> Result<(u64, u64, u32)> {
    let count = geom.bone_bounds.len() as u32;

    pad_to_alignment(cursor, 4)?;
    let low_offset = cursor.stream_position()?;
    for bb in &geom.bone_bounds {
        for &v in &bb.min {
            cursor.write_f32_le(v)?;
        }
    }

    pad_to_alignment(cursor, 4)?;
    let high_offset = cursor.stream_position()?;
    for bb in &geom.bone_bounds {
        for &v in &bb.max {
            cursor.write_f32_le(v)?;
        }
    }

    Ok((low_offset, high_offset, count))
}

/// Pad the cursor position to the given byte alignment.
fn pad_to_alignment<W: Write + Seek>(writer: &mut W, alignment: u64) -> Result<()> {
    let pos = writer.stream_position()?;
    let remainder = pos % alignment;
    if remainder != 0 {
        let padding = alignment - remainder;
        for _ in 0..padding {
            writer.write_u8(0)?;
        }
    }
    Ok(())
}

/// Fix up a packed array header at the given position using zerocopy.
fn fixup_packed_array_header(
    cursor: &mut MutCursor<'_>,
    header_pos: usize,
    count: u32,
    offset: u64,
) -> Result<()> {
    let final_offset = if count == 0 {
        EMPTY_OFFSET_SENTINEL
    } else {
        offset
    };
    let arr = PackedArrayRaw {
        count: count.to_le_bytes(),
        _padding: [0; 4],
        offset: final_offset.to_le_bytes(),
    };
    cursor.seek(SeekFrom::Start(header_pos as u64))?;
    cursor.write_all(arr.as_bytes())?;
    Ok(())
}

/// Write accessory structs (24 bytes each) and return (offset, count, inner_fixups).
///
/// Each accessory's `mObjectIndices` packed array offset is written as a placeholder (0)
/// and recorded in `inner_fixups` for later patching by `write_accessory_indices`.
fn write_accessory_structs(
    cursor: &mut MutCursor<'_>,
    accessories: &[Accessory],
) -> Result<(u64, u32, Vec<usize>)> {
    let count = accessories.len() as u32;
    if count == 0 {
        return Ok((0, 0, Vec::new()));
    }

    pad_to_alignment(cursor, 8)?;
    let offset = cursor.stream_position()?;
    let mut inner_fixups = Vec::with_capacity(accessories.len());

    for acc in accessories {
        // Position of the inner packed array's offset field (for later fixup)
        let inner_offset_pos = cursor.stream_position()? as usize
            + core::mem::size_of::<[u8; 4]>() * 2  // first_bone + num_bones
            + core::mem::size_of::<[u8; 4]>()       // inner count
            + core::mem::size_of::<[u8; 4]>(); // inner padding

        let raw = AccessoryRaw {
            first_bone: acc.first_bone.to_le_bytes(),
            num_bones: acc.num_bones.to_le_bytes(),
            object_indices: PackedArrayRaw {
                count: (acc.object_indices.len() as u32).to_le_bytes(),
                _padding: [0; 4],
                offset: if acc.object_indices.is_empty() {
                    EMPTY_OFFSET_SENTINEL.to_le_bytes()
                } else {
                    0u64.to_le_bytes() // placeholder
                },
            },
        };
        cursor.write_all(raw.as_bytes())?;

        if !acc.object_indices.is_empty() {
            inner_fixups.push(inner_offset_pos);
        }
    }

    Ok((offset, count, inner_fixups))
}

/// Write the actual i32 index data for each accessory and fix up the inner offsets.
fn write_accessory_indices(
    cursor: &mut MutCursor<'_>,
    accessories: &[Accessory],
    inner_fixups: &[usize],
) -> Result<()> {
    let mut fixup_idx = 0;
    for acc in accessories {
        if acc.object_indices.is_empty() {
            continue;
        }

        pad_to_alignment(cursor, 4)?;
        let data_offset = cursor.stream_position()?;

        for &idx in &acc.object_indices {
            cursor.write_i32_le(idx)?;
        }

        // Patch the inner packed array offset
        let saved = cursor.stream_position()?;
        cursor.seek(SeekFrom::Start(inner_fixups[fixup_idx] as u64))?;
        cursor.write_u64_le(data_offset)?;
        cursor.seek(SeekFrom::Start(saved))?;

        fixup_idx += 1;
    }
    Ok(())
}

/// Write a packed UnivertPacker (84 bytes on-disk) for DE sections.
///
/// Layout: 2 string offset fields (u64 each, fixed up via StringTable),
/// then 12 u32 type fields (pos, basis, basis_scale, tangent, normal,
/// uv[0..8], indices, weights, diffuse, index).
fn write_packed_univert_packer(
    cursor: &mut MutCursor<'_>,
    strings: &mut super::string_table::StringTable,
    packer: Option<&crate::vertex::packer::UnivertPacker>,
) -> Result<()> {
    let packer = match packer {
        Some(p) => p,
        None => {
            // Write 84 bytes of zeros if no packer
            for _ in 0..84 {
                cursor.write_u8(0)?;
            }
            return Ok(());
        }
    };

    // pack_order string offset (placeholder, fixed up by StringTable)
    let pack_order_pos = cursor.stream_position()? as usize;
    cursor.write_u64_le(EMPTY_OFFSET_SENTINEL)?;
    if !packer.pack_order.is_empty() {
        strings.add(pack_order_pos, packer.pack_order.clone());
    }

    // decl_order string offset (placeholder, fixed up by StringTable)
    let decl_order_pos = cursor.stream_position()? as usize;
    cursor.write_u64_le(EMPTY_OFFSET_SENTINEL)?;
    if !packer.decl_order.is_empty() {
        strings.add(decl_order_pos, packer.decl_order.clone());
    }

    // 12 type fields as u32
    cursor.write_u32_le(packer.pos_type as u32)?;
    cursor.write_u32_le(packer.basis_type as u32)?;
    cursor.write_u32_le(packer.basis_scale_type as u32)?;
    cursor.write_u32_le(packer.tangent_type as u32)?;
    cursor.write_u32_le(packer.normal_type as u32)?;
    for uv in &packer.uv_types {
        cursor.write_u32_le(*uv as u32)?;
    }
    cursor.write_u32_le(packer.indices_type as u32)?;
    cursor.write_u32_le(packer.weights_type as u32)?;
    cursor.write_u32_le(packer.diffuse_type as u32)?;
    cursor.write_u32_le(packer.index_type as u32)?;

    Ok(())
}
