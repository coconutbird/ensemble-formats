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

use alloc::string::String;
use alloc::vec::Vec;

use byteorder::{LittleEndian, WriteBytesExt};
use std::io::{Cursor, Seek, Write};

use crate::chunk_ids::GEOM_HEADER_SIGNATURE;
use crate::error::Result;
use crate::types::UgxGeom;

/// Build the cached data chunk (0x700).
pub(super) fn build_cached_data(geom: &UgxGeom) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut cursor = Cursor::new(&mut buf);

    // ---- Geometry header (BUGXGeomHeader, 60 bytes) ----
    cursor.write_u32::<LittleEndian>(GEOM_HEADER_SIGNATURE)?;
    cursor.write_i32::<LittleEndian>(geom.rigid_bone_index)?;
    for &v in &geom.bounding_sphere.center {
        cursor.write_f32::<LittleEndian>(v)?;
    }
    cursor.write_f32::<LittleEndian>(geom.bounding_sphere.radius)?;
    for &v in &geom.bounds.min {
        cursor.write_f32::<LittleEndian>(v)?;
    }
    for &v in &geom.bounds.max {
        cursor.write_f32::<LittleEndian>(v)?;
    }
    cursor.write_i16::<LittleEndian>(1)?; // mMaxInstances
    let max_vertex_index = geom
        .sections
        .iter()
        .map(|s| s.num_verts as u32)
        .max()
        .unwrap_or(1);
    let instance_index_multiplier = (max_vertex_index).next_power_of_two() as i16;
    cursor.write_i16::<LittleEndian>(instance_index_multiplier)?;
    cursor.write_i16::<LittleEndian>(i16::MAX)?; // mLargeGeomBoneIndex
    cursor.write_u8(if geom.all_sections_rigid { 1 } else { 0 })?;
    cursor.write_u8(if geom.global_bones { 1 } else { 0 })?;
    cursor.write_u8(if geom.all_sections_skinned { 1 } else { 0 })?;
    cursor.write_u8(if geom.rigid_only { 1 } else { 0 })?;
    cursor.write_u16::<LittleEndian>(0)?; // padding
    cursor.write_u32::<LittleEndian>(0)?; // padding

    // ---- Packed array headers ----
    let sections_header_pos = cursor.stream_position()? as usize;
    write_packed_array_header_placeholder(&mut cursor)?;
    let bones_header_pos = cursor.stream_position()? as usize;
    write_packed_array_header_placeholder(&mut cursor)?;
    let accessories_header_pos = cursor.stream_position()? as usize;
    write_packed_array_header_placeholder(&mut cursor)?;
    let valid_acc_header_pos = cursor.stream_position()? as usize;
    write_packed_array_header_placeholder(&mut cursor)?;
    let bounds_low_header_pos = cursor.stream_position()? as usize;
    write_packed_array_header_placeholder(&mut cursor)?;
    let bounds_high_header_pos = cursor.stream_position()? as usize;
    write_packed_array_header_placeholder(&mut cursor)?;

    // ---- Section data ----
    let sections_offset = cursor.stream_position()?;
    let num_sections = geom.sections.len() as u32;

    struct StringFixup {
        position: u64,
        string: String,
    }
    let mut string_fixups: Vec<StringFixup> = Vec::new();
    let mut bone_remap_fixups: Vec<(usize, usize)> = Vec::new();

    for (section_idx, section) in geom.sections.iter().enumerate() {
        cursor.write_i32::<LittleEndian>(section.material_index)?;
        cursor.write_i32::<LittleEndian>(section.accessory_index)?;
        cursor.write_i32::<LittleEndian>(section.max_bones)?;
        cursor.write_i32::<LittleEndian>(section.rigid_bone_index)?;
        cursor.write_i32::<LittleEndian>(section.ib_offset)?;
        cursor.write_i32::<LittleEndian>(section.num_tris)?;
        cursor.write_i32::<LittleEndian>(section.vb_offset)?;
        cursor.write_i32::<LittleEndian>(section.vb_bytes)?;
        cursor.write_i32::<LittleEndian>(section.vert_size)?;
        cursor.write_i32::<LittleEndian>(section.num_verts)?;

        // BoneRemap packed array (16 bytes)
        let bone_remap_header_pos = cursor.stream_position()? as usize;
        cursor.write_u32::<LittleEndian>(section.bone_remap.len() as u32)?;
        cursor.write_u32::<LittleEndian>(0)?;
        if section.bone_remap.is_empty() {
            cursor.write_u64::<LittleEndian>(0xFFFFFFFF)?;
        } else {
            cursor.write_u64::<LittleEndian>(0)?;
            bone_remap_fixups.push((bone_remap_header_pos, section_idx));
        }

        // UnivertPacker (84 bytes)
        let packer = &section.base_vert_packer;
        let pack_order_fixup_pos = cursor.stream_position()?;
        cursor.write_u64::<LittleEndian>(0)?;
        string_fixups.push(StringFixup {
            position: pack_order_fixup_pos,
            string: packer.pack_order.clone(),
        });
        let decl_order_fixup_pos = cursor.stream_position()?;
        cursor.write_u64::<LittleEndian>(0)?;
        string_fixups.push(StringFixup {
            position: decl_order_fixup_pos,
            string: packer.decl_order.clone(),
        });

        cursor.write_u32::<LittleEndian>(packer.pos_type as u32)?;
        cursor.write_u32::<LittleEndian>(packer.basis_type as u32)?;
        cursor.write_u32::<LittleEndian>(packer.basis_scale_type as u32)?;
        cursor.write_u32::<LittleEndian>(packer.tangent_type as u32)?;
        cursor.write_u32::<LittleEndian>(packer.normal_type as u32)?;
        for i in 0..8 {
            cursor.write_u32::<LittleEndian>(packer.uv_types[i] as u32)?;
        }
        cursor.write_u32::<LittleEndian>(packer.indices_type as u32)?;
        cursor.write_u32::<LittleEndian>(packer.weights_type as u32)?;
        cursor.write_u32::<LittleEndian>(packer.diffuse_type as u32)?;
        cursor.write_u32::<LittleEndian>(packer.index_type as u32)?;

        cursor.write_i32::<LittleEndian>(if section.rigid_only { 1 } else { 0 })?;
        cursor.write_i32::<LittleEndian>(if section.global_bones { 1 } else { 0 })?;
        cursor.write_i32::<LittleEndian>(0)?; // padding
    }

    // ---- Bone remap data ----
    for &(header_pos, section_idx) in &bone_remap_fixups {
        pad_to_alignment(&mut cursor, 4)?;
        let remap_offset = cursor.stream_position()?;
        cursor.write_all(&geom.sections[section_idx].bone_remap)?;
        let saved_pos = cursor.stream_position()?;
        cursor.seek(std::io::SeekFrom::Start((header_pos + 8) as u64))?;
        cursor.write_u64::<LittleEndian>(remap_offset)?;
        cursor.seek(std::io::SeekFrom::Start(saved_pos))?;
    }

    // ---- Bone data ----
    pad_to_alignment(&mut cursor, 8)?;
    let bones_offset = cursor.stream_position()?;
    let num_bones = geom.bones.len() as u32;

    for bone in &geom.bones {
        let name_fixup_pos = cursor.stream_position()?;
        cursor.write_u64::<LittleEndian>(0)?;
        string_fixups.push(StringFixup {
            position: name_fixup_pos,
            string: bone.name.clone(),
        });

        for row in &bone.model_to_bone.rows {
            for &val in row {
                cursor.write_f32::<LittleEndian>(val)?;
            }
        }

        cursor.write_i32::<LittleEndian>(bone.parent_index)?;
        cursor.write_u32::<LittleEndian>(0)?; // padding
    }

    // ---- Bone bounds data ----
    pad_to_alignment(&mut cursor, 4)?;
    let bounds_low_offset = cursor.stream_position()?;
    let num_bone_bounds = geom.bone_bounds.len() as u32;
    for bb in &geom.bone_bounds {
        for &v in &bb.min {
            cursor.write_f32::<LittleEndian>(v)?;
        }
    }

    pad_to_alignment(&mut cursor, 4)?;
    let bounds_high_offset = cursor.stream_position()?;
    for bb in &geom.bone_bounds {
        for &v in &bb.max {
            cursor.write_f32::<LittleEndian>(v)?;
        }
    }

    // ---- String table ----
    pad_to_alignment(&mut cursor, 2)?;
    let mut string_offsets: std::collections::HashMap<String, u64> =
        std::collections::HashMap::new();
    for fixup in &string_fixups {
        if !string_offsets.contains_key(&fixup.string) {
            let offset = cursor.stream_position()?;
            string_offsets.insert(fixup.string.clone(), offset);
            cursor.write_all(fixup.string.as_bytes())?;
            cursor.write_u8(0)?;
            pad_to_alignment(&mut cursor, 2)?;
        }
    }

    // ---- Fix up string offsets ----
    for fixup in &string_fixups {
        let string_offset = string_offsets[&fixup.string];
        cursor.seek(std::io::SeekFrom::Start(fixup.position))?;
        cursor.write_u64::<LittleEndian>(string_offset)?;
    }

    // ---- Fix up packed array headers ----
    fixup_packed_array_header(
        &mut cursor,
        sections_header_pos,
        num_sections,
        sections_offset,
    )?;
    fixup_packed_array_header(&mut cursor, bones_header_pos, num_bones, bones_offset)?;
    fixup_packed_array_header(&mut cursor, accessories_header_pos, 0, 0)?;
    fixup_packed_array_header(&mut cursor, valid_acc_header_pos, 0, 0)?;
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

/// Write a placeholder packed array header (16 bytes: u32 count, u32 pad, u64 offset).
fn write_packed_array_header_placeholder<W: Write>(writer: &mut W) -> Result<()> {
    writer.write_u32::<LittleEndian>(0)?;
    writer.write_u32::<LittleEndian>(0)?;
    writer.write_u64::<LittleEndian>(0)?;
    Ok(())
}

/// Fix up a packed array header at the given position.
fn fixup_packed_array_header(
    cursor: &mut Cursor<&mut Vec<u8>>,
    header_pos: usize,
    count: u32,
    offset: u64,
) -> Result<()> {
    cursor.seek(std::io::SeekFrom::Start(header_pos as u64))?;
    cursor.write_u32::<LittleEndian>(count)?;
    cursor.write_u32::<LittleEndian>(0)?;
    let final_offset = if count == 0 { 0xFFFFFFFF } else { offset };
    cursor.write_u64::<LittleEndian>(final_offset)?;
    Ok(())
}
