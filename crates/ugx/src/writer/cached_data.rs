//! Cached data chunk (0x700) builder.
//!
//! Supports both HW1 (v4) and HW2 (v6) formats:
//!
//! ## HW1 Layout (v4):
//! - Signature `0xC2340004`, 152-byte sections with embedded UnivertPacker,
//!   valid accessories as i32 indices, includes AABB tree.
//!
//! ## HW2 Layout (v6):
//! - Signature `0xC2340006`, 72-byte sections (no UnivertPacker),
//!   valid accessories as 4-byte i32 indices, no AABB tree.

use alloc::vec::Vec;

use nostdio::{MutCursor, Seek, SeekFrom, Write, WriteLe};
use zerocopy::IntoBytes;

use crate::constants::EMPTY_OFFSET_SENTINEL_32;
use crate::error::Result;
use crate::types::raw::{AccessoryRaw, BVector3Raw, GeomHeaderRaw, PackedArrayRaw};
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
///
/// The engine's `BPackedArray::pack` writes strings **inline** immediately
/// after the struct array that references them, rather than in a deferred
/// string table.  This writer replicates that layout:
///
/// 1. Section structs  (with embedded bone-remap data)
/// 2. UnivertPacker strings inline (null-terminated, padded to 8-byte align)
/// 3. Bone structs
/// 4. Bone name strings inline (null-terminated in 32-byte fixed slots)
/// 5. Accessories / valid-accessories / bone bounds
pub(super) fn build_cached_data(geom: &UgxGeom, version: UgxVersion) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut cursor = MutCursor::new(&mut buf);

    // 1. Geometry header (64 bytes).
    write_header(&mut cursor, geom, version)?;

    // 2. Six packed-array header placeholders (6 × 16 bytes).
    let hdr_pos = write_array_header_placeholders(&mut cursor)?;

    // 3. Section data + deferred bone-remap writes.
    let (sections_offset, num_sections, packer_string_fixups) =
        write_sections(&mut cursor, geom, version)?;

    // 4. UnivertPacker strings inline (right after section data).
    //    End cursor borrow, write strings directly to buf, then re-create cursor.
    let _ = cursor;
    write_inline_strings(&mut buf, &packer_string_fixups, 8);

    let mut cursor = MutCursor::new(&mut buf);
    cursor.seek(SeekFrom::End(0))?;

    // 5. Bone data.
    let (bones_offset, num_bones, bone_name_fixups) = write_bones(&mut cursor, geom)?;

    // 6. Bone name strings inline (32-byte fixed slots after bone structs).
    let _ = cursor;
    write_inline_strings(&mut buf, &bone_name_fixups, 32);

    let mut cursor = MutCursor::new(&mut buf);
    cursor.seek(SeekFrom::End(0))?;

    // 7. Accessories (full 24-byte structs, used by both versions).
    let (acc_offset, num_acc, acc_fixups) =
        write_accessory_structs(&mut cursor, &geom.accessories)?;

    // 8. Accessory inner index data (immediately after accessory structs,
    //    matching the engine's BPackedArray::pack layout).
    write_accessory_indices(&mut cursor, &geom.accessories, &acc_fixups)?;

    // 9. Valid accessories (version-dependent encoding).
    let (valid_acc_offset, num_valid_acc, valid_acc_fixups) =
        write_valid_accessories(&mut cursor, geom, version)?;
    if !valid_acc_fixups.is_empty() {
        write_accessory_indices(&mut cursor, &geom.valid_accessories, &valid_acc_fixups)?;
    }

    // 10. Bone bounds (min[] then max[]).
    let (bounds_low_offset, bounds_high_offset, num_bounds) = write_bone_bounds(&mut cursor, geom)?;

    // 11. Fixup pass — patch the six packed-array headers with real offsets.
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

/// An inline string fixup: at `offset_pos` in the buffer, write the u64 LE
/// offset of the string that will be placed inline later.
struct InlineStringFixup {
    /// Byte position where the u64 offset placeholder lives.
    offset_pos: usize,
    /// The string content.
    string: alloc::string::String,
}

/// Write all section structs (version-branched layout) followed by deferred
/// bone-remap data.  Returns `(offset, count, packer_string_fixups)`.
fn write_sections(
    cursor: &mut MutCursor<'_>,
    geom: &UgxGeom,
    version: UgxVersion,
) -> Result<(u64, u32, Vec<InlineStringFixup>)> {
    let sections_offset = cursor.stream_position()?;
    let num_sections = geom.sections.len() as u32;
    let mut bone_remap_fixups: Vec<(usize, usize)> = Vec::new();
    let mut packer_string_fixups: Vec<InlineStringFixup> = Vec::new();

    for (section_idx, section) in geom.sections.iter().enumerate() {
        // Fixed section fields (40 bytes, shared by both versions).
        let fixed = crate::types::raw::PackedSectionFixedRaw {
            material_index: section.material_index.to_le_bytes(),
            accessory_index: section.accessory_index.to_le_bytes(),
            max_bones: section.max_bones.to_le_bytes(),
            rigid_bone_index: section.rigid_bone_index.to_le_bytes(),
            ib_offset: section.ib_offset.to_le_bytes(),
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
            section,
            section_idx,
            version,
            &mut bone_remap_fixups,
            &mut packer_string_fixups,
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

    Ok((sections_offset, num_sections, packer_string_fixups))
}

/// Write the version-specific tail of a single section struct.
///
/// HW1 (112 bytes after fixed): bone_remap(16) + UnivertPacker(84) + flags(12).
/// HW2 (32 bytes after fixed): flags(16) + bone_remap(16).
fn write_section_tail(
    cursor: &mut MutCursor<'_>,
    section: &crate::types::Section,
    section_idx: usize,
    version: UgxVersion,
    bone_remap_fixups: &mut Vec<(usize, usize)>,
    packer_string_fixups: &mut Vec<InlineStringFixup>,
) -> Result<()> {
    match version {
        UgxVersion::Hw1 => {
            write_bone_remap_header(cursor, section, section_idx, bone_remap_fixups)?;

            write_packed_univert_packer(
                cursor,
                section.base_vert_packer.as_ref(),
                packer_string_fixups,
            )?;

            cursor.write_i32_le(if section.rigid_only { 1 } else { 0 })?;
            cursor.write_i32_le(if section.global_bones { 1 } else { 0 })?;
            cursor.write_i32_le(0)?; // padding
        }
        UgxVersion::Hw2 => {
            cursor.write_i32_le(if section.rigid_only { 1 } else { 0 })?;
            cursor.write_i32_le(if section.global_bones { 1 } else { 0 })?;
            cursor.write_i32_le(crate::constants::HW2_SECTION_RESERVED1)?;
            cursor.write_i32_le(crate::constants::HW2_SECTION_RESERVED2)?;

            write_bone_remap_header(cursor, section, section_idx, bone_remap_fixups)?;
        }
    }
    Ok(())
}

/// Write a bone remap `PackedArrayRaw` placeholder (16 bytes) and register a
/// fixup if the remap is non-empty.
fn write_bone_remap_header(
    cursor: &mut MutCursor<'_>,
    section: &crate::types::Section,
    section_idx: usize,
    bone_remap_fixups: &mut Vec<(usize, usize)>,
) -> Result<()> {
    let header_pos = cursor.stream_position()? as usize;
    let arr = PackedArrayRaw {
        count: (section.bone_remap.len() as u32).to_le_bytes(),
        _padding: [0; 4],
        offset: if section.bone_remap.is_empty() {
            (EMPTY_OFFSET_SENTINEL_32 as u64).to_le_bytes()
        } else {
            0u64.to_le_bytes()
        },
    };
    cursor.write_all(arr.as_bytes())?;
    if !section.bone_remap.is_empty() {
        bone_remap_fixups.push((header_pos, section_idx));
    }
    Ok(())
}

/// Write packed bone structs.  Returns `(offset, count, name_fixups)` for
/// the bones packed-array header and inline name strings.
fn write_bones(
    cursor: &mut MutCursor<'_>,
    geom: &UgxGeom,
) -> Result<(u64, u32, Vec<InlineStringFixup>)> {
    pad_to_alignment(cursor, 8)?;
    let offset = cursor.stream_position()?;
    let count = geom.bones.len() as u32;
    let mut name_fixups = Vec::with_capacity(geom.bones.len());

    for bone in &geom.bones {
        let name_fixup_pos = cursor.stream_position()? as usize;

        let mut mtb = [[0u8; 4]; 16];
        for (r, row) in bone.model_to_bone.rows.iter().enumerate() {
            for (c, &val) in row.iter().enumerate() {
                mtb[r * 4 + c] = val.to_le_bytes();
            }
        }

        // Padding after parent_index is always zero.
        // (The engine's BPackedArray rebase only checks the name_offset
        // sentinel at +0, not the parent_index padding at +76.)
        let padding = [0u8; 4];
        let packed = crate::types::raw::PackedBoneRaw {
            name_offset: 0u64.to_le_bytes(), // placeholder
            model_to_bone: mtb,
            parent_index: bone.parent_index.to_le_bytes(),
            _padding: padding,
        };
        cursor.write_all(packed.as_bytes())?;
        name_fixups.push(InlineStringFixup {
            offset_pos: name_fixup_pos,
            string: bone.name.clone(),
        });
    }

    Ok((offset, count, name_fixups))
}

/// Write valid accessories as i32 indices into the accessories array.
///
/// IDA analysis confirms both HW1 and HW2 use `BPackedArray_Simple__unpack`
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
        cursor.write_all(BVector3Raw::from(bb.min).as_bytes())?;
    }

    pad_to_alignment(cursor, 4)?;
    let high_offset = cursor.stream_position()?;
    for bb in &geom.bone_bounds {
        cursor.write_all(BVector3Raw::from(bb.max).as_bytes())?;
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
            writer.write_u8_le(0)?;
        }
    }
    Ok(())
}

/// Write inline strings from fixups directly into the buffer, each in a
/// fixed-size slot of `slot_size` bytes (null-terminated + zero-padded).
///
/// After writing, patches the u64 LE offset at each fixup's `offset_pos`.
fn write_inline_strings(buf: &mut Vec<u8>, fixups: &[InlineStringFixup], slot_size: usize) {
    for fixup in fixups {
        let string_offset = buf.len();
        // Write null-terminated string
        buf.extend_from_slice(fixup.string.as_bytes());
        buf.push(0);
        // Pad to slot_size
        let written = fixup.string.len() + 1;
        if written < slot_size {
            buf.resize(buf.len() + (slot_size - written), 0);
        }
        // Patch the u64 LE offset at the fixup position
        buf[fixup.offset_pos..fixup.offset_pos + 8]
            .copy_from_slice(&(string_offset as u64).to_le_bytes());
    }
}

/// Fix up a packed array header at the given position using zerocopy.
fn fixup_packed_array_header(
    cursor: &mut MutCursor<'_>,
    header_pos: usize,
    count: u32,
    offset: u64,
) -> Result<()> {
    let final_offset = if count == 0 {
        EMPTY_OFFSET_SENTINEL_32 as u64
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
                    (EMPTY_OFFSET_SENTINEL_32 as u64).to_le_bytes()
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

/// Write a packed UnivertPacker (84 bytes on-disk) for HW1 sections.
///
/// Layout: 2 string offset fields (u64 each, fixed up inline),
/// then 12 u32 type fields (pos, basis, basis_scale, tangent, normal,
/// uv[0..8], indices, weights, diffuse, index).
fn write_packed_univert_packer(
    cursor: &mut MutCursor<'_>,
    packer: Option<&crate::vertex::packer::UnivertPacker>,
    string_fixups: &mut Vec<InlineStringFixup>,
) -> Result<()> {
    let packer = match packer {
        Some(p) => p,
        None => {
            // Write 84 bytes of zeros if no packer
            for _ in 0..84 {
                cursor.write_u8_le(0)?;
            }
            return Ok(());
        }
    };

    // pack_order string offset (placeholder)
    let pack_order_pos = cursor.stream_position()? as usize;
    cursor.write_u64_le(EMPTY_OFFSET_SENTINEL_32 as u64)?;
    if !packer.pack_order.is_empty() {
        string_fixups.push(InlineStringFixup {
            offset_pos: pack_order_pos,
            string: packer.pack_order.clone(),
        });
    }

    // decl_order string offset (placeholder)
    let decl_order_pos = cursor.stream_position()? as usize;
    cursor.write_u64_le(EMPTY_OFFSET_SENTINEL_32 as u64)?;
    if !packer.decl_order.is_empty() {
        string_fixups.push(InlineStringFixup {
            offset_pos: decl_order_pos,
            string: packer.decl_order.clone(),
        });
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
