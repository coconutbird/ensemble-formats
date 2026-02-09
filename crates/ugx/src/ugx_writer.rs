//! UGX file writer.
//!
//! Serializes a `UgxGeom` into UGX binary format (ECF container).
//! Writes chunks 0x700 (cached data), 0x701 (index buffer), 0x702 (vertex buffer),
//! 0x703 (granny bones), and 0x704 (materials).

use byteorder::{LittleEndian, WriteBytesExt};
use std::io::{Cursor, Seek, Write};

use crate::error::Result;
use crate::types::{MapType, Material};
use crate::ugx::UgxGeom;

/// ECF chunk IDs for UGX.
const ECF_CACHED_DATA_CHUNK_ID: u64 = 0x00000700;
const ECF_IB_CHUNK_ID: u64 = 0x00000701;
const ECF_VB_CHUNK_ID: u64 = 0x00000702;
const ECF_GRANNY_CHUNK_ID: u64 = 0x00000703;
const ECF_MATERIAL_CHUNK_ID: u64 = 0x00000704;

/// Geometry header signature (v4 = original format, writer always writes v4).
const GEOM_HEADER_SIGNATURE: u32 = 0xC2340004;

/// Write a UGX geometry to bytes (ECF container).
pub fn write_ugx(geom: &UgxGeom) -> Result<Vec<u8>> {
    let cached_data = build_cached_data(geom)?;
    let ib_data = build_index_buffer(geom);

    let mut output = Cursor::new(Vec::new());
    // ECF file ID 0xAAC93746 is required for UGX files - the game validates this in BGrannyModel::load
    let mut ecf = ecf::EcfWriter::new(&mut output, 0xAAC93746);

    ecf.add_chunk(ECF_CACHED_DATA_CHUNK_ID, cached_data);
    ecf.add_chunk(ECF_IB_CHUNK_ID, ib_data);
    ecf.add_chunk(ECF_VB_CHUNK_ID, geom.vertex_buffer.clone());

    // Write granny bones chunk if we have granny bone data
    if !geom.granny_bones.is_empty() {
        let granny_data = build_granny_data(geom)?;
        ecf.add_chunk(ECF_GRANNY_CHUNK_ID, granny_data);
    }

    // Write materials chunk if we have materials
    if !geom.materials.is_empty() {
        let mat_data = build_material_data(geom)?;
        ecf.add_chunk(ECF_MATERIAL_CHUNK_ID, mat_data);
    }

    ecf.finalize()?;

    Ok(output.into_inner())
}

/// Build the index buffer chunk (0x701).
fn build_index_buffer(geom: &UgxGeom) -> Vec<u8> {
    let mut buf = Vec::with_capacity(geom.index_buffer.len() * 2);
    for &idx in &geom.index_buffer {
        buf.extend_from_slice(&idx.to_le_bytes());
    }
    buf
}

/// Granny bone size in bytes.
///
/// Per-bone layout (164 bytes):
/// - `+0x00` (8 bytes): u64 name string offset
/// - `+0x08` (4 bytes): i32 parent index
/// - `+0x0C` (4 bytes): u32 local transform flags (bit 0=position, 1=orientation, 2=scale_shear)
/// - `+0x10` (12 bytes): f32×3 local position
/// - `+0x1C` (16 bytes): f32×4 local orientation (quaternion xyzw)
/// - `+0x2C` (36 bytes): f32×9 local scale_shear (3×3 row-major)
/// - `+0x50` (64 bytes): f32×16 inverse world matrix (4×4 row-major)
/// - `+0x90` (4 bytes): f32 LOD error
/// - `+0x94` (16 bytes): extended data (zeros)
const GRANNY_BONE_SIZE: usize = 164;

/// Granny local transform flags.
const GRANNY_HAS_POSITION: u32 = 0x1;
const GRANNY_HAS_ORIENTATION: u32 = 0x2;
const GRANNY_HAS_SCALE_SHEAR: u32 = 0x4;

/// Build the granny bones chunk (0x703).
///
/// Produces a Granny2-compatible serialized chunk with:
/// - File info header with skeleton and mesh array pointers
/// - Skeleton struct with bone count and array pointer
/// - Bone array with names, parent indices, computed local transforms, and inverse world matrices
/// - Mesh pointer array with section names
/// - String table
///
/// Local transforms are computed from inverse world matrices: for each bone,
/// `local = parent_world_inverse * world` where `world = inverse(inverse_world)`.
fn build_granny_data(geom: &UgxGeom) -> Result<Vec<u8>> {
    use crate::types::Matrix4x4;

    let bone_count = geom.granny_bones.len();
    let section_count = geom.sections.len();

    // ---- Compute local transforms from inverse world matrices ----
    // world[i] = inverse(inverse_world[i])
    // local[i] = inverse_world[parent] * world[i]   (for non-root bones)
    // local[i] = world[i]                            (for root bones)
    let world_matrices: Vec<Matrix4x4> = geom
        .granny_bones
        .iter()
        .map(|bone| bone.inverse_world_matrix.inverse().unwrap_or_default())
        .collect();

    struct LocalTransform {
        flags: u32,
        position: [f32; 3],
        orientation: [f32; 4],      // quaternion xyzw
        scale_shear: [[f32; 3]; 3], // 3×3 row-major
    }

    let local_transforms: Vec<LocalTransform> = geom
        .granny_bones
        .iter()
        .enumerate()
        .map(|(i, bone)| {
            let local_matrix =
                if bone.parent_index >= 0 && (bone.parent_index as usize) < bone_count {
                    let parent_idx = bone.parent_index as usize;
                    // local = parent_inverse_world * child_world
                    geom.granny_bones[parent_idx]
                        .inverse_world_matrix
                        .multiply(&world_matrices[i])
                } else {
                    world_matrices[i].clone()
                };

            // Extract position from row 3 (translation row in row-major DX convention)
            let position = local_matrix.translation();

            // Extract the upper-left 3×3 for rotation + scale
            let m = &local_matrix.rows;
            // Column lengths = scale factors
            let sx = (m[0][0] * m[0][0] + m[1][0] * m[1][0] + m[2][0] * m[2][0]).sqrt();
            let sy = (m[0][1] * m[0][1] + m[1][1] * m[1][1] + m[2][1] * m[2][1]).sqrt();
            let sz = (m[0][2] * m[0][2] + m[1][2] * m[1][2] + m[2][2] * m[2][2]).sqrt();

            // Build a pure rotation matrix by removing scale
            let rot_matrix = if sx > 1e-7 && sy > 1e-7 && sz > 1e-7 {
                Matrix4x4 {
                    rows: [
                        [m[0][0] / sx, m[0][1] / sy, m[0][2] / sz, 0.0],
                        [m[1][0] / sx, m[1][1] / sy, m[1][2] / sz, 0.0],
                        [m[2][0] / sx, m[2][1] / sy, m[2][2] / sz, 0.0],
                        [0.0, 0.0, 0.0, 1.0],
                    ],
                }
            } else {
                Matrix4x4::identity()
            };

            let orientation = rot_matrix.to_quaternion();

            // Scale_shear: Granny stores this as the 3×3 matrix S where M_3x3 = R * S
            // So S = R^T * M_3x3 (since R is orthogonal, R^-1 = R^T)
            let rt = rot_matrix.transpose();
            let scale_shear = [
                [
                    rt.rows[0][0] * m[0][0] + rt.rows[0][1] * m[1][0] + rt.rows[0][2] * m[2][0],
                    rt.rows[0][0] * m[0][1] + rt.rows[0][1] * m[1][1] + rt.rows[0][2] * m[2][1],
                    rt.rows[0][0] * m[0][2] + rt.rows[0][1] * m[1][2] + rt.rows[0][2] * m[2][2],
                ],
                [
                    rt.rows[1][0] * m[0][0] + rt.rows[1][1] * m[1][0] + rt.rows[1][2] * m[2][0],
                    rt.rows[1][0] * m[0][1] + rt.rows[1][1] * m[1][1] + rt.rows[1][2] * m[2][1],
                    rt.rows[1][0] * m[0][2] + rt.rows[1][1] * m[1][2] + rt.rows[1][2] * m[2][2],
                ],
                [
                    rt.rows[2][0] * m[0][0] + rt.rows[2][1] * m[1][0] + rt.rows[2][2] * m[2][0],
                    rt.rows[2][0] * m[0][1] + rt.rows[2][1] * m[1][1] + rt.rows[2][2] * m[2][1],
                    rt.rows[2][0] * m[0][2] + rt.rows[2][1] * m[1][2] + rt.rows[2][2] * m[2][2],
                ],
            ];

            // Determine which flags to set based on whether values differ from defaults
            let mut flags = 0u32;
            if position[0].abs() > 1e-7 || position[1].abs() > 1e-7 || position[2].abs() > 1e-7 {
                flags |= GRANNY_HAS_POSITION;
            }
            if (orientation[0].abs() > 1e-7)
                || (orientation[1].abs() > 1e-7)
                || (orientation[2].abs() > 1e-7)
                || ((orientation[3] - 1.0).abs() > 1e-7)
            {
                flags |= GRANNY_HAS_ORIENTATION;
            }
            let is_identity_scale = (scale_shear[0][0] - 1.0).abs() < 1e-5
                && scale_shear[0][1].abs() < 1e-5
                && scale_shear[0][2].abs() < 1e-5
                && scale_shear[1][0].abs() < 1e-5
                && (scale_shear[1][1] - 1.0).abs() < 1e-5
                && scale_shear[1][2].abs() < 1e-5
                && scale_shear[2][0].abs() < 1e-5
                && scale_shear[2][1].abs() < 1e-5
                && (scale_shear[2][2] - 1.0).abs() < 1e-5;
            if !is_identity_scale {
                flags |= GRANNY_HAS_SCALE_SHEAR;
            }

            LocalTransform {
                flags,
                position,
                orientation,
                scale_shear,
            }
        })
        .collect();

    // ---- Layout ----
    // [0x00..0x60]: File info header (96 bytes)
    //   [0x10..0x18]: u64 filename string offset
    //   [0x30..0x34]: u32 skeleton count (1)
    //   [0x34..0x3C]: u64 skeleton offset
    //   [0x54..0x58]: i32 mesh count
    //   [0x58..0x60]: u64 mesh array ptr
    // [0x60..0x80]: Skeleton struct (32 bytes)
    //   [+0x00..+0x08]: u64 name string offset
    //   [+0x08..+0x10]: padding
    //   [+0x10..+0x18]: u64 bone name (reuse first bone name)
    //   [+0x18..+0x1C]: u32 bone count
    //   [+0x1C..+0x24]: u64 bones array offset
    // [0x84..0x88]: padding to align bones to 8 bytes
    // [0x88..]: Bone array (bone_count × 164 bytes)
    // After bones: Mesh pointer array (section_count × 8 bytes)
    // After mesh ptrs: Mesh structs (section_count × 8 bytes)
    // String table

    let header_size: usize = 96; // 0x60
    let skeleton_offset: u64 = header_size as u64;
    let skeleton_size: usize = 36;
    // Align bones start to 8 bytes
    let bones_start_unaligned = header_size + skeleton_size;
    let bones_start = (bones_start_unaligned + 7) & !7; // 136 = 0x88
    let bones_end = bones_start + bone_count * GRANNY_BONE_SIZE;

    let mesh_ptrs_start = bones_end;
    let mesh_structs_start = mesh_ptrs_start + section_count * 8;
    let strings_start = mesh_structs_start + section_count * 8;

    let mut buf = vec![0u8; strings_start];
    let mut cursor = Cursor::new(&mut buf);

    // ---- File info header [0x00..0x60] ----
    // [0x30]: u32 skeleton_count = 1
    cursor.seek(std::io::SeekFrom::Start(0x30))?;
    cursor.write_u32::<LittleEndian>(1)?;
    // [0x34]: u64 skeleton_offset
    cursor.write_u64::<LittleEndian>(skeleton_offset)?;
    // [0x54]: i32 mesh_count
    cursor.seek(std::io::SeekFrom::Start(0x54))?;
    cursor.write_i32::<LittleEndian>(section_count as i32)?;
    // [0x58]: u64 mesh_pointer_array_offset
    cursor.write_u64::<LittleEndian>(mesh_ptrs_start as u64)?;

    // ---- Skeleton struct [0x60..] ----
    // +0x18: u32 bone_count
    cursor.seek(std::io::SeekFrom::Start(skeleton_offset + 0x18))?;
    cursor.write_u32::<LittleEndian>(bone_count as u32)?;
    // +0x1C: u64 bones_array_offset
    cursor.write_u64::<LittleEndian>(bones_start as u64)?;

    // ---- Bone array ----
    struct StringFixup {
        position: usize,
        string: String,
    }
    let mut string_fixups: Vec<StringFixup> = Vec::new();

    for (i, bone) in geom.granny_bones.iter().enumerate() {
        let base = bones_start + i * GRANNY_BONE_SIZE;
        let lt = &local_transforms[i];

        // +0x00: u64 name offset (placeholder)
        string_fixups.push(StringFixup {
            position: base,
            string: bone.name.clone(),
        });

        // +0x08: i32 parent_index
        cursor.seek(std::io::SeekFrom::Start((base + 0x08) as u64))?;
        cursor.write_i32::<LittleEndian>(bone.parent_index)?;

        // +0x0C: u32 local_transform_flags
        cursor.write_u32::<LittleEndian>(lt.flags)?;

        // +0x10: f32×3 local position
        for &v in &lt.position {
            cursor.write_f32::<LittleEndian>(v)?;
        }

        // +0x1C: f32×4 local orientation (quaternion xyzw)
        for &v in &lt.orientation {
            cursor.write_f32::<LittleEndian>(v)?;
        }

        // +0x2C: f32×9 local scale_shear (3×3 row-major)
        for row in &lt.scale_shear {
            for &v in row {
                cursor.write_f32::<LittleEndian>(v)?;
            }
        }

        // +0x50: f32×16 inverse_world_matrix (4×4 row-major)
        for row in &bone.inverse_world_matrix.rows {
            for &val in row {
                cursor.write_f32::<LittleEndian>(val)?;
            }
        }

        // +0x90: f32 LOD_error
        // Compute as bounding sphere radius — a reasonable default for bones
        cursor.write_f32::<LittleEndian>(geom.bounding_sphere.radius)?;

        // +0x94: 16 bytes extended data (already zeros from initialization)
    }

    // ---- Mesh pointer array + mesh structs ----
    for i in 0..section_count {
        let ptr_pos = mesh_ptrs_start + i * 8;
        let struct_pos = mesh_structs_start + i * 8;

        cursor.seek(std::io::SeekFrom::Start(ptr_pos as u64))?;
        cursor.write_u64::<LittleEndian>(struct_pos as u64)?;

        string_fixups.push(StringFixup {
            position: struct_pos,
            string: format!("mesh_{}", i),
        });
    }

    // ---- String table ----
    // Also add skeleton name and filename to fixups
    let skeleton_name = "Skeleton".to_string();
    string_fixups.push(StringFixup {
        position: skeleton_offset as usize, // skeleton +0x00: name offset
        string: skeleton_name,
    });

    drop(cursor);

    let mut string_offsets: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for fixup in &string_fixups {
        if !string_offsets.contains_key(&fixup.string) {
            let offset = buf.len();
            string_offsets.insert(fixup.string.clone(), offset);
            buf.extend_from_slice(fixup.string.as_bytes());
            buf.push(0); // null terminator
        }
    }

    // ---- Fix up string offsets ----
    for fixup in &string_fixups {
        let string_offset = string_offsets[&fixup.string] as u64;
        let pos = fixup.position;
        buf[pos..pos + 8].copy_from_slice(&string_offset.to_le_bytes());
    }

    Ok(buf)
}

/// Build the cached data chunk (0x700).
///
/// Layout:
/// 1. Geometry header (signature, bounds, flags, padding) — 64 bytes to 0x40
/// 2. Packed array headers (sections, bones, accessories, valid_accessories,
///    bone_bounds_low, bone_bounds_high) — 6 × 16 = 96 bytes
/// 3. Section data (152 bytes each) — includes UnivertPacker with string offsets
/// 4. Bone data (80 bytes each) — includes name string offsets
/// 5. Bone bounds data (12 bytes each for low, 12 bytes each for high)
/// 6. String table (null-terminated strings for bone names, pack_order, decl_order)
///
/// String offsets are written as 64-bit pointers into this blob.
fn build_cached_data(geom: &UgxGeom) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut cursor = Cursor::new(&mut buf);

    // ---- Geometry header ----
    // +0x00: signature
    cursor.write_u32::<LittleEndian>(GEOM_HEADER_SIGNATURE)?;
    // +0x04: rigid_bone_index
    cursor.write_i32::<LittleEndian>(geom.rigid_bone_index)?;
    // +0x08: bounding sphere center + radius
    for &v in &geom.bounding_sphere.center {
        cursor.write_f32::<LittleEndian>(v)?;
    }
    cursor.write_f32::<LittleEndian>(geom.bounding_sphere.radius)?;
    // +0x1C: AABB min
    for &v in &geom.bounds.min {
        cursor.write_f32::<LittleEndian>(v)?;
    }
    // +0x28: AABB max
    for &v in &geom.bounds.max {
        cursor.write_f32::<LittleEndian>(v)?;
    }
    // +0x34: max_instances
    cursor.write_i16::<LittleEndian>(0)?;
    // +0x36: instance_index_multiplier
    cursor.write_i16::<LittleEndian>(0)?;
    // +0x38: large_geom_bone_index
    cursor.write_i16::<LittleEndian>(-1)?;
    // +0x3A: flags
    cursor.write_u8(if geom.all_sections_rigid { 1 } else { 0 })?;
    cursor.write_u8(if geom.global_bones { 1 } else { 0 })?;
    cursor.write_u8(if geom.all_sections_skinned { 1 } else { 0 })?;
    cursor.write_u8(if geom.rigid_only { 1 } else { 0 })?;
    // +0x3E: padding to 0x40
    cursor.write_u16::<LittleEndian>(0)?;
    cursor.write_u32::<LittleEndian>(0)?;

    // Current position: 0x40 (64 bytes)
    // ---- Packed array headers ----
    // We need to know the final offsets, so we reserve space for headers first,
    // then write the actual data, then go back and fix up the offsets.

    let sections_header_pos = cursor.stream_position()? as usize;
    write_packed_array_header_placeholder(&mut cursor)?; // sections
    let bones_header_pos = cursor.stream_position()? as usize;
    write_packed_array_header_placeholder(&mut cursor)?; // bones
    let accessories_header_pos = cursor.stream_position()? as usize;
    write_packed_array_header_placeholder(&mut cursor)?; // accessories
    let valid_acc_header_pos = cursor.stream_position()? as usize;
    write_packed_array_header_placeholder(&mut cursor)?; // valid_accessories
    let bounds_low_header_pos = cursor.stream_position()? as usize;
    write_packed_array_header_placeholder(&mut cursor)?; // bone_bounds_low
    let bounds_high_header_pos = cursor.stream_position()? as usize;
    write_packed_array_header_placeholder(&mut cursor)?; // bone_bounds_high

    // ---- Section data ----
    // Each section is 152 bytes. UnivertPacker strings need fixup.
    let sections_offset = cursor.stream_position()? as u64;
    let num_sections = geom.sections.len() as u32;

    // Track positions that need string offset fixup
    struct StringFixup {
        position: u64, // position in the buffer where the u64 offset is written
        string: String,
    }
    let mut string_fixups: Vec<StringFixup> = Vec::new();
    let mut bone_remap_fixups: Vec<(usize, usize)> = Vec::new(); // (header_pos, section_idx)

    for (section_idx, section) in geom.sections.iter().enumerate() {
        // +0x00: mMaterialIndex
        cursor.write_i32::<LittleEndian>(section.material_index)?;
        // +0x04: mAccessoryIndex
        cursor.write_i32::<LittleEndian>(section.accessory_index)?;
        // +0x08: mMaxBones
        cursor.write_i32::<LittleEndian>(section.max_bones)?;
        // +0x0C: mRigidBoneIndex
        cursor.write_i32::<LittleEndian>(section.rigid_bone_index)?;
        // +0x10: mIBOfs
        cursor.write_i32::<LittleEndian>(section.ib_offset)?;
        // +0x14: mNumTris
        cursor.write_i32::<LittleEndian>(section.num_tris)?;
        // +0x18: mVBOfs
        cursor.write_i32::<LittleEndian>(section.vb_offset)?;
        // +0x1C: mVBBytes
        cursor.write_i32::<LittleEndian>(section.vb_bytes)?;
        // +0x20: mVertSize
        cursor.write_i32::<LittleEndian>(section.vert_size)?;
        // +0x24: mNumVerts
        cursor.write_i32::<LittleEndian>(section.num_verts)?;

        // +0x28: BoneRemap packed array (16 bytes)
        let bone_remap_header_pos = cursor.stream_position()? as usize;
        cursor.write_u32::<LittleEndian>(section.bone_remap.len() as u32)?; // count
        cursor.write_u32::<LittleEndian>(0)?; // pad
        cursor.write_u64::<LittleEndian>(0)?; // offset (placeholder, fixed up later)
        if !section.bone_remap.is_empty() {
            bone_remap_fixups.push((bone_remap_header_pos, section_idx));
        }

        // +0x38: UnivertPacker (84 bytes)
        let packer = &section.base_vert_packer;

        // pack_order string offset (u64) — fixup later
        let pack_order_fixup_pos = cursor.stream_position()?;
        cursor.write_u64::<LittleEndian>(0)?; // placeholder
        string_fixups.push(StringFixup {
            position: pack_order_fixup_pos,
            string: packer.pack_order.clone(),
        });

        // decl_order string offset (u64) — fixup later
        let decl_order_fixup_pos = cursor.stream_position()?;
        cursor.write_u64::<LittleEndian>(0)?; // placeholder
        string_fixups.push(StringFixup {
            position: decl_order_fixup_pos,
            string: packer.decl_order.clone(),
        });

        // Vertex element types (each u32)
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

        // +0x8C: mRigidOnly
        cursor.write_i32::<LittleEndian>(if section.rigid_only { 1 } else { 0 })?;
        // +0x90: mGlobalBones
        cursor.write_i32::<LittleEndian>(if section.global_bones { 1 } else { 0 })?;
        // +0x94: mPadding
        cursor.write_i32::<LittleEndian>(0)?;
    }

    // ---- Bone remap data ----
    // Write bone remap arrays for sections that have them, and fix up offsets
    for &(header_pos, section_idx) in &bone_remap_fixups {
        let remap_offset = cursor.stream_position()? as u64;
        cursor.write_all(&geom.sections[section_idx].bone_remap)?;
        // Fix up the offset in the packed array header (at header_pos + 8 for the u64 offset)
        let saved_pos = cursor.stream_position()?;
        cursor.seek(std::io::SeekFrom::Start((header_pos + 8) as u64))?;
        cursor.write_u64::<LittleEndian>(remap_offset)?;
        cursor.seek(std::io::SeekFrom::Start(saved_pos))?;
    }

    // ---- Bone data ----
    let bones_offset = cursor.stream_position()? as u64;
    let num_bones = geom.bones.len() as u32;

    for bone in &geom.bones {
        // +0x00: name offset (u64) — fixup later
        let name_fixup_pos = cursor.stream_position()?;
        cursor.write_u64::<LittleEndian>(0)?; // placeholder
        string_fixups.push(StringFixup {
            position: name_fixup_pos,
            string: bone.name.clone(),
        });

        // +0x08: model_to_bone matrix (4x4 = 64 bytes)
        for row in &bone.model_to_bone.rows {
            for &val in row {
                cursor.write_f32::<LittleEndian>(val)?;
            }
        }

        // +0x48: parent_index (i32)
        cursor.write_i32::<LittleEndian>(bone.parent_index)?;
        // +0x4C: padding (4 bytes)
        cursor.write_u32::<LittleEndian>(0)?;
    }

    // ---- Bone bounds data ----
    let bounds_low_offset = cursor.stream_position()? as u64;
    let num_bone_bounds = geom.bone_bounds.len() as u32;
    for bb in &geom.bone_bounds {
        for &v in &bb.min {
            cursor.write_f32::<LittleEndian>(v)?;
        }
    }

    let bounds_high_offset = cursor.stream_position()? as u64;
    for bb in &geom.bone_bounds {
        for &v in &bb.max {
            cursor.write_f32::<LittleEndian>(v)?;
        }
    }

    // ---- String table ----
    // Deduplicate strings and assign offsets
    let mut string_offsets: std::collections::HashMap<String, u64> =
        std::collections::HashMap::new();
    for fixup in &string_fixups {
        if !string_offsets.contains_key(&fixup.string) {
            let offset = cursor.stream_position()?;
            string_offsets.insert(fixup.string.clone(), offset);
            cursor.write_all(fixup.string.as_bytes())?;
            cursor.write_u8(0)?; // null terminator
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

    drop(cursor);
    Ok(buf)
}

/// Write a placeholder packed array header (16 bytes: u32 count, u32 pad, u64 offset).
fn write_packed_array_header_placeholder<W: Write>(writer: &mut W) -> Result<()> {
    writer.write_u32::<LittleEndian>(0)?; // count
    writer.write_u32::<LittleEndian>(0)?; // pad
    writer.write_u64::<LittleEndian>(0)?; // offset
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
    cursor.write_u32::<LittleEndian>(0)?; // pad
    cursor.write_u64::<LittleEndian>(offset)?;
    Ok(())
}

/// Build the material chunk (0x704) as a BBinaryDataTree packed document.
///
/// Tree structure:
/// ```text
/// <Materials>
///   <Material @Name="name" @Ver=4>
///     <NameValues>
///       <SpecPower> text=Float(...)
///       <Flags> text=UInt(...)
///       <BlendType> text=UInt(...)
///       <Opacity> text=UInt(0-255)
///     <Maps>
///       <diffuse @UVWVel=Float(0.0)>
///         <Map @Name="texture_path" @Channel=Int(0) @Flags=UInt(7)>
///       ...
/// ```
fn build_material_data(geom: &UgxGeom) -> Result<Vec<u8>> {
    let mut root = bdt::Node::new("Materials");

    for mat in &geom.materials {
        root.children.push(build_material_node(mat));
    }

    let data = bdt::PackedWriter::write_le(&root)?;
    Ok(data)
}

/// Build a single material BDT node.
fn build_material_node(mat: &Material) -> bdt::Node {
    let mut node = bdt::Node::new("Material");
    node.attributes
        .push(bdt::Attribute::with_string("Name", &mat.name));
    node.attributes
        .push(bdt::Attribute::new("Ver", bdt::Variant::Int(4)));

    // NameValues child with material properties
    let mut nv = bdt::Node::new("NameValues");

    let mut spec_node = bdt::Node::new("SpecPower");
    spec_node.text = bdt::Variant::Float(mat.spec_power);
    nv.children.push(spec_node);

    let mut flags_node = bdt::Node::new("Flags");
    flags_node.text = bdt::Variant::UInt(mat.flags);
    nv.children.push(flags_node);

    let mut blend_node = bdt::Node::new("BlendType");
    blend_node.text = bdt::Variant::UInt(mat.blend_type as u32);
    nv.children.push(blend_node);

    let mut opacity_node = bdt::Node::new("Opacity");
    opacity_node.text = bdt::Variant::UInt((mat.opacity * 255.0) as u32);
    nv.children.push(opacity_node);

    node.children.push(nv);

    // Maps child with texture map slots
    let mut maps = bdt::Node::new("Maps");

    for map_type in MapType::ALL {
        let idx = map_type as usize;
        let uvw = mat.uvw_velocity[idx];
        let has_maps = !mat.maps[idx].is_empty();
        let has_uvw = uvw[0] != 0.0 || uvw[1] != 0.0 || uvw[2] != 0.0;

        // Only write map type nodes that have data
        if !has_maps && !has_uvw {
            continue;
        }

        let mut type_node = bdt::Node::new(map_type.name());
        type_node
            .attributes
            .push(bdt::Attribute::new("UVWVel", bdt::Variant::Float(uvw[0])));

        for map in &mat.maps[idx] {
            let mut map_node = bdt::Node::new("Map");
            map_node
                .attributes
                .push(bdt::Attribute::with_string("Name", &map.name));
            map_node.attributes.push(bdt::Attribute::new(
                "Channel",
                bdt::Variant::Int(map.channel as i32),
            ));
            map_node.attributes.push(bdt::Attribute::new(
                "Flags",
                bdt::Variant::UInt(map.flags as u32),
            ));
            type_node.children.push(map_node);
        }

        maps.children.push(type_node);
    }

    node.children.push(maps);

    node
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;
    use crate::ugx::GrannyBone;
    use crate::univert_packer::{UnivertPacker, UnpackedVertex, MAX_UV};
    use crate::vertex_element::VertexElementType;

    /// Create a minimal test UgxGeom with one section and two bones.
    fn make_test_geom() -> UgxGeom {
        let packer = UnivertPacker {
            pack_order: "PNT0".to_string(),
            decl_order: "PNT0".to_string(),
            pos_type: VertexElementType::Float3,
            basis_type: VertexElementType::Float4,
            basis_scale_type: VertexElementType::Float2,
            tangent_type: VertexElementType::Ignore,
            normal_type: VertexElementType::Float3,
            uv_types: {
                let mut uv = [VertexElementType::Ignore; MAX_UV];
                uv[0] = VertexElementType::Float2;
                uv
            },
            indices_type: VertexElementType::UByte4,
            weights_type: VertexElementType::Float4,
            diffuse_type: VertexElementType::Ignore,
            index_type: VertexElementType::Ignore,
        };

        // Build vertex buffer
        let vertices = vec![
            UnpackedVertex {
                position: [0.0, 0.0, 0.0],
                normal: [0.0, 1.0, 0.0],
                texcoords: {
                    let mut tc = [[0.0; 2]; MAX_UV];
                    tc[0] = [0.0, 0.0];
                    tc
                },
                num_texcoords: 1,
                ..Default::default()
            },
            UnpackedVertex {
                position: [1.0, 0.0, 0.0],
                normal: [0.0, 1.0, 0.0],
                texcoords: {
                    let mut tc = [[0.0; 2]; MAX_UV];
                    tc[0] = [1.0, 0.0];
                    tc
                },
                num_texcoords: 1,
                ..Default::default()
            },
            UnpackedVertex {
                position: [0.0, 1.0, 0.0],
                normal: [0.0, 1.0, 0.0],
                texcoords: {
                    let mut tc = [[0.0; 2]; MAX_UV];
                    tc[0] = [0.0, 1.0];
                    tc
                },
                num_texcoords: 1,
                ..Default::default()
            },
        ];

        let mut vertex_buffer = Vec::new();
        for v in &vertices {
            packer.pack_vertex(&mut vertex_buffer, v).unwrap();
        }

        let vert_size = packer.vertex_size() as i32;
        let vb_bytes = vertex_buffer.len() as i32;

        let section = Section {
            material_index: -1,
            accessory_index: -1,
            max_bones: 0,
            rigid_bone_index: -1,
            ib_offset: 0,
            num_tris: 1,
            vb_offset: 0,
            vb_bytes,
            vert_size,
            num_verts: 3,
            base_vert_packer: packer,
            bone_remap: Vec::new(),
            rigid_only: true,
            global_bones: false,
        };

        let bones = vec![
            Bone {
                name: "root".to_string(),
                parent_index: -1,
                model_to_bone: Matrix4x4::identity(),
            },
            Bone {
                name: "child".to_string(),
                parent_index: 0,
                model_to_bone: Matrix4x4::identity(),
            },
        ];

        UgxGeom {
            bounding_sphere: Sphere {
                center: [0.5, 0.5, 0.0],
                radius: 1.0,
            },
            bounds: AABB {
                min: [0.0, 0.0, 0.0],
                max: [1.0, 1.0, 0.0],
            },
            materials: Vec::new(),
            bones,
            granny_bones: Vec::new(),
            bone_bounds: vec![
                AABB {
                    min: [0.0, 0.0, 0.0],
                    max: [1.0, 1.0, 0.0],
                },
                AABB {
                    min: [-1.0, -1.0, -1.0],
                    max: [1.0, 1.0, 1.0],
                },
            ],
            sections: vec![section],
            vertex_buffer,
            index_buffer: vec![0, 1, 2],
            rigid_only: true,
            rigid_bone_index: 0,
            all_sections_rigid: true,
            all_sections_skinned: false,
            global_bones: false,
        }
    }

    #[test]
    fn test_write_read_roundtrip() {
        let original = make_test_geom();

        // Write to bytes
        let bytes = write_ugx(&original).unwrap();

        // Read back
        let read_back = UgxGeom::read(&bytes).unwrap();

        // Compare header fields
        assert_eq!(read_back.rigid_bone_index, original.rigid_bone_index);
        assert_eq!(read_back.rigid_only, original.rigid_only);
        assert_eq!(read_back.all_sections_rigid, original.all_sections_rigid);
        assert_eq!(
            read_back.all_sections_skinned,
            original.all_sections_skinned
        );
        assert_eq!(read_back.global_bones, original.global_bones);

        // Compare bounds
        assert_eq!(
            read_back.bounding_sphere.center,
            original.bounding_sphere.center
        );
        assert_eq!(
            read_back.bounding_sphere.radius,
            original.bounding_sphere.radius
        );
        assert_eq!(read_back.bounds.min, original.bounds.min);
        assert_eq!(read_back.bounds.max, original.bounds.max);

        // Compare sections
        assert_eq!(read_back.sections.len(), original.sections.len());
        let s_orig = &original.sections[0];
        let s_read = &read_back.sections[0];
        assert_eq!(s_read.material_index, s_orig.material_index);
        assert_eq!(s_read.num_tris, s_orig.num_tris);
        assert_eq!(s_read.num_verts, s_orig.num_verts);
        assert_eq!(s_read.vert_size, s_orig.vert_size);
        assert_eq!(s_read.vb_offset, s_orig.vb_offset);
        assert_eq!(s_read.vb_bytes, s_orig.vb_bytes);
        assert_eq!(s_read.ib_offset, s_orig.ib_offset);
        assert_eq!(
            s_read.base_vert_packer.pack_order,
            s_orig.base_vert_packer.pack_order
        );

        // Compare bones
        assert_eq!(read_back.bones.len(), original.bones.len());
        for (b_orig, b_read) in original.bones.iter().zip(read_back.bones.iter()) {
            assert_eq!(b_read.name, b_orig.name);
            assert_eq!(b_read.parent_index, b_orig.parent_index);
        }

        // Compare bone bounds
        assert_eq!(read_back.bone_bounds.len(), original.bone_bounds.len());
        for (bb_orig, bb_read) in original
            .bone_bounds
            .iter()
            .zip(read_back.bone_bounds.iter())
        {
            assert_eq!(bb_read.min, bb_orig.min);
            assert_eq!(bb_read.max, bb_orig.max);
        }

        // Compare vertex data (unpack and compare)
        let orig_verts = original.unpack_section_vertices(0).unwrap();
        let read_verts = read_back.unpack_section_vertices(0).unwrap();
        assert_eq!(read_verts.len(), orig_verts.len());
        for (v_orig, v_read) in orig_verts.iter().zip(read_verts.iter()) {
            assert_eq!(v_read.position, v_orig.position);
            assert_eq!(v_read.normal, v_orig.normal);
            assert_eq!(v_read.texcoords[0], v_orig.texcoords[0]);
        }

        // Compare indices
        let orig_indices = original.get_section_indices(0);
        let read_indices = read_back.get_section_indices(0);
        assert_eq!(read_indices, orig_indices);
    }

    #[test]
    fn test_write_read_materials_roundtrip() {
        let mut geom = make_test_geom();

        // Add materials with various properties and texture maps
        geom.materials = vec![
            Material {
                name: "terrain_grass".to_string(),
                spec_power: 25.0,
                flags: 3,
                blend_type: 1,
                opacity: 0.8,
                maps: {
                    let mut maps: [Vec<Map>; MapType::NUM_TYPES] = Default::default();
                    maps[MapType::Diffuse as usize] = vec![Map {
                        name: "art/textures/grass_diff.ddx".to_string(),
                        channel: 0,
                        flags: 7,
                    }];
                    maps[MapType::Normal as usize] = vec![Map {
                        name: "art/textures/grass_norm.ddx".to_string(),
                        channel: 0,
                        flags: 7,
                    }];
                    maps
                },
                uvw_velocity: [[0.0; 3]; MapType::NUM_TYPES],
            },
            Material {
                name: "metal_plate".to_string(),
                spec_power: 50.0,
                flags: 0,
                blend_type: 0,
                opacity: 1.0,
                maps: {
                    let mut maps: [Vec<Map>; MapType::NUM_TYPES] = Default::default();
                    maps[MapType::Diffuse as usize] = vec![Map {
                        name: "art/textures/metal_diff.ddx".to_string(),
                        channel: 0,
                        flags: 7,
                    }];
                    maps[MapType::Gloss as usize] = vec![Map {
                        name: "art/textures/metal_gloss.ddx".to_string(),
                        channel: 1,
                        flags: 3,
                    }];
                    maps
                },
                uvw_velocity: [[0.0; 3]; MapType::NUM_TYPES],
            },
        ];

        // Write to bytes
        let bytes = write_ugx(&geom).unwrap();

        // Read back
        let read_back = UgxGeom::read(&bytes).unwrap();

        // Verify material count
        assert_eq!(read_back.materials.len(), 2);

        // Verify first material
        let m0 = &read_back.materials[0];
        assert_eq!(m0.name, "terrain_grass");
        assert!((m0.spec_power - 25.0).abs() < 0.1);
        assert_eq!(m0.flags, 3);
        assert_eq!(m0.blend_type, 1);
        // Opacity roundtrips through UInt(0-255): 0.8 * 255 = 204, 204/255 = 0.8
        assert!((m0.opacity - 0.8).abs() < 0.01);

        // Verify first material's diffuse map
        assert_eq!(m0.maps[MapType::Diffuse as usize].len(), 1);
        assert_eq!(
            m0.maps[MapType::Diffuse as usize][0].name,
            "art/textures/grass_diff.ddx"
        );
        assert_eq!(m0.maps[MapType::Diffuse as usize][0].channel, 0);
        assert_eq!(m0.maps[MapType::Diffuse as usize][0].flags, 7);

        // Verify first material's normal map
        assert_eq!(m0.maps[MapType::Normal as usize].len(), 1);
        assert_eq!(
            m0.maps[MapType::Normal as usize][0].name,
            "art/textures/grass_norm.ddx"
        );

        // Verify second material
        let m1 = &read_back.materials[1];
        assert_eq!(m1.name, "metal_plate");
        assert!((m1.spec_power - 50.0).abs() < 0.1);
        assert_eq!(m1.flags, 0);
        assert_eq!(m1.blend_type, 0);
        assert!((m1.opacity - 1.0).abs() < 0.01);

        // Verify second material's gloss map
        assert_eq!(m1.maps[MapType::Gloss as usize].len(), 1);
        assert_eq!(
            m1.maps[MapType::Gloss as usize][0].name,
            "art/textures/metal_gloss.ddx"
        );
        assert_eq!(m1.maps[MapType::Gloss as usize][0].channel, 1);
        assert_eq!(m1.maps[MapType::Gloss as usize][0].flags, 3);

        // Verify empty map slots stay empty
        assert!(m0.maps[MapType::Gloss as usize].is_empty());
        assert!(m1.maps[MapType::Normal as usize].is_empty());
    }

    #[test]
    fn test_write_read_granny_bones_roundtrip() {
        let mut geom = make_test_geom();

        // Add granny bones with non-identity matrices
        geom.granny_bones = vec![
            GrannyBone {
                name: "root".to_string(),
                parent_index: -1,
                inverse_world_matrix: Matrix4x4 {
                    rows: [
                        [1.0, 0.0, 0.0, 0.0],
                        [0.0, 1.0, 0.0, 0.0],
                        [0.0, 0.0, 1.0, 0.0],
                        [0.0, 0.0, 0.0, 1.0],
                    ],
                },
            },
            GrannyBone {
                name: "spine".to_string(),
                parent_index: 0,
                inverse_world_matrix: Matrix4x4 {
                    rows: [
                        [1.0, 0.0, 0.0, 0.0],
                        [0.0, 0.0, 1.0, 0.0],
                        [0.0, -1.0, 0.0, 0.0],
                        [0.5, -2.0, 1.5, 1.0],
                    ],
                },
            },
        ];

        // Write to UGX bytes
        let bytes = write_ugx(&geom).unwrap();

        // Read back
        let read_back = UgxGeom::read(&bytes).unwrap();

        // Verify granny bones survived
        assert_eq!(read_back.granny_bones.len(), 2);

        let gb0 = &read_back.granny_bones[0];
        assert_eq!(gb0.name, "root");
        assert_eq!(gb0.parent_index, -1);
        // Identity matrix
        for row in 0..4 {
            for col in 0..4 {
                let expected = if row == col { 1.0 } else { 0.0 };
                assert!(
                    (gb0.inverse_world_matrix.rows[row][col] - expected).abs() < 1e-6,
                    "gb0 matrix[{}][{}] = {}, expected {}",
                    row,
                    col,
                    gb0.inverse_world_matrix.rows[row][col],
                    expected
                );
            }
        }

        let gb1 = &read_back.granny_bones[1];
        assert_eq!(gb1.name, "spine");
        assert_eq!(gb1.parent_index, 0);
        // Non-identity matrix
        let expected_rows = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0, 0.0],
            [0.5, -2.0, 1.5, 1.0],
        ];
        for row in 0..4 {
            for col in 0..4 {
                assert!(
                    (gb1.inverse_world_matrix.rows[row][col] - expected_rows[row][col]).abs()
                        < 1e-6,
                    "gb1 matrix[{}][{}] = {}, expected {}",
                    row,
                    col,
                    gb1.inverse_world_matrix.rows[row][col],
                    expected_rows[row][col]
                );
            }
        }
    }
}
