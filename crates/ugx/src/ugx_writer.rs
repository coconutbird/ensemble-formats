//! UGX file writer.
//!
//! Serializes a `UgxGeom` into UGX binary format (ECF container).
//! Writes chunks 0x700 (cached data), 0x701 (index buffer), 0x702 (vertex buffer).

use byteorder::{LittleEndian, WriteBytesExt};
use std::io::{Cursor, Seek, Write};

use crate::error::Result;
use crate::ugx::UgxGeom;

/// ECF chunk IDs for UGX.
const ECF_CACHED_DATA_CHUNK_ID: u64 = 0x00000700;
const ECF_IB_CHUNK_ID: u64 = 0x00000701;
const ECF_VB_CHUNK_ID: u64 = 0x00000702;

/// Geometry header signature.
const GEOM_HEADER_SIGNATURE: u32 = 0xC2340004;

/// Write a UGX geometry to bytes (ECF container).
pub fn write_ugx(geom: &UgxGeom) -> Result<Vec<u8>> {
    let cached_data = build_cached_data(geom)?;
    let ib_data = build_index_buffer(geom);

    let mut output = Cursor::new(Vec::new());
    let mut ecf = ecf::EcfWriter::new(&mut output, 0);

    ecf.add_chunk(ECF_CACHED_DATA_CHUNK_ID, cached_data);
    ecf.add_chunk(ECF_IB_CHUNK_ID, ib_data);
    ecf.add_chunk(ECF_VB_CHUNK_ID, geom.vertex_buffer.clone());

    ecf.finalize()?;

    Ok(output.into_inner())
}

/// Build the index buffer chunk (0x701): all u16 indices as little-endian bytes.
fn build_index_buffer(geom: &UgxGeom) -> Vec<u8> {
    let mut buf = Vec::with_capacity(geom.index_buffer.len() * 2);
    for &idx in &geom.index_buffer {
        buf.extend_from_slice(&idx.to_le_bytes());
    }
    buf
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

    for section in &geom.sections {
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

        // +0x28: BoneRemap packed array (16 bytes) — empty
        cursor.write_u32::<LittleEndian>(0)?; // count
        cursor.write_u32::<LittleEndian>(0)?; // pad
        cursor.write_u64::<LittleEndian>(0)?; // offset

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
    let mut string_offsets: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
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
    fixup_packed_array_header(&mut cursor, sections_header_pos, num_sections, sections_offset)?;
    fixup_packed_array_header(&mut cursor, bones_header_pos, num_bones, bones_offset)?;
    fixup_packed_array_header(&mut cursor, accessories_header_pos, 0, 0)?;
    fixup_packed_array_header(&mut cursor, valid_acc_header_pos, 0, 0)?;
    fixup_packed_array_header(&mut cursor, bounds_low_header_pos, num_bone_bounds, bounds_low_offset)?;
    fixup_packed_array_header(&mut cursor, bounds_high_header_pos, num_bone_bounds, bounds_high_offset)?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;
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
                texcoords: [[0.0, 0.0], [0.0; 2], [0.0; 2], [0.0; 2]],
                num_texcoords: 1,
                ..Default::default()
            },
            UnpackedVertex {
                position: [1.0, 0.0, 0.0],
                normal: [0.0, 1.0, 0.0],
                texcoords: [[1.0, 0.0], [0.0; 2], [0.0; 2], [0.0; 2]],
                num_texcoords: 1,
                ..Default::default()
            },
            UnpackedVertex {
                position: [0.0, 1.0, 0.0],
                normal: [0.0, 1.0, 0.0],
                texcoords: [[0.0, 1.0], [0.0; 2], [0.0; 2], [0.0; 2]],
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
                AABB { min: [0.0, 0.0, 0.0], max: [1.0, 1.0, 0.0] },
                AABB { min: [-1.0, -1.0, -1.0], max: [1.0, 1.0, 1.0] },
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
        assert_eq!(read_back.all_sections_skinned, original.all_sections_skinned);
        assert_eq!(read_back.global_bones, original.global_bones);

        // Compare bounds
        assert_eq!(read_back.bounding_sphere.center, original.bounding_sphere.center);
        assert_eq!(read_back.bounding_sphere.radius, original.bounding_sphere.radius);
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
        assert_eq!(s_read.base_vert_packer.pack_order, s_orig.base_vert_packer.pack_order);

        // Compare bones
        assert_eq!(read_back.bones.len(), original.bones.len());
        for (b_orig, b_read) in original.bones.iter().zip(read_back.bones.iter()) {
            assert_eq!(b_read.name, b_orig.name);
            assert_eq!(b_read.parent_index, b_orig.parent_index);
        }

        // Compare bone bounds
        assert_eq!(read_back.bone_bounds.len(), original.bone_bounds.len());
        for (bb_orig, bb_read) in original.bone_bounds.iter().zip(read_back.bone_bounds.iter()) {
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
}
