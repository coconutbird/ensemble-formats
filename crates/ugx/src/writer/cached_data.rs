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
use zerocopy::IntoBytes;

use crate::chunk_ids::GEOM_HEADER_SIGNATURE;
use crate::error::Result;
use crate::raw::{GeomHeaderRaw, PackedArrayRaw};
use crate::types::UgxGeom;

/// Build the cached data chunk (0x700).
pub(super) fn build_cached_data(geom: &UgxGeom) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut cursor = Cursor::new(&mut buf);

    // ---- Geometry header via zerocopy struct ----
    let max_vertex_index = geom
        .sections
        .iter()
        .map(|s| s.num_verts as u32)
        .max()
        .unwrap_or(1);
    let instance_index_multiplier = (max_vertex_index).next_power_of_two() as i16;

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
        max_instances: 1i16.to_le_bytes(),
        instance_index_multiplier: instance_index_multiplier.to_le_bytes(),
        large_geom_bone_index: i16::MAX.to_le_bytes(),
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

    struct StringFixup {
        position: u64,
        string: String,
    }
    let mut string_fixups: Vec<StringFixup> = Vec::new();
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

        string_fixups.push(StringFixup {
            position: name_fixup_pos,
            string: bone.name.clone(),
        });
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

/// Fix up a packed array header at the given position using zerocopy.
fn fixup_packed_array_header(
    cursor: &mut Cursor<&mut Vec<u8>>,
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
    cursor.seek(std::io::SeekFrom::Start(header_pos as u64))?;
    cursor.write_all(arr.as_bytes())?;
    Ok(())
}
