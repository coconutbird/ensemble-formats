//! Cached data chunk (0x700) builder.
//!
//! Layout:
//! 1. Geometry header (signature, bounds, flags, padding) — 64 bytes to 0x40
//! 2. Packed array headers (sections, bones, accessories, valid_accessories,
//!    bone_bounds_low, bone_bounds_high) — 6 × 16 = 96 bytes
//! 3. Section data (152 bytes each) — includes UnivertPacker with string offsets
//! 4. Bone data (80 bytes each) — includes name string offsets
//! 5. Bone bounds data (12 bytes each for low, 12 bytes each for high)
//! 6. String table (null-terminated strings for bone names, pack_order, decl_order)

use alloc::vec::Vec;

use ecf::io::{MutCursor, Seek, SeekFrom, Write, WriteLe};
use zerocopy::IntoBytes;

use crate::chunk_ids::GEOM_HEADER_SIGNATURE;
use crate::error::Result;
use crate::raw::{AccessoryRaw, GeomHeaderRaw, PackedArrayRaw};
use crate::types::{Accessory, UgxGeom};

/// Build the cached data chunk (0x700).
pub(super) fn build_cached_data(geom: &UgxGeom) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut cursor = MutCursor::new(&mut buf);

    // ---- Geometry header via zerocopy struct ----
    let header = GeomHeaderRaw {
        signature: GEOM_HEADER_SIGNATURE.to_le_bytes(),
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

    // ---- Packed array headers (6 × 16 bytes, placeholders) ----
    let empty_arr = PackedArrayRaw {
        count: [0; 4],
        _padding: [0; 4],
        offset: [0; 8],
    };
    let sections_header_pos = cursor.stream_position()? as usize;
    cursor.write_all(empty_arr.as_bytes())?;
    let bones_header_pos = cursor.stream_position()? as usize;
    cursor.write_all(empty_arr.as_bytes())?;
    let accessories_header_pos = cursor.stream_position()? as usize;
    cursor.write_all(empty_arr.as_bytes())?;
    let valid_acc_header_pos = cursor.stream_position()? as usize;
    cursor.write_all(empty_arr.as_bytes())?;
    let bounds_low_header_pos = cursor.stream_position()? as usize;
    cursor.write_all(empty_arr.as_bytes())?;
    let bounds_high_header_pos = cursor.stream_position()? as usize;
    cursor.write_all(empty_arr.as_bytes())?;

    // ---- Section data ----
    let sections_offset = cursor.stream_position()?;
    let num_sections = geom.sections.len() as u32;

    let mut strings = super::string_table::StringTable::new();
    let mut bone_remap_fixups: Vec<(usize, usize)> = Vec::new();

    for (section_idx, section) in geom.sections.iter().enumerate() {
        // Fixed section fields via zerocopy struct
        let section_fixed = crate::raw::PackedSectionFixedRaw {
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
        cursor.write_all(section_fixed.as_bytes())?;

        // BoneRemap packed array (16 bytes)
        let bone_remap_header_pos = cursor.stream_position()? as usize;
        let bone_remap_arr = PackedArrayRaw {
            count: (section.bone_remap.len() as u32).to_le_bytes(),
            _padding: [0; 4],
            offset: if section.bone_remap.is_empty() {
                0xFFFF_FFFFu64.to_le_bytes()
            } else {
                0u64.to_le_bytes()
            },
        };
        cursor.write_all(bone_remap_arr.as_bytes())?;
        if !section.bone_remap.is_empty() {
            bone_remap_fixups.push((bone_remap_header_pos, section_idx));
        }

        // UnivertPacker (84 bytes)
        let packer = &section.base_vert_packer;
        let pack_order_fixup_pos = cursor.stream_position()?;
        cursor.write_u64_le(0)?;
        strings.add(pack_order_fixup_pos as usize, packer.pack_order.clone());
        let decl_order_fixup_pos = cursor.stream_position()?;
        cursor.write_u64_le(0)?;
        strings.add(decl_order_fixup_pos as usize, packer.decl_order.clone());

        cursor.write_u32_le(packer.pos_type as u32)?;
        cursor.write_u32_le(packer.basis_type as u32)?;
        cursor.write_u32_le(packer.basis_scale_type as u32)?;
        cursor.write_u32_le(packer.tangent_type as u32)?;
        cursor.write_u32_le(packer.normal_type as u32)?;
        for i in 0..8 {
            cursor.write_u32_le(packer.uv_types[i] as u32)?;
        }
        cursor.write_u32_le(packer.indices_type as u32)?;
        cursor.write_u32_le(packer.weights_type as u32)?;
        cursor.write_u32_le(packer.diffuse_type as u32)?;
        cursor.write_u32_le(packer.index_type as u32)?;

        cursor.write_i32_le(if section.rigid_only { 1 } else { 0 })?;
        cursor.write_i32_le(if section.global_bones { 1 } else { 0 })?;
        cursor.write_i32_le(0)?; // padding
    }

    // ---- Bone remap data ----
    for &(header_pos, section_idx) in &bone_remap_fixups {
        pad_to_alignment(&mut cursor, 4)?;
        let remap_offset = cursor.stream_position()?;
        cursor.write_all(&geom.sections[section_idx].bone_remap)?;
        let saved_pos = cursor.stream_position()?;
        cursor.seek(SeekFrom::Start((header_pos + 8) as u64))?;
        cursor.write_u64_le(remap_offset)?;
        cursor.seek(SeekFrom::Start(saved_pos))?;
    }

    // ---- Bone data ----
    pad_to_alignment(&mut cursor, 8)?;
    let bones_offset = cursor.stream_position()?;
    let num_bones = geom.bones.len() as u32;

    for bone in &geom.bones {
        let name_fixup_pos = cursor.stream_position()?;

        // Build model_to_bone as 16 × [u8; 4] from 4×4 row-major matrix
        let mut mtb = [[0u8; 4]; 16];
        for (r, row) in bone.model_to_bone.rows.iter().enumerate() {
            for (c, &val) in row.iter().enumerate() {
                mtb[r * 4 + c] = val.to_le_bytes();
            }
        }

        let packed_bone = crate::raw::PackedBoneRaw {
            name_offset: 0u64.to_le_bytes(), // placeholder, fixed up later
            model_to_bone: mtb,
            parent_index: bone.parent_index.to_le_bytes(),
            _padding: [0; 4],
        };
        cursor.write_all(packed_bone.as_bytes())?;

        strings.add(name_fixup_pos as usize, bone.name.clone());
    }

    // ---- Accessory data ----
    let (accessories_offset, num_accessories, acc_inner_fixups) =
        write_accessories(&mut cursor, &geom.accessories)?;
    let (valid_acc_offset, num_valid_acc, valid_acc_inner_fixups) =
        write_accessories(&mut cursor, &geom.valid_accessories)?;

    // ---- Bone bounds data ----
    pad_to_alignment(&mut cursor, 4)?;
    let bounds_low_offset = cursor.stream_position()?;
    let num_bone_bounds = geom.bone_bounds.len() as u32;
    for bb in &geom.bone_bounds {
        for &v in &bb.min {
            cursor.write_f32_le(v)?;
        }
    }

    pad_to_alignment(&mut cursor, 4)?;
    let bounds_high_offset = cursor.stream_position()?;
    for bb in &geom.bone_bounds {
        for &v in &bb.max {
            cursor.write_f32_le(v)?;
        }
    }

    // ---- Accessory inner index data (must come after all structs) ----
    write_accessory_indices(&mut cursor, &geom.accessories, &acc_inner_fixups)?;
    write_accessory_indices(
        &mut cursor,
        &geom.valid_accessories,
        &valid_acc_inner_fixups,
    )?;

    // ---- String table ----
    pad_to_alignment(&mut cursor, 2)?;
    // Release the cursor borrow so we can operate on `buf` directly via StringTable.
    {
        let _ = cursor;
    }
    strings.write_aligned(&mut buf, 2);

    // ---- Fix up packed array headers ----
    let mut cursor = MutCursor::new(&mut buf);
    fixup_packed_array_header(
        &mut cursor,
        sections_header_pos,
        num_sections,
        sections_offset,
    )?;
    fixup_packed_array_header(&mut cursor, bones_header_pos, num_bones, bones_offset)?;
    fixup_packed_array_header(
        &mut cursor,
        accessories_header_pos,
        num_accessories,
        accessories_offset,
    )?;
    fixup_packed_array_header(
        &mut cursor,
        valid_acc_header_pos,
        num_valid_acc,
        valid_acc_offset,
    )?;
    fixup_packed_array_header(
        &mut cursor,
        bounds_low_header_pos,
        num_bone_bounds,
        bounds_low_offset,
    )?;
    fixup_packed_array_header(
        &mut cursor,
        bounds_high_header_pos,
        num_bone_bounds,
        bounds_high_offset,
    )?;

    Ok(buf)
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
    let final_offset = if count == 0 { 0xFFFF_FFFF } else { offset };
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
fn write_accessories(
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
                    0xFFFF_FFFFu64.to_le_bytes()
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
